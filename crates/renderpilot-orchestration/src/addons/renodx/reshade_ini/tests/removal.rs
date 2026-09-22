use renderpilot_domain::{RenoDxConfigReceipt, RenoDxSetPathBaseline, RenoDxSetPathValue};

use crate::addons::renodx::reshade_ini::{plan_config, plan_config_removal, plan_set_path_removal};
use crate::addons::renodx::types::{RenoDxConfig, RenoDxConfigKey, RenoDxConfigSetting};

use super::ini_path;

#[test]
fn removal_of_absent_baseline_drops_only_the_created_empty_section() {
    let path = ini_path();
    let receipt = RenoDxConfigReceipt::new(
        path,
        RenoDxSetPathBaseline::Absent,
        false,
        RenoDxSetPathValue::One,
    );
    let removed = plan_set_path_removal(
        b"[General]\r\nKeep=1\r\n[renodx]\r\nSet_Path=1\r\n",
        &receipt,
    )
    .expect("remove set path");
    assert_eq!(
        String::from_utf8(removed.after.expect("after")).expect("utf8"),
        "[General]\r\nKeep=1\r\n"
    );
}

#[test]
fn config_tolerant_removal_continues_on_malformed_key() {
    let path = ini_path();
    let installed = plan_config(
        path.clone(),
        b"",
        None,
        Some(&RenoDxConfig {
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
        }),
    )
    .expect("install config");
    let malformed_owned = String::from_utf8(installed.after)
        .expect("utf8")
        .replace("Upgrade_R10G10B10A2_UNORM=1", "Upgrade_R10G10B10A2_UNORM")
        .into_bytes();
    let removed = plan_config_removal(&path, Some(&malformed_owned), &installed.receipt)
        .expect("tolerant removal preserves malformed key");
    let removed_text = String::from_utf8(removed.after.expect("after")).expect("utf8");
    assert!(removed_text.contains("Upgrade_R10G10B10A2_UNORM"));
    assert!(!removed_text.contains("ColorGradeContrast"));
}

#[test]
fn config_removal_restores_no_final_newline_for_existing_and_created_sections() {
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

    let existing = plan_config(path.clone(), b"[renodx]\nUser=keep", None, Some(&config))
        .expect("existing section install");
    let removed = plan_config_removal(&path, Some(&existing.after), &existing.receipt)
        .expect("existing section removal");
    assert!(removed.changed);
    assert_eq!(
        removed.after.as_deref(),
        Some(b"[renodx]\nUser=keep".as_slice())
    );

    let created =
        plan_config(path.clone(), b"Keep=1", None, Some(&config)).expect("created section install");
    let removed = plan_config_removal(&path, Some(&created.after), &created.receipt)
        .expect("created section removal");
    assert!(removed.changed);
    assert_eq!(removed.after.as_deref(), Some(b"Keep=1".as_slice()));
}

#[test]
fn config_removal_preserves_edited_key_and_removes_other_owned_keys() {
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
    let edited = String::from_utf8(installed.after)
        .expect("utf8")
        .replace("ColorGradeContrast=80", "ColorGradeContrast=81")
        .into_bytes();
    let removed =
        plan_config_removal(&path, Some(&edited), &installed.receipt).expect("tolerant removal");
    let text = String::from_utf8(removed.after.expect("after")).expect("utf8");
    assert!(!text.contains("Upgrade_R10G10B10A2"));
    assert!(text.contains("ColorGradeContrast=81"));
}
