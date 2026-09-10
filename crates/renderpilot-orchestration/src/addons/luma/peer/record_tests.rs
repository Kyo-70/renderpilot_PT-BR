use renderpilot_application::{GameRepository, InstalledAddonRepository, ProxyTopologyRepository};
use renderpilot_domain::{
    AddonKind, FileReceipt, GameId, GameIdentity, GameInstallation, GameProxyTopology, GameRuntime,
    InstalledAddon, Launcher, OptiScalerAdoptionState, OptiScalerConfigurationBaseline,
    OptiScalerFileCleanup, OptiScalerFileReceipt, OptiScalerFileRole, OptiScalerInstallStateParts,
    PathRef, Platform, ProxyImplementation, ProxyLink, ProxyRootPrestate, TrackedSource,
    TrackedSourceRole,
};
use renderpilot_storage_sqlite::{
    GameMutationCommit, InstalledAddonMutation, OptiScalerAggregateMutation,
};
use tempfile::TempDir;

use super::commit_metadata;
use crate::Context;

struct Fixture {
    _database: TempDir,
    _game_root: TempDir,
    context: Context,
    game_id: GameId,
    peer: InstalledAddon,
    topology: GameProxyTopology,
}

fn path(root: &std::path::Path, name: &str) -> PathRef {
    PathRef::new(root.join(name).to_string_lossy().replace('\\', "/")).expect("path")
}

fn exact_reused_receipt(path: &std::path::Path) -> FileReceipt {
    let (parent, leaf) = crate::fs::verified_parent(path).expect("verified parent");
    let observation = parent
        .observe_leaf(&leaf)
        .expect("observe receipt")
        .expect("receipt file");
    FileReceipt::reused(
        observation.identity,
        renderpilot_detection::sha256_file(path).expect("receipt digest"),
    )
    .expect("reused receipt")
}

fn fixture(advisory: bool) -> Fixture {
    let database = tempfile::tempdir().expect("database");
    let game_root = tempfile::tempdir().expect("game root");
    let context = Context::open_at(database.path().join("catalog.sqlite")).expect("context");
    let game_id =
        GameId::new(format!("manual:luma-peer:{}", ulid::Ulid::generate())).expect("game id");
    let root = PathRef::new(game_root.path().to_string_lossy().replace('\\', "/"))
        .expect("game root path");
    context
        .storage()
        .upsert_game(&GameInstallation::new(
            GameIdentity::new(game_id.clone(), "Luma peer", Launcher::Manual).expect("identity"),
            Platform::Windows,
            GameRuntime::NativeWindows,
            root,
        ))
        .expect("game");

    let mut source = TrackedSource::new(
        TrackedSourceRole::AddonPayload,
        "https://example.invalid/luma.zip",
        Some("old-etag".to_owned()),
        "old-digest",
    );
    if advisory {
        source = source.with_advisory();
    }
    let peer = InstalledAddon::new(
        game_id.clone(),
        AddonKind::Luma,
        path(game_root.path(), "Luma.addon"),
    )
    .with_addon_version("Build 1")
    .with_tracked_source(source);
    context
        .storage()
        .upsert_installed_addon(&peer)
        .expect("peer");

    let root_slot_path = game_root.path().join("dxgi.dll");
    let config_path = game_root.path().join("OptiScaler.ini");
    let config_bytes = b"[OptiScaler]\nEnabled=true\n".to_vec();
    std::fs::write(&root_slot_path, b"OptiScaler outer").expect("OptiScaler outer");
    std::fs::write(&config_path, &config_bytes).expect("OptiScaler config");
    let root_slot = path(game_root.path(), "dxgi.dll");
    let config_receipt = exact_reused_receipt(&config_path);
    let topology = GameProxyTopology {
        id: format!("optiscaler:luma-peer:{}", game_id.as_str()),
        game_id: game_id.clone(),
        root_slot: root_slot.clone(),
        outer: ProxyLink {
            implementation: ProxyImplementation::OptiScaler,
            path: root_slot,
            receipt: exact_reused_receipt(&root_slot_path),
        },
        downstream: None,
        downstream_origin: None,
        root_prestate: ProxyRootPrestate::Absent,
    };
    let state = renderpilot_domain::from_persisted(
        OptiScalerInstallStateParts {
            game_id: game_id.clone(),
            release_id: "luma-peer".to_owned(),
            manifest_revision: "luma-peer".to_owned(),
            archive_sha256: None,
            source: None,
            target_exe_path: path(game_root.path(), "Game.exe"),
            target_dir: PathRef::new(game_root.path().to_string_lossy().replace('\\', "/"))
                .expect("target dir"),
            modules: vec!["core".to_owned()],
            release_files: vec![OptiScalerFileReceipt {
                path: path(game_root.path(), "OptiScaler.ini"),
                installed: config_receipt.clone(),
                role: OptiScalerFileRole::Configuration,
                cleanup: OptiScalerFileCleanup::PreserveUnchanged,
                baseline: renderpilot_domain::OptiScalerReleaseFileBaseline::Absent,
            }],
            runtime_bindings: Vec::new(),
            directory_receipts: Vec::new(),
            proxy_topology_id: Some(topology.id.clone()),
            config_schema: 1,
            config_base_release: "luma-peer".to_owned(),
            adoption_state: OptiScalerAdoptionState::AdoptedExact,
            prerequisite_binding: renderpilot_domain::OptiScalerPrerequisiteBinding::None,
            created_at: None,
            updated_at: None,
        },
        OptiScalerConfigurationBaseline::present(config_receipt, config_bytes)
            .expect("configuration baseline"),
    )
    .expect("state");
    context
        .storage()
        .commit_game_mutation(GameMutationCommit {
            game_id: &game_id,
            component_set: None,
            baseline_mutations: &[],
            addon: InstalledAddonMutation::OptiScaler(
                OptiScalerAggregateMutation::AdoptExactMetadata {
                    state: &state,
                    topology: &topology,
                },
            ),
            mutation_id: None,
        })
        .expect("OptiScaler topology");
    let peer = context
        .storage()
        .get_installed_addon(&game_id)
        .expect("load peer")
        .expect("peer exists");

    Fixture {
        _database: database,
        _game_root: game_root,
        context,
        game_id,
        peer,
        topology,
    }
}

fn refreshed_peer(before: &InstalledAddon, digest: &str, advisory: bool) -> InstalledAddon {
    let mut source = TrackedSource::new(
        TrackedSourceRole::AddonPayload,
        "https://example.invalid/luma-new.zip",
        Some("new-etag".to_owned()),
        digest,
    );
    if advisory {
        source = source.with_advisory();
    }
    before
        .clone()
        .with_addon_version("Build 2")
        .with_tracked_sources(vec![source])
}

#[test]
fn active_topology_metadata_refresh_uses_the_guard_bound_cas() {
    let fixture = fixture(false);
    let after = refreshed_peer(&fixture.peer, "new-digest", false);
    let guard = crate::game_mutation_lock::try_lock(&fixture.game_id).expect("guard");

    commit_metadata(&fixture.context, &guard, &fixture.peer, &after).expect("metadata refresh");

    let loaded = fixture
        .context
        .storage()
        .get_installed_addon(&fixture.game_id)
        .expect("peer")
        .expect("peer exists");
    assert_eq!(loaded.addon_version(), after.addon_version());
    assert_eq!(loaded.tracked_sources(), after.tracked_sources());
    assert_eq!(loaded.created_files(), after.created_files());
    assert_eq!(
        fixture
            .context
            .storage()
            .get_proxy_topology(&fixture.game_id)
            .expect("topology")
            .expect("topology exists"),
        fixture.topology
    );
}

#[test]
fn topologyless_metadata_refresh_uses_the_aggregate_cas() {
    let database = tempfile::tempdir().expect("database");
    let game_root = tempfile::tempdir().expect("game root");
    let context = Context::open_at(database.path().join("catalog.sqlite")).expect("context");
    let game_id = GameId::new(format!(
        "manual:luma-peer-no-topology:{}",
        ulid::Ulid::generate()
    ))
    .expect("game id");
    let root = PathRef::new(game_root.path().to_string_lossy().replace('\\', "/"))
        .expect("game root path");
    context
        .storage()
        .upsert_game(&GameInstallation::new(
            GameIdentity::new(
                game_id.clone(),
                "Luma peer without topology",
                Launcher::Manual,
            )
            .expect("identity"),
            Platform::Windows,
            GameRuntime::NativeWindows,
            root,
        ))
        .expect("game");
    let before = InstalledAddon::new(
        game_id.clone(),
        AddonKind::Luma,
        path(game_root.path(), "Luma.addon"),
    )
    .with_addon_version("Build 1");
    context
        .storage()
        .upsert_installed_addon(&before)
        .expect("peer");
    let before = context
        .storage()
        .get_installed_addon(&game_id)
        .expect("peer")
        .expect("peer exists");
    let after = before.clone().with_addon_version("Build 2");
    let guard = crate::game_mutation_lock::try_lock(&game_id).expect("guard");

    commit_metadata(&context, &guard, &before, &after).expect("metadata refresh");

    let loaded = context
        .storage()
        .get_installed_addon(&game_id)
        .expect("peer")
        .expect("peer exists");
    assert_eq!(loaded.addon_version(), after.addon_version());
    assert_eq!(loaded.created_files(), after.created_files());
    let connection =
        rusqlite::Connection::open(database.path().join("catalog.sqlite")).expect("connection");
    let revision: i64 = connection
        .query_row(
            "SELECT peer_aggregate_revision FROM games WHERE id = ?1",
            [game_id.as_str()],
            |row| row.get(0),
        )
        .expect("revision");
    assert_eq!(revision, 1);
}

#[test]
fn active_topology_rejects_advisory_digest_promotion_without_fallback() {
    let fixture = fixture(true);
    let after = refreshed_peer(&fixture.peer, "new-digest", true);
    let guard = crate::game_mutation_lock::try_lock(&fixture.game_id).expect("guard");

    assert!(commit_metadata(&fixture.context, &guard, &fixture.peer, &after).is_err());
    assert_eq!(
        fixture
            .context
            .storage()
            .get_installed_addon(&fixture.game_id)
            .expect("peer")
            .expect("peer exists"),
        fixture.peer
    );
    assert_eq!(
        fixture
            .context
            .storage()
            .get_proxy_topology(&fixture.game_id)
            .expect("topology")
            .expect("topology exists"),
        fixture.topology
    );
}

#[test]
fn wrong_game_guard_is_rejected_before_metadata_write() {
    let fixture = fixture(false);
    let other_game =
        GameId::new(format!("manual:other:{}", ulid::Ulid::generate())).expect("other game");
    let guard = crate::game_mutation_lock::try_lock(&other_game).expect("other guard");
    let after = refreshed_peer(&fixture.peer, "new-digest", false);

    assert!(commit_metadata(&fixture.context, &guard, &fixture.peer, &after).is_err());
    assert_eq!(
        fixture
            .context
            .storage()
            .get_installed_addon(&fixture.game_id)
            .expect("peer")
            .expect("peer exists"),
        fixture.peer
    );
}
