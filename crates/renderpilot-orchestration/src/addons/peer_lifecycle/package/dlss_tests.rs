use renderpilot_domain::{
    AddonKind, FileReceipt, GameId, GameProxyTopology, InstalledAddon, PathRef,
    PlannedGameProxyTopology, ProxyImplementation, ProxyLink, ProxyPeerRoute, ProxyRootPrestate,
    RenoDxDlssBeforeImage, RenoDxDlssClaim, RenoDxDlssProjection, Sha256Hash, TrackedSource,
    TrackedSourceRole,
};

use super::{PeerMutationPackage, RenoDxDlssMutationRequest};

fn path(path: &std::path::Path) -> PathRef {
    PathRef::new(path.to_string_lossy().replace('\\', "/")).expect("path")
}

fn hash(byte: char) -> Sha256Hash {
    Sha256Hash::new(byte.to_string().repeat(64)).expect("hash")
}

fn topology(root: &std::path::Path, game_id: &GameId) -> GameProxyTopology {
    let root_slot = path(&root.join("dxgi.dll"));
    GameProxyTopology {
        id: "topology:renodx-dlss-package".to_owned(),
        game_id: game_id.clone(),
        root_slot: root_slot.clone(),
        outer: ProxyLink {
            implementation: ProxyImplementation::OptiScaler,
            path: root_slot,
            receipt: FileReceipt::owned("optiscaler", hash('a')).expect("receipt"),
        },
        downstream: None,
        downstream_origin: None,
        root_prestate: ProxyRootPrestate::Absent,
    }
}

#[test]
fn typed_dlss_package_admits_projection_under_sealed_payload_root() {
    let game_root = tempfile::tempdir().expect("game root");
    let payload_root = tempfile::tempdir().expect("payload root");
    let game_id = GameId::new("manual:renodx-dlss-package").expect("game id");
    let addon = path(&payload_root.path().join("renodx-game.addon64"));
    let companion = path(&payload_root.path().join("renodx-dlssfix.addon64"));
    let before_source = TrackedSource::new(
        TrackedSourceRole::DlssFix,
        "https://example.test/dlss-fix-before",
        None,
        "digest-before",
    );
    let after_source = TrackedSource::new(
        TrackedSourceRole::DlssFix,
        "https://example.test/dlss-fix-after",
        None,
        "digest-after",
    );
    let before = InstalledAddon::new(game_id.clone(), AddonKind::RenoDx, addon)
        .with_created_file(companion.clone())
        .with_tracked_source(before_source.clone());
    let after = before
        .clone()
        .with_tracked_sources(vec![after_source.clone()]);
    let topology = topology(game_root.path(), &game_id);
    let planned = PlannedGameProxyTopology::Exact(topology.clone());
    let projection = RenoDxDlssProjection::new(
        companion.clone(),
        RenoDxDlssBeforeImage::absent(),
        RenoDxDlssClaim::new(true, Some(before_source)).expect("before claim"),
        RenoDxDlssClaim::new(true, Some(after_source)).expect("after claim"),
    );

    let package = PeerMutationPackage::plan_active_with_renodx_dlss(RenoDxDlssMutationRequest {
        before_peer: &before,
        after_peer: &after,
        before_topology: &topology,
        planned_after_topology: &planned,
        program: None,
        payloads: Vec::new(),
        game_root: game_root.path().to_path_buf(),
        payload_root: Some(payload_root.path().to_path_buf()),
        renodx_reshade_ini: None,
        projection: projection.clone(),
    })
    .expect("typed claim-only package");

    assert!(package.plan().program().endpoints().is_empty());
    assert!(package.contract().intents().is_empty());
    assert_eq!(package.route(), ProxyPeerRoute::DurableDisjoint);
    assert_eq!(package.renodx_dlss_projection(), Some(&projection));
    assert!(
        package
            .read_guards()
            .iter()
            .any(|guard| guard.path() == &companion)
    );
}

#[test]
fn generic_empty_program_constructor_remains_closed() {
    assert!(crate::peer_mutation_executor::ExactEndpointProgram::new(Vec::new()).is_err());
}
