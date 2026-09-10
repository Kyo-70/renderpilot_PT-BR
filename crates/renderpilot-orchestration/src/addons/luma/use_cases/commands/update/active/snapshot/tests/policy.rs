use std::path::PathBuf;

use renderpilot_domain::{TrackedSource, TrackedSourceRole};

use super::super::super::model::DgVoodooLocalDecision;
use super::super::{dependency_paths, dgvoodoo_decision, managed_dgvoodoo_decision};
use super::{record, target};
use crate::addons::luma::test_support::sample_dgvoodoo_requirement;

#[test]
fn current_and_unknown_managed_runtimes_remain_distinct_for_network_policy() {
    assert_eq!(
        managed_dgvoodoo_decision(
            crate::addons::luma::dgvoodoo::OwnedDgVoodooStatus::Current,
            true,
        ),
        DgVoodooLocalDecision::Preserve { config_owned: true }
    );
    assert_eq!(
        managed_dgvoodoo_decision(
            crate::addons::luma::dgvoodoo::OwnedDgVoodooStatus::Unknown,
            true,
        ),
        DgVoodooLocalDecision::ReplaceOnFull { config_owned: true }
    );
}

#[test]
fn dependency_union_is_sorted_and_deduplicates_windows_path_identity() {
    let mut requirement = sample_dgvoodoo_requirement();
    let crate::addons::luma::types::LumaExternalRequirement::Dgvoodoo2 { install_map, .. } =
        &mut requirement;
    install_map.push(crate::addons::luma::types::ManagedInstallMapEntry {
        source: "MS/x86/D3D9.dll".to_owned(),
        dest: "d3D9.DLL".to_owned(),
        sha256: "different-entry".to_owned(),
        size: 1,
    });
    let record = record()
        .with_created_file(super::path("C:/Games/Snapshot/d3d9.dll"))
        .with_created_file(super::path("C:/Games/Snapshot/D3D8.dll"));
    let paths = dependency_paths(&target(Some(requirement)), &record);

    assert_eq!(
        paths,
        vec![
            PathBuf::from("C:/Games/Snapshot/D3D8.dll"),
            PathBuf::from("C:/Games/Snapshot/D3D9.dll"),
            PathBuf::from("C:/Games/Snapshot/dgVoodoo.conf"),
        ]
    );
}

#[test]
fn required_owned_unknown_runtime_is_deferred_to_full_repair_with_config_ownership() {
    let root = tempfile::tempdir().expect("root");
    let d3d9 = root.path().join("D3D9.dll");
    let config = root.path().join("dgVoodoo.conf");
    std::fs::write(&d3d9, b"not a dgVoodoo PE").expect("wrapper");
    std::fs::write(&config, b"[General]\n").expect("config");
    let record = renderpilot_domain::InstalledAddon::new(
        super::game_id(),
        renderpilot_domain::AddonKind::Luma,
        super::path(
            root.path()
                .join("Luma.addon64")
                .to_string_lossy()
                .into_owned(),
        ),
    )
    .with_created_file(super::path(d3d9.to_string_lossy().into_owned()))
    .with_created_file(super::path(config.to_string_lossy().into_owned()))
    .with_tracked_source(TrackedSource::new(
        TrackedSourceRole::DgVoodooWrapper,
        "https://example.invalid/dgvoodoo.zip",
        None,
        "archive",
    ));
    let target = super::target_at(
        root.path().to_path_buf(),
        Some(sample_dgvoodoo_requirement()),
    );
    let paths = dependency_paths(&target, &record);

    assert_eq!(
        dgvoodoo_decision(&target, &record, &paths),
        DgVoodooLocalDecision::ReplaceOnFull { config_owned: true }
    );
}

#[test]
fn required_owned_incomplete_runtime_is_replacement_and_keeps_config_authority() {
    let requirement = sample_dgvoodoo_requirement();
    let record = record()
        .with_created_file(super::path("C:/Games/Snapshot/D3D9.dll"))
        .with_created_file(super::path("C:/Games/Snapshot/dgVoodoo.conf"))
        .with_tracked_source(TrackedSource::new(
            TrackedSourceRole::DgVoodooWrapper,
            "https://example.invalid/dgvoodoo.zip",
            None,
            "archive",
        ));
    let target = target(Some(requirement));
    let paths = dependency_paths(&target, &record);

    assert_eq!(
        dgvoodoo_decision(&target, &record, &paths),
        DgVoodooLocalDecision::Replace { config_owned: true }
    );
}

#[test]
fn required_reused_runtime_without_record_authority_is_preserved() {
    let target = target(Some(sample_dgvoodoo_requirement()));
    let record = record();
    let paths = dependency_paths(&target, &record);

    assert_eq!(
        dgvoodoo_decision(&target, &record, &paths),
        DgVoodooLocalDecision::Preserve {
            config_owned: false
        }
    );
}

#[test]
fn obsolete_owned_runtime_is_removed_but_unrelated_history_is_preserved() {
    let target = target(None);
    let owned = record().with_created_file(super::path("C:/Games/Snapshot/D3D9.dll"));
    let owned_paths = dependency_paths(&target, &owned);
    assert_eq!(
        dgvoodoo_decision(&target, &owned, &owned_paths),
        DgVoodooLocalDecision::Remove
    );

    let clean = record();
    let clean_paths = dependency_paths(&target, &clean);
    assert_eq!(
        dgvoodoo_decision(&target, &clean, &clean_paths),
        DgVoodooLocalDecision::Preserve {
            config_owned: false
        }
    );
}
