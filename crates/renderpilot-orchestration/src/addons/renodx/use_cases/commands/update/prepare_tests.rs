use super::*;
use crate::addons::renodx::types::RenoDxProcessingPath;
use renderpilot_domain::{
    AddonKind, GameId, InstalledAddon, PathRef, RenoDxConfigReceipt, RenoDxSetPathBaseline,
    RenoDxSetPathValue,
};
use tempfile::tempdir;

fn snapshot(
    game_dir: &std::path::Path,
    processing_path: RenoDxProcessingPath,
    receipt: Option<RenoDxConfigReceipt>,
) -> UpdateSnapshot {
    let addon =
        PathRef::new(game_dir.join("renodx.addon64").to_string_lossy()).expect("addon path");
    let record = InstalledAddon::new(
        GameId::new("manual:renodx-config-prepare").expect("game id"),
        AddonKind::RenoDx,
        addon,
    )
    .with_renodx_config_receipt(receipt)
    .expect("receipt");
    UpdateSnapshot {
        record,
        game_dir: game_dir.to_path_buf(),
        processing_path,
        renodx_config: None,
        shared_vulkan_channel: None,
        addon: None,
        host: None,
        host_target: None,
    }
}

fn receipt(
    path: &std::path::Path,
    baseline: RenoDxSetPathBaseline,
    last: RenoDxSetPathValue,
) -> RenoDxConfigReceipt {
    let section_preexisted = matches!(baseline, RenoDxSetPathBaseline::Present { .. });
    RenoDxConfigReceipt::new(
        PathRef::new(path.to_string_lossy()).expect("ini path"),
        baseline,
        section_preexisted,
        last,
    )
}

#[test]
fn config_prepare_captures_baseline_and_reconciles_policy_flips() {
    let root = tempdir().expect("root");
    let ini = root.path().join("ReShade.ini");
    std::fs::write(&ini, b"[renodx]\nSet_Path=arbitrary\n").expect("ini");
    let prepared =
        prepare_config_update(&snapshot(root.path(), RenoDxProcessingPath::Native, None))
            .expect("capture")
            .expect("config projection");
    assert_eq!(
        prepared.after.as_deref(),
        Some(b"[renodx]\nSet_Path=0\n".as_slice())
    );
    assert_eq!(
        prepared.receipt.as_ref().expect("receipt").baseline,
        RenoDxSetPathBaseline::Present {
            value: "arbitrary".to_owned()
        }
    );

    let current = b"[renodx]\nSet_Path=1\n";
    std::fs::write(&ini, current).expect("reset ini");
    let record_snapshot = snapshot(
        root.path(),
        RenoDxProcessingPath::Native,
        Some(receipt(
            &ini,
            RenoDxSetPathBaseline::Present {
                value: "arbitrary".to_owned(),
            },
            RenoDxSetPathValue::One,
        )),
    );
    let prepared = prepare_config_update(&record_snapshot)
        .expect("flip")
        .expect("config projection");
    assert_eq!(
        prepared.after.as_deref(),
        Some(b"[renodx]\nSet_Path=0\n".as_slice())
    );
    assert_eq!(
        prepared.receipt.as_ref().expect("receipt").baseline,
        RenoDxSetPathBaseline::Present {
            value: "arbitrary".to_owned()
        }
    );

    std::fs::write(&ini, b"[renodx]\nSet_Path=user-edit\n").expect("edit ini");
    assert!(prepare_config_update(&record_snapshot).is_err());
}

#[test]
fn config_prepare_fails_closed_on_an_external_edit_when_relinquishing_path() {
    let root = tempdir().expect("root");
    let ini = root.path().join("ReShade.ini");
    let receipt = receipt(&ini, RenoDxSetPathBaseline::Absent, RenoDxSetPathValue::One);
    std::fs::write(&ini, b"[renodx]\nSet_Path=1\n").expect("ini");
    let prepared = prepare_config_update(&snapshot(
        root.path(),
        RenoDxProcessingPath::Unmanaged,
        Some(receipt.clone()),
    ))
    .expect("relinquish")
    .expect("config projection");
    assert_eq!(prepared.receipt, None);
    assert_eq!(prepared.after.as_deref(), Some(b"".as_slice()));

    std::fs::write(&ini, b"[renodx]\nSet_Path=user-edit\n").expect("edit ini");
    let error = prepare_config_update(&snapshot(
        root.path(),
        RenoDxProcessingPath::Unmanaged,
        Some(receipt),
    ))
    .expect_err("external edit must fail closed");
    assert!(error.to_string().contains("edited outside RenderPilot"));
}
