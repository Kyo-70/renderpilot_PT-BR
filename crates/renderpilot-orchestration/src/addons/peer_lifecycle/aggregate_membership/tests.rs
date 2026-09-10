use super::*;

use renderpilot_domain::{
    AddonKind, FileReceipt, GameId, GameProxyTopology, InstalledAddon, ManagedAddonFile, PathRef,
    ProxyImplementation, ProxyLink, ProxyRootPrestate, Sha256Hash,
};

fn path(path: &std::path::Path) -> PathRef {
    PathRef::new(path.to_string_lossy().replace('\\', "/")).expect("path")
}

fn hash(value: char) -> Sha256Hash {
    Sha256Hash::new(value.to_string().repeat(64)).expect("hash")
}

fn topology(game: &std::path::Path, game_id: &GameId) -> GameProxyTopology {
    let root_slot = path(&game.join("dxgi.dll"));
    GameProxyTopology {
        id: "topology:aggregate-membership-package".to_owned(),
        game_id: game_id.clone(),
        root_slot: root_slot.clone(),
        outer: ProxyLink {
            implementation: ProxyImplementation::OptiScaler,
            path: root_slot,
            receipt: FileReceipt::owned("opti", hash('a')).expect("receipt"),
        },
        downstream: None,
        downstream_origin: None,
        root_prestate: ProxyRootPrestate::Absent,
    }
}

fn peer(
    game: &std::path::Path,
    game_id: &GameId,
    managed_path: &std::path::Path,
    managed_hash: Sha256Hash,
) -> InstalledAddon {
    InstalledAddon::new(
        game_id.clone(),
        AddonKind::Luma,
        path(&game.join("luma.addon64")),
    )
    .try_with_managed_files(vec![ManagedAddonFile::reused(
        path(managed_path),
        managed_hash,
    )])
    .expect("peer")
}

fn empty_peer(game: &std::path::Path, game_id: &GameId) -> InstalledAddon {
    InstalledAddon::new(
        game_id.clone(),
        AddonKind::Luma,
        path(&game.join("luma.addon64")),
    )
}

#[test]
fn package_seals_game_and_distinct_payload_roots_for_reused_membership() {
    let game = tempfile::tempdir().expect("game");
    let payload = tempfile::tempdir().expect("payload");
    let game_id = GameId::new("manual:aggregate-membership-package").expect("game id");
    let managed_path = payload.path().join("nvngx_dlss.dll");
    let managed_hash = hash('b');
    let before = peer(game.path(), &game_id, &managed_path, managed_hash.clone());
    let after = InstalledAddon::new(
        game_id.clone(),
        AddonKind::Luma,
        path(&game.path().join("luma.addon64")),
    )
    .try_with_managed_files(Vec::new())
    .expect("after");
    let topology = topology(game.path(), &game_id);

    let package = PeerAggregateMembershipPackage::plan_active(PeerAggregateMembershipRequest {
        before_peer: &before,
        after_peer: &after,
        unchanged_topology: &topology,
        game_root: game.path().to_path_buf(),
        payload_root: Some(payload.path().to_path_buf()),
    })
    .expect("package")
    .into_commit_inputs();

    assert_eq!(package.before_peer(), &before);
    assert_eq!(package.after_peer(), &after);
    assert_eq!(package.unchanged_topology(), &topology);
    assert_eq!(package.sealed_roots().len(), 2);
    assert_eq!(
        package.canonical_game_root(),
        package.sealed_roots().first().expect("game root")
    );
    assert_eq!(package.read_guards().len(), 1);
    assert_eq!(package.read_guards()[0].path(), &path(&managed_path));
    assert_eq!(
        package.read_guards()[0].expectation().sha256(),
        Some(&managed_hash)
    );
}

#[test]
fn package_accepts_a_reused_membership_addition() {
    let game = tempfile::tempdir().expect("game");
    let payload = tempfile::tempdir().expect("payload");
    let game_id = GameId::new("manual:aggregate-membership-addition").expect("game id");
    let managed_path = payload.path().join("nvngx_dlss.dll");
    let managed_hash = hash('e');
    let before = empty_peer(game.path(), &game_id);
    let after = peer(game.path(), &game_id, &managed_path, managed_hash.clone());
    let topology = topology(game.path(), &game_id);

    let package = PeerAggregateMembershipPackage::plan_active(PeerAggregateMembershipRequest {
        before_peer: &before,
        after_peer: &after,
        unchanged_topology: &topology,
        game_root: game.path().to_path_buf(),
        payload_root: Some(payload.path().to_path_buf()),
    })
    .expect("membership addition package")
    .into_commit_inputs();

    assert_eq!(package.before_peer(), &before);
    assert_eq!(package.after_peer(), &after);
    assert_eq!(package.read_guards().len(), 1);
    assert_eq!(package.read_guards()[0].path(), &path(&managed_path));
    assert_eq!(
        package.read_guards()[0].expectation().sha256(),
        Some(&managed_hash)
    );
}

#[test]
fn payload_root_cannot_authorize_topology_participant() {
    let game = tempfile::tempdir().expect("game");
    let payload = tempfile::tempdir().expect("payload");
    let game_id = GameId::new("manual:aggregate-membership-topology").expect("game id");
    let managed_path = payload.path().join("nvngx_dlss.dll");
    let before = peer(game.path(), &game_id, &managed_path, hash('c'));
    let after = InstalledAddon::new(
        game_id.clone(),
        AddonKind::Luma,
        path(&game.path().join("luma.addon64")),
    );
    let mut topology = topology(game.path(), &game_id);
    let outside_root_slot = path(&payload.path().join("dxgi.dll"));
    topology.root_slot = outside_root_slot.clone();
    topology.outer.path = outside_root_slot;

    assert!(
        PeerAggregateMembershipPackage::plan_active(PeerAggregateMembershipRequest {
            before_peer: &before,
            after_peer: &after,
            unchanged_topology: &topology,
            game_root: game.path().to_path_buf(),
            payload_root: Some(payload.path().to_path_buf()),
        })
        .is_err()
    );
}

#[test]
fn equal_roots_and_unsealed_membership_guard_fail_closed() {
    let game = tempfile::tempdir().expect("game");
    let outside = tempfile::tempdir().expect("outside");
    let game_id = GameId::new("manual:aggregate-membership-roots").expect("game id");
    let managed_path = outside.path().join("nvngx_dlss.dll");
    let before = peer(game.path(), &game_id, &managed_path, hash('d'));
    let after = InstalledAddon::new(
        game_id.clone(),
        AddonKind::Luma,
        path(&game.path().join("luma.addon64")),
    );
    let topology = topology(game.path(), &game_id);

    assert!(
        PeerAggregateMembershipPackage::plan_active(PeerAggregateMembershipRequest {
            before_peer: &before,
            after_peer: &after,
            unchanged_topology: &topology,
            game_root: game.path().to_path_buf(),
            payload_root: Some(game.path().join(".")),
        })
        .is_err()
    );
    assert!(
        PeerAggregateMembershipPackage::plan_active(PeerAggregateMembershipRequest {
            before_peer: &before,
            after_peer: &after,
            unchanged_topology: &topology,
            game_root: game.path().to_path_buf(),
            payload_root: None,
        })
        .is_err()
    );
}
