use super::*;
use crate::addons::matching::MatchConfidence;
use crate::peer_mutation_executor::PeerPathSnapshot;
use renderpilot_domain::{
    FileReceipt, GameProxyTopology, PathRef, ProxyImplementation, ProxyLink, ProxyRootPrestate,
    Sha256Hash,
};
use tempfile::tempdir;

fn topology(game_id: &GameId, root: &Path) -> GameProxyTopology {
    let root_slot =
        PathRef::new(root.join("dxgi.dll").to_string_lossy().into_owned()).expect("root slot");
    GameProxyTopology {
        id: "optiscaler:active-install-phase-test".to_owned(),
        game_id: game_id.clone(),
        root_slot: root_slot.clone(),
        outer: ProxyLink {
            implementation: ProxyImplementation::OptiScaler,
            path: root_slot,
            receipt: FileReceipt::owned("outer", Sha256Hash::new("a".repeat(64)).expect("digest"))
                .expect("receipt"),
        },
        downstream: None,
        downstream_origin: None,
        root_prestate: ProxyRootPrestate::Absent,
    }
}

fn snapshot(
    game_id: &GameId,
    root: &Path,
    host_snapshot: PeerPathSnapshot,
) -> ActiveInstallSnapshot {
    let game_root = root.to_path_buf();
    let host_path = PathRef::new(
        game_root
            .join("ReShade64.dll")
            .to_string_lossy()
            .into_owned(),
    )
    .expect("host path");
    ActiveInstallSnapshot {
        game_root,
        target_dir: root.to_path_buf(),
        asset: "Luma.zip".to_owned(),
        addon_file: "Luma.addon".to_owned(),
        arch: renderpilot_domain::Architecture::X64,
        proxy_dll_name: "dxgi.dll".to_owned(),
        external_requirement: None,
        writes_host: true,
        dgvoodoo_kind: DgVoodooPrepKind::None,
        topology: topology(game_id, root),
        host_path,
        host_snapshot,
    }
}

#[test]
fn phase_three_rejects_host_content_drift_even_when_the_path_is_unchanged() {
    let root = tempdir().expect("root");
    let host = root.path().join("ReShade64.dll");
    std::fs::write(&host, b"before").expect("before host");
    let host_path = PathRef::new(host.to_string_lossy().into_owned()).expect("host path");
    let root_ref = PathRef::new(root.path().to_string_lossy().into_owned()).expect("root");
    let before_host = observe_peer_path_snapshot(&host_path, &root_ref).expect("before");
    std::fs::write(&host, b"after").expect("after host");
    let current_host = observe_peer_path_snapshot(&host_path, &root_ref).expect("current");
    let game_id = GameId::new("manual:active-install-phase-test").expect("game id");
    let before = snapshot(&game_id, root.path(), before_host);
    let current = snapshot(&game_id, root.path(), current_host);

    assert!(ensure_snapshot_matches(&before, &current).is_err());
}

#[test]
fn preparation_kind_mismatch_is_rejected_before_fetch() {
    let plan = ResolvedLumaInstall {
        asset: "Luma.zip".to_owned(),
        addon_file: "Luma.addon".to_owned(),
        arch: renderpilot_domain::Architecture::X64,
        proxy_dll_name: "dxgi.dll".to_owned(),
        confidence: MatchConfidence::Verified,
        launch_args: Vec::new(),
        features: None,
        guidance: Vec::new(),
        external_requirement: Some(
            crate::addons::luma::test_support::sample_dgvoodoo_requirement(),
        ),
        profile: crate::addons::luma::types::LumaProfile::Game,
    };

    let error = preparation_for_plan(&plan, DgVoodooPrepKind::None)
        .expect_err("missing dgVoodoo preparation must fail closed");
    assert!(matches!(error, ServiceError::InvalidInput(_)));
}
