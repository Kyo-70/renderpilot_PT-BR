use renderpilot_domain::{PathRef, RenoDxConfigReceipt, RenoDxSetPathBaseline, RenoDxSetPathValue};

use crate::addons::renodx::reshade_ini::{
    RenoDxConfigError, plan_config, plan_config_reconcile, plan_config_removal, plan_set_path,
    plan_set_path_reconcile,
};
use crate::addons::renodx::types::{RenoDxConfig, RenoDxConfigKey, RenoDxConfigSetting};

use super::ini_path;

#[test]
fn reconcile_obeys_receipt_cas_and_restores_original_value() {
    let path = ini_path();
    let receipt = RenoDxConfigReceipt::new(
        path.clone(),
        RenoDxSetPathBaseline::Present {
            value: "custom".to_owned(),
        },
        true,
        RenoDxSetPathValue::One,
    );
    let mismatch = plan_set_path_reconcile(
        path.clone(),
        Some(b"[renodx]\nSet_Path=custom-edit\n"),
        Some(RenoDxSetPathValue::Zero),
        Some(&receipt),
    );
    assert!(matches!(
        mismatch,
        Err(RenoDxConfigError::Ambiguous(
            "receipt-owned RenoDX configuration was edited outside RenderPilot"
        ))
    ));

    let restored =
        plan_set_path_reconcile(path, Some(b"[renodx]\nSet_Path=1\n"), None, Some(&receipt))
            .expect("restore");
    assert_eq!(restored.receipt, None);
    assert_eq!(
        String::from_utf8(restored.after.expect("after")).expect("utf8"),
        "[renodx]\nSet_Path=custom\n"
    );
}

#[test]
fn reconcile_captures_absent_file_and_keeps_user_bytes_when_value_is_unchanged() {
    let path = ini_path();
    let created = plan_set_path_reconcile(path.clone(), None, Some(RenoDxSetPathValue::Zero), None)
        .expect("create config");
    assert_eq!(
        created.after.as_deref(),
        Some(b"[renodx]\r\nSet_Path=0\r\n".as_slice())
    );
    assert_eq!(
        created.receipt.as_ref().expect("receipt").baseline,
        RenoDxSetPathBaseline::Absent
    );

    let receipt = created.receipt.expect("receipt");
    let user_bytes = b"[renodx]\nSet_Path=0\n; user edit\n";
    let no_op = plan_set_path_reconcile(
        path,
        Some(user_bytes),
        Some(RenoDxSetPathValue::Zero),
        Some(&receipt),
    )
    .expect("same policy");
    assert!(!no_op.changed);
    assert_eq!(no_op.after.as_deref(), Some(user_bytes.as_slice()));
}

#[test]
fn opaque_rhs_round_trips_through_reconcile_and_uninstall() {
    for original_value in ["", "a=b"] {
        let path = ini_path();
        let before = format!("[renodx]\nSet_Path={original_value}\n");
        let installed = plan_set_path(path.clone(), before.as_bytes(), RenoDxSetPathValue::One)
            .expect("install opaque RHS");
        let reconciled = plan_set_path_reconcile(
            path.clone(),
            Some(&installed.after),
            Some(RenoDxSetPathValue::Zero),
            Some(&installed.receipt),
        )
        .expect("reconcile opaque RHS");
        let receipt = reconciled.receipt.expect("updated receipt");
        let removed =
            plan_set_path_reconcile(path, reconciled.after.as_deref(), None, Some(&receipt))
                .expect("uninstall opaque RHS");
        assert_eq!(removed.after.as_deref(), Some(before.as_bytes()));
        assert!(removed.receipt.is_none());
    }
}

#[test]
fn reconcile_fails_closed_on_user_edit() {
    let path = ini_path();
    let receipt = RenoDxConfigReceipt::new(
        path.clone(),
        RenoDxSetPathBaseline::Absent,
        false,
        RenoDxSetPathValue::One,
    );
    let user_bytes = b"[renodx]\nSet_Path=custom\n";
    let error = plan_set_path_reconcile(path, Some(user_bytes), None, Some(&receipt))
        .expect_err("strict reconcile must fail closed on user edit");
    assert_eq!(
        error,
        RenoDxConfigError::Ambiguous(
            "receipt-owned RenoDX configuration was edited outside RenderPilot"
        )
    );
}

#[test]
fn install_then_update_then_uninstall_preserves_anchor_and_original_bytes() {
    let path = ini_path();
    let original = b"[General]\nfoo=bar";

    // Step 1: Install RenoDX with Set_Path=1 into file without trailing newline
    let install_step = plan_set_path_reconcile(
        path.clone(),
        Some(original),
        Some(RenoDxSetPathValue::One),
        None,
    )
    .expect("install reconcile succeeds");
    assert!(install_step.changed);
    let install_receipt = install_step.receipt.expect("install receipt");
    assert_eq!(install_receipt.newline_anchor.as_deref(), Some("foo=bar"));
    let after_install = install_step.after.expect("after install");

    // Step 2: Update desired to Set_Path=0
    let update_step = plan_set_path_reconcile(
        path.clone(),
        Some(&after_install),
        Some(RenoDxSetPathValue::Zero),
        Some(&install_receipt),
    )
    .expect("update reconcile succeeds");
    assert!(update_step.changed);
    let update_receipt = update_step.receipt.expect("update receipt");
    // Anchor MUST NOT be lost upon update!
    assert_eq!(update_receipt.newline_anchor.as_deref(), Some("foo=bar"));
    let after_update = update_step.after.expect("after update");

    // Step 3: Uninstall RenoDX (desired = None)
    let uninstall_step =
        plan_set_path_reconcile(path, Some(&after_update), None, Some(&update_receipt))
            .expect("uninstall reconcile succeeds");
    assert!(uninstall_step.changed);
    let after_uninstall = uninstall_step.after.expect("after uninstall");

    // Step 4: Verify exact byte-for-byte equality to original
    assert_eq!(after_uninstall.as_slice(), original);
}

#[test]
fn reconcile_evaluates_four_cas_states_strictly() {
    let path = ini_path();
    let receipt = RenoDxConfigReceipt::new(
        path.clone(),
        RenoDxSetPathBaseline::Absent,
        false,
        RenoDxSetPathValue::Zero,
    );

    // State 1: File absent with existing receipt -> Ambiguous (missing file)
    let err = plan_set_path_reconcile(
        path.clone(),
        None,
        Some(RenoDxSetPathValue::Zero),
        Some(&receipt),
    )
    .expect_err("missing file must fail");
    assert_eq!(
        err,
        RenoDxConfigError::Ambiguous("receipt-owned ReShade.ini is missing")
    );

    // State 2: Key absent from file -> Ambiguous (missing key)
    let empty_file = b"[renodx]\n";
    let err = plan_set_path_reconcile(
        path.clone(),
        Some(empty_file),
        Some(RenoDxSetPathValue::Zero),
        Some(&receipt),
    )
    .expect_err("missing key must fail");
    assert_eq!(
        err,
        RenoDxConfigError::Ambiguous(
            "receipt-owned RenoDX configuration was edited outside RenderPilot"
        )
    );

    // State 3: Value mismatch -> Ambiguous (edited outside RenderPilot)
    let user_edit = b"[renodx]\nSet_Path=999\n";
    let err = plan_set_path_reconcile(
        path.clone(),
        Some(user_edit),
        Some(RenoDxSetPathValue::Zero),
        Some(&receipt),
    )
    .expect_err("edited value must fail even on no-op path");
    assert_eq!(
        err,
        RenoDxConfigError::Ambiguous(
            "receipt-owned RenoDX configuration was edited outside RenderPilot"
        )
    );

    // Duplicate sections/keys fail-closed
    let duplicate_sections = b"[renodx]\nSet_Path=0\n[renodx]\nSet_Path=0\n";
    let err = plan_set_path_reconcile(
        path.clone(),
        Some(duplicate_sections),
        Some(RenoDxSetPathValue::Zero),
        Some(&receipt),
    )
    .expect_err("duplicate sections must fail");
    assert_eq!(
        err,
        RenoDxConfigError::Ambiguous("multiple [renodx] sections")
    );

    // State 4: Value matches receipt and desired matches -> Clean no-op
    let matching = b"[renodx]\nSet_Path=0\n";
    let no_op = plan_set_path_reconcile(
        path,
        Some(matching),
        Some(RenoDxSetPathValue::Zero),
        Some(&receipt),
    )
    .expect("matching state succeeds");
    assert!(!no_op.changed);
    assert_eq!(no_op.after.as_deref(), Some(matching.as_slice()));
}

#[test]
fn reconcile_uses_normalized_path_relation_for_windows_compatibility() {
    let win_path = PathRef::new(r"C:\Games\Cyberpunk\ReShade.ini").expect("win path");
    let unix_path = PathRef::new("c:/games/cyberpunk/reshade.ini").expect("unix path");

    let receipt = RenoDxConfigReceipt::new(
        win_path,
        RenoDxSetPathBaseline::Absent,
        false,
        RenoDxSetPathValue::Zero,
    );

    let file = b"[renodx]\nSet_Path=0\n";
    // Passing unix_path (different case/separators) to win_path receipt must compare equal.
    let reconciled = plan_set_path_reconcile(
        unix_path,
        Some(file),
        Some(RenoDxSetPathValue::Zero),
        Some(&receipt),
    )
    .expect("normalized path comparison must succeed");
    assert!(!reconciled.changed);
}

#[test]
fn config_reconcile_updates_and_removes_keys_with_per_key_cas() {
    let path = ini_path();
    let config = RenoDxConfig {
        settings: vec![
            RenoDxConfigSetting {
                key: RenoDxConfigKey::UpgradeR10G10B10A2Unorm,
                value: 1,
            },
            RenoDxConfigSetting {
                key: RenoDxConfigKey::ColorGradeContrast,
                value: 80,
            },
        ],
    };
    let updated_config = RenoDxConfig {
        settings: vec![RenoDxConfigSetting {
            key: RenoDxConfigKey::ColorGradeContrast,
            value: 90,
        }],
    };
    let empty_config = RenoDxConfig { settings: vec![] };
    let installed = plan_config(path.clone(), b"[renodx]\nUser=keep\n", None, Some(&config))
        .expect("install config");
    let updated = plan_config_reconcile(
        path.clone(),
        Some(&installed.after),
        None,
        Some(&updated_config),
        Some(&installed.receipt),
    )
    .expect("update config");
    let updated_text = String::from_utf8(updated.after.clone().expect("after")).expect("utf8");
    assert!(updated_text.contains("User=keep"));
    assert!(!updated_text.contains("Upgrade_R10G10B10A2_UNORM"));
    assert!(updated_text.contains("ColorGradeContrast=90"));
    let receipt = updated.receipt.expect("updated receipt");
    let removed = plan_config_reconcile(
        path,
        Some(updated_text.as_bytes()),
        None,
        Some(&empty_config),
        Some(&receipt),
    )
    .expect("uninstall config");
    assert_eq!(removed.receipt, None);
    assert_eq!(
        String::from_utf8(removed.after.expect("after")).expect("utf8"),
        "[renodx]\nUser=keep\n"
    );
}

#[test]
fn strict_config_reconcile_restores_no_final_newline_after_changed_update_and_empty_desired() {
    let path = ini_path();
    let config = RenoDxConfig {
        settings: vec![
            RenoDxConfigSetting {
                key: RenoDxConfigKey::UpgradeR10G10B10A2Unorm,
                value: 1,
            },
            RenoDxConfigSetting {
                key: RenoDxConfigKey::ColorGradeContrast,
                value: 80,
            },
        ],
    };
    let updated_config = RenoDxConfig {
        settings: vec![RenoDxConfigSetting {
            key: RenoDxConfigKey::ColorGradeContrast,
            value: 90,
        }],
    };
    let empty_config = RenoDxConfig { settings: vec![] };

    for original in [b"[renodx]\nUser=keep".as_slice(), b"Keep=1".as_slice()] {
        let installed = plan_config(path.clone(), original, None, Some(&config))
            .expect("install config into no-final-newline file");
        assert_eq!(
            installed.receipt.newline_anchor.as_deref(),
            Some(if original.starts_with(b"[") {
                "User=keep"
            } else {
                "Keep=1"
            },)
        );

        let updated = plan_config_reconcile(
            path.clone(),
            Some(&installed.after),
            None,
            Some(&updated_config),
            Some(&installed.receipt),
        )
        .expect("changed config update");
        let updated_receipt = updated.receipt.expect("updated receipt");
        let updated_after = updated.after.expect("updated bytes");
        assert!(updated.changed);

        let emptied = plan_config_reconcile(
            path.clone(),
            Some(&updated_after),
            None,
            Some(&empty_config),
            Some(&updated_receipt),
        )
        .expect("strict empty reconcile");
        assert!(emptied.changed);
        assert_eq!(emptied.receipt, None);
        assert_eq!(emptied.after.as_deref(), Some(original));
    }
}

#[test]
fn config_reconcile_rejects_malformed_owned_key() {
    let path = ini_path();
    let config = RenoDxConfig {
        settings: vec![RenoDxConfigSetting {
            key: RenoDxConfigKey::UpgradeR10G10B10A2Unorm,
            value: 1,
        }],
    };
    let installed = plan_config(path.clone(), b"", None, Some(&config)).expect("install");
    let edited = String::from_utf8(installed.after)
        .expect("utf8")
        .replace("Upgrade_R10G10B10A2_UNORM=1", "Upgrade_R10G10B10A2_UNORM")
        .into_bytes();
    assert!(matches!(
        plan_config_reconcile(
            path,
            Some(&edited),
            None,
            Some(&config),
            Some(&installed.receipt)
        ),
        Err(RenoDxConfigError::Ambiguous(
            "managed RenoDX configuration key has no assignment delimiter"
        ))
    ));
}

#[test]
fn v2_receipt_allows_legacy_set_path_mirror_with_newline_anchor() {
    let path = ini_path();
    let config = RenoDxConfig {
        settings: vec![RenoDxConfigSetting {
            key: RenoDxConfigKey::UpgradeR10G10B10A2Unorm,
            value: 2,
        }],
    };
    let original = b"[renodx]\nSet_Path=1";
    let installed = plan_config(
        path.clone(),
        original,
        Some(RenoDxSetPathValue::One),
        Some(&config),
    )
    .expect("append typed key");
    assert!(installed.receipt.is_supported());
    let reconciled = plan_config_reconcile(
        path.clone(),
        Some(&installed.after),
        Some(RenoDxSetPathValue::One),
        Some(&config),
        Some(&installed.receipt),
    )
    .expect("reconcile supported v2 receipt");
    assert!(!reconciled.changed);
    let removed = plan_config_removal(
        &path,
        Some(&reconciled.after.expect("after")),
        &installed.receipt,
    )
    .expect("remove supported v2 receipt");
    assert_eq!(removed.after.as_deref(), Some(original.as_slice()));
}

#[test]
fn config_reconcile_fails_closed_when_any_owned_key_was_edited() {
    let path = ini_path();
    let config = RenoDxConfig {
        settings: vec![
            RenoDxConfigSetting {
                key: RenoDxConfigKey::UpgradeR10G10B10A2Unorm,
                value: 1,
            },
            RenoDxConfigSetting {
                key: RenoDxConfigKey::ColorGradeContrast,
                value: 80,
            },
        ],
    };
    let installed = plan_config(path.clone(), b"", None, Some(&config)).expect("install config");
    let mut edited = installed.after;
    let text = String::from_utf8(edited.clone()).expect("utf8");
    edited = text
        .replace("ColorGradeContrast=80", "ColorGradeContrast=81")
        .into_bytes();
    let error = plan_config_reconcile(
        path,
        Some(&edited),
        None,
        Some(&config),
        Some(&installed.receipt),
    )
    .expect_err("external edit must fail closed");
    assert_eq!(
        error,
        RenoDxConfigError::Ambiguous(
            "receipt-owned RenoDX configuration was edited outside RenderPilot"
        )
    );
}
