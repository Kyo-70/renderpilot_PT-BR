mod policies;

use std::path::PathBuf;

use renderpilot_domain::{
    AddonKind, Architecture, FileReceipt, GameId, GameProxyTopology, PathRef, ProxyImplementation,
    ProxyLink, ProxyRootPrestate, TrackedSource, TrackedSourceRole, Version,
};

use super::{dgvoodoo, host, payload};
use super::{sources, sources::convert_dependency_paths};
use crate::addons::luma::use_cases::commands::update::active::model::{
    ActiveUpdatePhase1, DgVoodooLocalDecision,
};
use crate::peer_mutation_executor::PeerPathSnapshot;

fn source(role: TrackedSourceRole, url: &str) -> TrackedSource {
    TrackedSource::new(role, url, Some("etag".to_owned()), "digest")
}

fn path(value: &str) -> PathRef {
    PathRef::new(value).expect("valid path")
}

fn phase1() -> ActiveUpdatePhase1 {
    let game_id = GameId::new("prepare-test-game").expect("game id");
    let root_slot = path("C:/Games/Prepare/dxgi.dll");
    let downstream = path("C:/Games/Prepare/ReShade64.dll");
    let receipt = FileReceipt::owned("host-id", digest('a')).expect("receipt");
    let topology = GameProxyTopology {
        id: "prepare-topology".to_owned(),
        game_id: game_id.clone(),
        root_slot: root_slot.clone(),
        outer: ProxyLink {
            implementation: ProxyImplementation::OptiScaler,
            path: root_slot,
            receipt: FileReceipt::owned("outer-id", digest('b')).expect("receipt"),
        },
        downstream: Some(ProxyLink {
            implementation: ProxyImplementation::ReShade,
            path: downstream.clone(),
            receipt,
        }),
        downstream_origin: Some(path("C:/Games/Prepare/dxgi.dll")),
        root_prestate: ProxyRootPrestate::Absent,
    };
    let addon = path("C:/Games/Prepare/Luma.addon64");
    let record = renderpilot_domain::InstalledAddon::new(game_id, AddonKind::Luma, addon)
        .with_tracked_source(source(
            TrackedSourceRole::AddonPayload,
            "https://github.com/Filoppi/Luma-Framework/releases/latest/download/Luma-Game.zip",
        ));
    ActiveUpdatePhase1 {
        record,
        topology,
        target: crate::addons::luma::use_cases::update_target::ResolvedUpdateTarget {
            game_dir: PathBuf::from("C:/Games/Prepare"),
            asset: "Luma-Game.zip".to_owned(),
            addon_file: "Luma.addon64".to_owned(),
            arch: Architecture::X64,
            proxy_dll_name: "dxgi.dll".to_owned(),
            external_requirement: None,
        },
        stored_game_install_path: path("C:/Games/Prepare"),
        canonical_game_root: PathBuf::from("C:/Games/Prepare"),
        downstream_path: downstream,
        downstream_snapshot: PeerPathSnapshot::Absent,
        minimum_reshade_version: Version::parse("6.7.0").expect("version"),
        had_torn_marker: false,
        payload_disk_intact: true,
        dependency_paths: Vec::new(),
        dgvoodoo: DgVoodooLocalDecision::Preserve {
            config_owned: false,
        },
        host_replacement_required: false,
    }
}

fn digest(byte: char) -> renderpilot_domain::Sha256Hash {
    renderpilot_domain::Sha256Hash::new(byte.to_string().repeat(64)).expect("digest")
}

#[test]
fn payload_provenance_requires_exactly_one_source() {
    let missing = Vec::new();
    assert!(matches!(
        sources::require_payload(&missing),
        Err(crate::ServiceError::InvalidInput(_))
    ));

    let duplicate = vec![
        source(TrackedSourceRole::AddonPayload, "https://one.invalid"),
        source(TrackedSourceRole::AddonPayload, "https://two.invalid"),
    ];
    assert!(matches!(
        sources::require_payload(&duplicate),
        Err(crate::ServiceError::InvalidInput(_))
    ));
}

#[test]
fn payload_replacement_keeps_exact_position_and_unrelated_sources() {
    let mut sources = vec![
        source(TrackedSourceRole::DlssFix, "https://dlss.invalid"),
        source(TrackedSourceRole::AddonPayload, "https://old.invalid"),
        source(TrackedSourceRole::HostBinary, "https://host.invalid"),
    ];
    let payload = crate::addons::luma::fetch::types::LumaPayload {
        files: Vec::new(),
        main_addon_rel: "Luma.addon".to_owned(),
        zip_digest: "digest".to_owned(),
        etag: Some("new-etag".to_owned()),
        last_modified: Some("new-date".to_owned()),
        build_number: None,
    };

    sources::replace_payload(&mut sources, "Luma-Game.zip", &payload).expect("payload source");

    assert_eq!(sources[0].role(), TrackedSourceRole::DlssFix);
    assert_eq!(sources[1].role(), TrackedSourceRole::AddonPayload);
    assert_eq!(
        sources[1].url(),
        "https://github.com/Filoppi/Luma-Framework/releases/latest/download/Luma-Game.zip"
    );
    assert_eq!(sources[1].digest(), "digest");
    assert_eq!(sources[2].role(), TrackedSourceRole::HostBinary);
    assert!(!sources[1].is_advisory());
}

#[test]
fn host_addition_is_after_payload_and_preserves_later_roles() {
    let mut sources = vec![
        source(TrackedSourceRole::AddonPayload, "https://payload.invalid"),
        source(TrackedSourceRole::DlssFix, "https://dlss.invalid"),
    ];
    let host = source(TrackedSourceRole::HostBinary, "https://host.invalid");

    sources::replace_host(&mut sources, host).expect("host source");

    assert_eq!(
        sources.iter().map(TrackedSource::role).collect::<Vec<_>>(),
        vec![
            TrackedSourceRole::AddonPayload,
            TrackedSourceRole::HostBinary,
            TrackedSourceRole::DlssFix,
        ]
    );
}

#[test]
fn host_replacement_keeps_existing_position() {
    let mut sources = vec![
        source(TrackedSourceRole::HostBinary, "https://old-host.invalid"),
        source(TrackedSourceRole::AddonPayload, "https://payload.invalid"),
    ];
    sources::replace_host(
        &mut sources,
        source(TrackedSourceRole::HostBinary, "https://new-host.invalid"),
    )
    .expect("host source");

    assert_eq!(sources[0].url(), "https://new-host.invalid");
    assert_eq!(sources[1].role(), TrackedSourceRole::AddonPayload);
}

#[test]
fn dgvoodoo_replacement_and_removal_are_role_local() {
    let mut sources = vec![
        source(TrackedSourceRole::AddonPayload, "https://payload.invalid"),
        source(TrackedSourceRole::DgVoodooWrapper, "https://old-dg.invalid"),
        source(TrackedSourceRole::DlssFix, "https://dlss.invalid"),
    ];
    sources::replace_dgvoodoo(
        &mut sources,
        source(TrackedSourceRole::DgVoodooWrapper, "https://new-dg.invalid"),
    )
    .expect("dgVoodoo source");
    assert_eq!(sources[1].url(), "https://new-dg.invalid");

    sources::remove_role(&mut sources, TrackedSourceRole::DgVoodooWrapper);
    assert_eq!(sources.len(), 2);
    assert_eq!(sources[0].role(), TrackedSourceRole::AddonPayload);
    assert_eq!(sources[1].role(), TrackedSourceRole::DlssFix);
}

#[test]
fn dependency_paths_are_converted_without_reordering_or_deduplication() {
    let converted = convert_dependency_paths(&[
        std::path::PathBuf::from("C:/Game/D3D9.dll"),
        std::path::PathBuf::from("c:/game/d3d9.dll"),
    ])
    .expect("dependency paths");
    assert_eq!(converted[0], path("C:/Game/D3D9.dll"));
    assert_eq!(converted[1], path("c:/game/d3d9.dll"));
}

#[test]
fn invalid_dependency_path_fails_before_any_artifact_is_prepared() {
    let error = convert_dependency_paths(&[std::path::PathBuf::from("")])
        .expect_err("empty dependency path");
    assert!(matches!(error, crate::ServiceError::InvalidInput(_)));
}

#[tokio::test]
async fn force_full_is_decided_locally_without_a_head_request() {
    assert_eq!(
        payload::plan(&phase1(), true).await,
        Ok(payload::Plan::Full)
    );
}

#[tokio::test]
async fn torn_or_missing_payload_facts_select_full_without_network() {
    let mut torn = phase1();
    torn.had_torn_marker = true;
    assert_eq!(payload::plan(&torn, false).await, Ok(payload::Plan::Full));

    let mut missing = phase1();
    missing.payload_disk_intact = false;
    assert_eq!(
        payload::plan(&missing, false).await,
        Ok(payload::Plan::Full)
    );
}

#[test]
fn retained_host_provenance_matches_reused_and_owned_custody() {
    let mut reused = phase1();
    reused
        .topology
        .downstream
        .as_mut()
        .expect("downstream")
        .receipt = FileReceipt::reused("host-id", digest('a')).expect("receipt");
    assert_eq!(host::plan(&reused), Ok(host::Plan::Preserve));

    let mut owned = phase1();
    owned.record = owned.record.with_tracked_source(source(
        TrackedSourceRole::HostBinary,
        "https://reshade.invalid/host.zip",
    ));
    assert_eq!(host::plan(&owned), Ok(host::Plan::Preserve));

    assert!(host::plan(&phase1()).is_err());
}

#[test]
fn deferred_dgvoodoo_replacement_requires_full_payload_and_sealed_requirement() {
    let mut phase = phase1();
    phase.dgvoodoo = DgVoodooLocalDecision::ReplaceOnFull { config_owned: true };
    phase.record = phase.record.with_tracked_source(source(
        TrackedSourceRole::DgVoodooWrapper,
        "https://dgvoodoo.invalid/archive.zip",
    ));
    assert_eq!(dgvoodoo::plan(&phase, false), Ok(dgvoodoo::Plan::Preserve));
    assert!(dgvoodoo::plan(&phase, true).is_err());

    phase.dgvoodoo = DgVoodooLocalDecision::Remove;
    assert_eq!(dgvoodoo::plan(&phase, true), Ok(dgvoodoo::Plan::Remove));
}
