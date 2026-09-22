use renderpilot_domain::{RenoDxSetPathBaseline, RenoDxSetPathValue};

use crate::addons::renodx::reshade_ini::{RenoDxConfigError, plan_config, plan_set_path};
use crate::addons::renodx::types::{RenoDxConfig, RenoDxConfigKey, RenoDxConfigSetting};

use super::ini_path;

#[test]
fn set_path_planner_captures_absent_and_existing_baselines() {
    let absent =
        plan_set_path(ini_path(), b"[General]\r\n", RenoDxSetPathValue::One).expect("absent key");
    assert_eq!(absent.receipt.baseline, RenoDxSetPathBaseline::Absent);
    assert_eq!(
        String::from_utf8(absent.after).expect("utf8"),
        "[General]\r\n[renodx]\r\nSet_Path=1\r\n"
    );

    let existing = plan_set_path(
        ini_path(),
        b"[renodx]\nSet_Path = arbitrary\nOther=keep\n",
        RenoDxSetPathValue::Zero,
    )
    .expect("existing key");
    assert_eq!(
        existing.receipt.baseline,
        RenoDxSetPathBaseline::Present {
            value: "arbitrary".to_owned()
        }
    );
    assert_eq!(
        String::from_utf8(existing.after).expect("utf8"),
        "[renodx]\nSet_Path = 0\nOther=keep\n"
    );

    for original in [
        b"[renodx]\nSet_Path=\n".as_slice(),
        b"[renodx]\nSet_Path=a=b\n".as_slice(),
    ] {
        let planned = plan_set_path(ini_path(), original, RenoDxSetPathValue::Zero)
            .expect("opaque baseline RHS");
        let expected = if original.ends_with(b"=\n") {
            ""
        } else {
            "a=b"
        };
        assert_eq!(
            planned.receipt.baseline,
            RenoDxSetPathBaseline::Present {
                value: expected.to_owned()
            }
        );
    }
}

#[test]
fn set_path_planner_rejects_ambiguous_or_unsafe_input() {
    assert!(matches!(
        plan_set_path(
            ini_path(),
            b"[renodx]\nSet_Path=0\nSet_Path=1\n",
            RenoDxSetPathValue::One
        ),
        Err(RenoDxConfigError::Ambiguous("duplicate configuration keys"))
    ));
    assert!(matches!(
        plan_set_path(
            ini_path(),
            b"[renodx\nSet_Path=0\n",
            RenoDxSetPathValue::One
        ),
        Err(RenoDxConfigError::Ambiguous("malformed section header"))
    ));
    assert!(matches!(
        plan_set_path(
            ini_path(),
            b"[renodx]\n[broken\nSet_Path=0\n",
            RenoDxSetPathValue::One
        ),
        Err(RenoDxConfigError::Ambiguous("malformed section header"))
    ));
    assert!(matches!(
        plan_set_path(
            ini_path(),
            b"[renodx]\nSet_Path=0\n[General]\nX=1\n[renodx\nSet_Path=1\n",
            RenoDxSetPathValue::One
        ),
        Err(RenoDxConfigError::Ambiguous("malformed section header"))
    ));
    for malformed_key in [b"Set_Path\n".as_slice(), b"Set_Path; comment\n".as_slice()] {
        let input = [&b"[renodx]\n"[..], malformed_key].concat();
        assert!(matches!(
            plan_set_path(ini_path(), &input, RenoDxSetPathValue::One),
            Err(RenoDxConfigError::Ambiguous(
                "managed RenoDX configuration key has no assignment delimiter"
            ))
        ));
    }
    let commented = plan_set_path(
        ini_path(),
        b"[renodx]\n; Set_Path\n# [broken\nSet_Path=0\n",
        RenoDxSetPathValue::One,
    )
    .expect("comments are opaque");
    assert!(
        String::from_utf8(commented.after)
            .expect("utf8")
            .contains("; Set_Path\n# [broken\nSet_Path=1\n")
    );
    let foreign_malformed = b"[broken\nOpaque=keep\n[General]\nValue=1\n";
    let preserved = plan_set_path(ini_path(), foreign_malformed, RenoDxSetPathValue::One)
        .expect("unrelated malformed section is opaque");
    assert!(
        String::from_utf8(preserved.after)
            .expect("utf8")
            .starts_with("[broken\nOpaque=keep\n[General]\nValue=1\n")
    );
    let foreign_after_target = b"[renodx]\nSet_Path=0\n[General]\nX=1\n[broken\nOpaque=keep\n";
    let preserved_after_target =
        plan_set_path(ini_path(), foreign_after_target, RenoDxSetPathValue::One)
            .expect("unrelated malformed section after target is opaque");
    assert!(
        String::from_utf8(preserved_after_target.after)
            .expect("utf8")
            .contains("[broken\nOpaque=keep\n")
    );
    assert!(matches!(
        plan_set_path(ini_path(), &[0xff], RenoDxSetPathValue::One),
        Err(RenoDxConfigError::NonUtf8)
    ));
}

#[test]
fn config_planner_writes_set_path_and_multiple_typed_keys_atomically() {
    let path = ini_path();
    let before = b"\xEF\xBB\xBF[General]\r\nKeep=1";
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
    let planned = plan_config(path, before, Some(RenoDxSetPathValue::One), Some(&config))
        .expect("multi-key config should plan");
    let text = String::from_utf8(planned.after.clone()).expect("utf8");
    assert!(text.starts_with("\u{feff}[General]\r\nKeep=1\r\n[renodx]\r\n"));
    assert!(text.contains("Set_Path=1\r\n"));
    assert!(text.contains("Upgrade_R10G10B10A2_UNORM=1\r\n"));
    assert!(text.contains("ColorGradeContrast=80\r\n"));
    assert_eq!(planned.receipt.entries.len(), 3);
}

#[test]
fn config_planner_rejects_unknown_duplicate_and_malformed_inputs() {
    let path = ini_path();
    let duplicate_config = RenoDxConfig {
        settings: vec![
            RenoDxConfigSetting {
                key: RenoDxConfigKey::ColorGradeContrast,
                value: 80,
            },
            RenoDxConfigSetting {
                key: RenoDxConfigKey::ColorGradeContrast,
                value: 90,
            },
        ],
    };
    assert!(matches!(
        plan_config(path.clone(), b"", None, Some(&duplicate_config)),
        Err(RenoDxConfigError::Ambiguous("duplicate configuration keys"))
    ));
    let malformed_config = RenoDxConfig {
        settings: vec![RenoDxConfigSetting {
            key: RenoDxConfigKey::UpgradeR10G10B10A2Unorm,
            value: 1,
        }],
    };
    assert!(matches!(
        plan_config(
            path,
            b"[renodx\nUpgrade_R10G10B10A2_UNORM=1\n",
            None,
            Some(&malformed_config)
        ),
        Err(RenoDxConfigError::Ambiguous("malformed section header"))
    ));
}

#[test]
fn config_planner_rejects_malformed_any_owned_key() {
    let path = ini_path();
    let config = RenoDxConfig {
        settings: vec![RenoDxConfigSetting {
            key: RenoDxConfigKey::UpgradeR10G10B10A2Unorm,
            value: 1,
        }],
    };
    let malformed = b"[renodx]\nUpgrade_R10G10B10A2_UNORM\n";
    assert!(matches!(
        plan_config(path, malformed, None, Some(&config)),
        Err(RenoDxConfigError::Ambiguous(
            "managed RenoDX configuration key has no assignment delimiter"
        ))
    ));
}
