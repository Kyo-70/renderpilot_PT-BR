use super::*;

use renderpilot_application::{GameRepository, InstalledAddonRepository, ProxyTopologyRepository};
use renderpilot_domain::{
    AddonKind, FileReceipt, GameId, GameIdentity, GameInstallation, GameProxyTopology, GameRuntime,
    InstalledAddon, Launcher, ManagedAddonFile, PathRef, Platform, ProxyImplementation, ProxyLink,
    ProxyRootPrestate, Sha256Hash,
};
use sha2::{Digest as _, Sha256};

fn path(path: &std::path::Path) -> PathRef {
    PathRef::new(path.to_string_lossy().replace('\\', "/")).expect("path")
}

fn hash(value: char) -> Sha256Hash {
    Sha256Hash::new(value.to_string().repeat(64)).expect("hash")
}

fn bytes_hash(bytes: &[u8]) -> Sha256Hash {
    Sha256Hash::new(hex::encode(Sha256::digest(bytes))).expect("hash")
}

fn topology(game: &std::path::Path, game_id: &GameId) -> GameProxyTopology {
    let root_slot = path(&game.join("dxgi.dll"));
    GameProxyTopology {
        id: "topology:aggregate-membership-executor-success".to_owned(),
        game_id: game_id.clone(),
        root_slot: root_slot.clone(),
        outer: ProxyLink {
            implementation: ProxyImplementation::OptiScaler,
            path: root_slot,
            receipt: FileReceipt::owned("opti", hash('b')).expect("receipt"),
        },
        downstream: None,
        downstream_origin: None,
        root_prestate: ProxyRootPrestate::Absent,
    }
}

enum LiveState {
    Missing,
    Directory,
    File(Vec<u8>),
}

struct MembershipFixture {
    _root: tempfile::TempDir,
    context: crate::Context,
    game_id: GameId,
    live: std::path::PathBuf,
    before: InstalledAddon,
    after: InstalledAddon,
    topology: GameProxyTopology,
}

fn membership_fixture(state: LiveState, accepted_hash: Sha256Hash) -> MembershipFixture {
    let root = tempfile::tempdir().expect("game");
    let live = root.path().join("nvngx_dlss.dll");
    match state {
        LiveState::Missing => {}
        LiveState::Directory => std::fs::create_dir(&live).expect("directory"),
        LiveState::File(bytes) => std::fs::write(&live, bytes).expect("live file"),
    }
    let game_id = GameId::generate();
    let game_record = GameInstallation::new(
        GameIdentity::new(game_id.clone(), "Aggregate membership", Launcher::Manual)
            .expect("identity"),
        Platform::Windows,
        GameRuntime::NativeWindows,
        path(root.path()),
    );
    let before = InstalledAddon::new(
        game_id.clone(),
        AddonKind::Luma,
        path(&root.path().join("luma.addon64")),
    )
    .try_with_managed_files(vec![ManagedAddonFile::reused(path(&live), accepted_hash)])
    .expect("before");
    let after = InstalledAddon::new(
        game_id.clone(),
        AddonKind::Luma,
        path(&root.path().join("luma.addon64")),
    );
    let topology = topology(root.path(), &game_id);
    let db = root.path().join("catalog.sqlite");
    let context = crate::Context::open_at(&db).expect("context");
    context.storage().upsert_game(&game_record).expect("game");
    context
        .storage()
        .upsert_installed_addon(&before)
        .expect("peer");
    let before = context
        .storage()
        .get_installed_addon(&game_id)
        .expect("load peer")
        .expect("peer exists");
    let connection = rusqlite::Connection::open(&db).expect("fixture connection");
    connection
        .execute(
            "INSERT INTO game_proxy_topologies (game_id, id, topology_json) VALUES (?1, ?2, ?3)",
            rusqlite::params![
                game_id.as_str(),
                topology.id.as_str(),
                serde_json::to_string(&topology).expect("topology json")
            ],
        )
        .expect("topology");

    MembershipFixture {
        _root: root,
        context,
        game_id,
        live,
        before,
        after,
        topology,
    }
}

fn package_for(
    fixture: &MembershipFixture,
) -> crate::addons::peer_lifecycle::PeerAggregateMembershipPackage {
    crate::addons::peer_lifecycle::PeerAggregateMembershipPackage::plan_active(
        crate::addons::peer_lifecycle::PeerAggregateMembershipRequest {
            before_peer: &fixture.before,
            after_peer: &fixture.after,
            unchanged_topology: &fixture.topology,
            game_root: fixture._root.path().to_path_buf(),
            payload_root: None,
        },
    )
    .expect("package")
}

fn assert_fixture_unchanged(fixture: &MembershipFixture) {
    assert_eq!(
        fixture
            .context
            .storage()
            .get_installed_addon(&fixture.game_id)
            .expect("peer"),
        Some(fixture.before.clone())
    );
    assert_eq!(
        fixture
            .context
            .storage()
            .get_proxy_topology(&fixture.game_id)
            .expect("topology"),
        Some(fixture.topology.clone())
    );
    assert!(
        fixture
            .context
            .storage()
            .pending_file_mutations_for_game(&fixture.game_id)
            .expect("pending")
            .is_empty()
    );
}

#[test]
fn successful_membership_change_commits_only_peer_and_keeps_live_file_and_topology() {
    let game = tempfile::tempdir().expect("game");
    let db = game.path().join("catalog.sqlite");
    let live = game.path().join("nvngx_dlss.dll");
    let bytes = b"accepted reused bytes";
    std::fs::write(&live, bytes).expect("live file");
    let game_id = GameId::new("manual:aggregate-membership-executor-success").expect("game id");
    let game_record = GameInstallation::new(
        GameIdentity::new(game_id.clone(), "Aggregate membership", Launcher::Manual)
            .expect("identity"),
        Platform::Windows,
        GameRuntime::NativeWindows,
        path(game.path()),
    );
    let before = InstalledAddon::new(
        game_id.clone(),
        AddonKind::Luma,
        path(&game.path().join("luma.addon64")),
    )
    .try_with_managed_files(vec![ManagedAddonFile::reused(
        path(&live),
        bytes_hash(bytes),
    )])
    .expect("before");
    let after = InstalledAddon::new(
        game_id.clone(),
        AddonKind::Luma,
        path(&game.path().join("luma.addon64")),
    );
    let topology = topology(game.path(), &game_id);

    let context = crate::Context::open_at(&db).expect("context");
    context.storage().upsert_game(&game_record).expect("game");
    context
        .storage()
        .upsert_installed_addon(&before)
        .expect("peer");
    let before = context
        .storage()
        .get_installed_addon(&game_id)
        .expect("load peer")
        .expect("peer exists");
    // Topology insertion is deliberately kept in the test fixture: the
    // production repository exposes reads only, while lifecycle writes own
    // topology through their aggregate commit boundary.
    let connection = rusqlite::Connection::open(&db).expect("fixture connection");
    connection
        .execute(
            "INSERT INTO game_proxy_topologies (game_id, id, topology_json) VALUES (?1, ?2, ?3)",
            rusqlite::params![
                game_id.as_str(),
                topology.id.as_str(),
                serde_json::to_string(&topology).expect("topology json")
            ],
        )
        .expect("topology");
    drop(connection);

    let package = crate::addons::peer_lifecycle::PeerAggregateMembershipPackage::plan_active(
        crate::addons::peer_lifecycle::PeerAggregateMembershipRequest {
            before_peer: &before,
            after_peer: &after,
            unchanged_topology: &topology,
            game_root: game.path().to_path_buf(),
            payload_root: None,
        },
    )
    .expect("package");
    let guard = crate::game_mutation_lock::blocking_lock(&game_id);
    context
        .peer_mutation_executor()
        .commit_reused_claim_membership(&guard, package)
        .expect("membership change");

    assert_eq!(std::fs::read(&live).expect("live file"), bytes);
    assert_eq!(
        context
            .storage()
            .get_proxy_topology(&game_id)
            .expect("topology"),
        Some(topology)
    );
    assert_eq!(
        context
            .storage()
            .get_installed_addon(&game_id)
            .expect("peer")
            .expect("peer exists")
            .managed_files(),
        after.managed_files()
    );
    assert!(
        context
            .storage()
            .pending_file_mutations_for_game(&game_id)
            .expect("pending")
            .is_empty()
    );
}

#[test]
fn missing_reused_live_guard_fails_before_storage_and_leaves_everything_exact() {
    let fixture = membership_fixture(LiveState::Missing, hash('a'));
    let package = package_for(&fixture);
    let guard = crate::game_mutation_lock::blocking_lock(&fixture.game_id);

    let error = fixture
        .context
        .peer_mutation_executor()
        .commit_reused_claim_membership(&guard, package)
        .expect_err("missing reused claim must fail closed");
    assert!(error.to_string().contains("read-guard"));
    assert_fixture_unchanged(&fixture);
}

#[test]
fn directory_reused_live_guard_fails_without_following_or_persisting() {
    let fixture = membership_fixture(LiveState::Directory, hash('a'));
    let package = package_for(&fixture);
    let guard = crate::game_mutation_lock::blocking_lock(&fixture.game_id);

    assert!(
        fixture
            .context
            .peer_mutation_executor()
            .commit_reused_claim_membership(&guard, package)
            .is_err()
    );
    assert_fixture_unchanged(&fixture);
}

#[test]
fn wrong_reused_live_digest_fails_before_storage_and_preserves_user_bytes() {
    let actual = b"user-edited bytes".to_vec();
    let fixture = membership_fixture(LiveState::File(actual.clone()), hash('a'));
    let package = package_for(&fixture);
    let guard = crate::game_mutation_lock::blocking_lock(&fixture.game_id);

    assert!(
        fixture
            .context
            .peer_mutation_executor()
            .commit_reused_claim_membership(&guard, package)
            .is_err()
    );
    assert_eq!(std::fs::read(&fixture.live).expect("live file"), actual);
    assert_fixture_unchanged(&fixture);
}

#[test]
fn between_observation_drift_fails_closed_after_prepare_without_touching_user_file() {
    let original = b"accepted reused bytes";
    let changed = b"user-edited after prepare".to_vec();
    let fixture = membership_fixture(LiveState::File(original.to_vec()), bytes_hash(original));
    let changed_path = fixture.live.clone();
    set_between_observation_hook(move || {
        std::fs::write(changed_path, &changed).expect("change user file");
    });
    let package = package_for(&fixture);
    let guard = crate::game_mutation_lock::blocking_lock(&fixture.game_id);

    let result = fixture
        .context
        .peer_mutation_executor()
        .commit_reused_claim_membership(&guard, package);
    clear_between_observation_hook();

    assert!(result.is_err(), "drift must reject the aggregate commit");
    assert_eq!(
        std::fs::read(&fixture.live).expect("live file"),
        b"user-edited after prepare"
    );
    assert_fixture_unchanged(&fixture);
}

#[test]
fn rejects_guard_game_mismatch_before_observation_or_storage() {
    let game = tempfile::tempdir().expect("game");
    let game_id = GameId::new("manual:aggregate-membership-executor").expect("game id");
    let other_game_id = GameId::new("manual:aggregate-membership-other").expect("other game id");
    let live = game.path().join("nvngx_dlss.dll");
    std::fs::write(&live, b"accepted").expect("live file");

    let before = InstalledAddon::new(
        game_id.clone(),
        AddonKind::Luma,
        path(&game.path().join("luma.addon64")),
    )
    .try_with_managed_files(vec![ManagedAddonFile::reused(path(&live), hash('a'))])
    .expect("before");
    let after = InstalledAddon::new(
        game_id.clone(),
        AddonKind::Luma,
        path(&game.path().join("luma.addon64")),
    );
    let root_slot = path(&game.path().join("dxgi.dll"));
    let topology = GameProxyTopology {
        id: "topology:aggregate-membership-executor".to_owned(),
        game_id,
        root_slot: root_slot.clone(),
        outer: ProxyLink {
            implementation: ProxyImplementation::OptiScaler,
            path: root_slot,
            receipt: FileReceipt::owned("opti", hash('b')).expect("receipt"),
        },
        downstream: None,
        downstream_origin: None,
        root_prestate: ProxyRootPrestate::Absent,
    };
    let package = crate::addons::peer_lifecycle::PeerAggregateMembershipPackage::plan_active(
        crate::addons::peer_lifecycle::PeerAggregateMembershipRequest {
            before_peer: &before,
            after_peer: &after,
            unchanged_topology: &topology,
            game_root: game.path().to_path_buf(),
            payload_root: None,
        },
    )
    .expect("package");
    let storage = renderpilot_storage_sqlite::SqliteStorage::in_memory().expect("storage");
    let executor = PeerMutationExecutor::new(storage);
    let guard = crate::game_mutation_lock::blocking_lock(&other_game_id);

    let error = executor
        .commit_reused_claim_membership(&guard, package)
        .expect_err("foreign game guard must fail closed");
    assert!(error.to_string().contains("guarded game"));
    assert_eq!(std::fs::read(&live).expect("live file"), b"accepted");
}
