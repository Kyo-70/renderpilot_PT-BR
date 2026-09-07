use renderpilot_application::{GameRepository, InstalledAddonRepository};
use renderpilot_domain::{
    GameId, GameIdentity, GameInstallation, GameRuntime, InstalledAddon, Launcher,
    PeerEndpointEvidence, PeerFileImage, Platform,
};
use serde_json::{Value, json};

use super::super::{PeerStorageRuntime, SharedPeerCommitPreparation};
use super::{ROOT, authority, hash, main_after, path};
use crate::SqliteStorage;
use crate::repositories::{
    BeginSharedVulkanMutation, PendingSharedVulkanMutationState, SharedArtifactMutation,
    SharedVulkanMutationScope,
};

pub(super) const SHARED_ROOT: &str = "C:/shared";

pub(super) fn roots_json(entries: &[(&str, &str, &str)]) -> String {
    json!({
        "version": 1,
        "roots": entries.iter().map(|(id, kind, canonical_path)| json!({
            "id": id,
            "kind": kind,
            "canonical_path": canonical_path,
        })).collect::<Vec<_>>(),
    })
    .to_string()
}

pub(super) fn game_shared_roots() -> String {
    roots_json(&[
        ("game-0", "game", ROOT),
        ("shared", "shared_vulkan", SHARED_ROOT),
    ])
}

pub(super) fn shared_manifest(id: &str, roots: &[&str], typed_path: Option<&str>) -> String {
    let mut endpoints = vec![json!({
        "ordinal": 0,
            "path": "C:/game/RenoDx.addon64",
        "role": "disjoint",
        "operation": "create",
        "planned_sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        "planned_length": 8,
        "before": null,
        "read_guards": [format!("{}:shared.addon64", roots[0])],
        "subtree_publishes": [],
    })];
    if let Some(path) = typed_path {
        endpoints.push(json!({
            "ordinal": 1,
            "path": path,
            "role": "renodx_reshade_ini",
            "operation": "create",
            "planned_sha256": "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
            "planned_length": 4,
            "before": null,
            "read_guards": [format!("{}:reshade.ini", roots[0])],
            "subtree_publishes": [],
        }));
    }
    serde_json::to_string(&json!({
        "peer_program": {
            "format": 1,
            "transaction_owner": id,
            "execution_class": "shared",
            "roots": roots,
            "stage": [],
            "custody": [],
            "created_ancestors": [],
            "endpoints": endpoints,
        }
    }))
    .expect("shared manifest")
}

pub(super) fn seed_game(storage: &SqliteStorage, game_id: &GameId) {
    storage
        .upsert_game(&GameInstallation::new(
            GameIdentity::new(game_id.clone(), "RenoDX shared", Launcher::Steam).expect("identity"),
            Platform::Windows,
            GameRuntime::NativeWindows,
            path(ROOT),
        ))
        .expect("game");
}

pub(super) fn begin_shared(storage: &SqliteStorage, id: &str, game_id: &GameId, roots: String) {
    storage
        .try_begin_shared_vulkan_mutation(&BeginSharedVulkanMutation {
            id: id.to_owned(),
            scope: SharedVulkanMutationScope::GameShared,
            game_id: Some(game_id.clone()),
            feature: renderpilot_domain::RENODX_INSTALL.to_owned(),
            initial_manifest_json: "{}".to_owned(),
            root_capabilities_json: roots,
        })
        .expect("shared row");
}

pub(super) fn typed_preparation(
    runtime: &PeerStorageRuntime,
    id: &str,
    game_id: &GameId,
    manifest: &str,
    after: &InstalledAddon,
) -> renderpilot_application::AppResult<super::super::PreparedPeerCommitPermit> {
    let authority = authority(renderpilot_domain::RENODX_INSTALL);
    runtime.finish_shared_peer_preparation(SharedPeerCommitPreparation {
        mutation_id: id,
        feature: renderpilot_domain::RENODX_INSTALL,
        game_id,
        manifest_json: manifest,
        before_peer: None,
        after_peer: Some(after),
        before_topology: None,
        planned_after_topology: None,
        route: renderpilot_domain::ProxyPeerRoute::DurableDisjoint,
        renodx_reshade_ini: Some(&authority),
    })
}

fn typed_evidence(manifest: &str) -> Vec<PeerEndpointEvidence> {
    let value: Value = serde_json::from_str(manifest).expect("manifest");
    let program =
        super::super::super::manifest::parse_peer_program(&value, "test").expect("program");
    vec![
        PeerEndpointEvidence::new(
            program.intents()[0].clone(),
            None,
            Some(
                PeerFileImage::new(
                    "addon",
                    hash("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"),
                    8,
                )
                .expect("image"),
            ),
        ),
        PeerEndpointEvidence::new(
            program.intents()[1].clone(),
            None,
            Some(
                PeerFileImage::new(
                    "ini",
                    hash("cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"),
                    4,
                )
                .expect("image"),
            ),
        ),
    ]
}

#[test]
fn shared_typed_game_zero_preparation_seals_authority() {
    let storage = SqliteStorage::in_memory().expect("storage");
    let game_id = GameId::new("game:renodx-shared-typed").expect("game id");
    seed_game(&storage, &game_id);
    let id = "renodx-shared-typed";
    let manifest = shared_manifest(id, &[ROOT, SHARED_ROOT], Some("C:/game/ReShade.ini"));
    begin_shared(&storage, id, &game_id, game_shared_roots());
    let runtime = PeerStorageRuntime::new(storage);
    let permit = typed_preparation(&runtime, id, &game_id, &manifest, &main_after(&game_id))
        .expect("typed shared preparation");
    assert_eq!(
        permit
            .contract()
            .renodx_reshade_ini_authority()
            .expect("authority"),
        &authority(renderpilot_domain::RENODX_INSTALL)
    );
}

#[test]
fn shared_typed_commit_rejects_root_capability_drift_before_projection() {
    let storage = SqliteStorage::in_memory().expect("storage");
    let game_id = GameId::new("game:renodx-shared-tamper").expect("game id");
    seed_game(&storage, &game_id);
    let id = "renodx-shared-tamper";
    let manifest = shared_manifest(id, &[ROOT, SHARED_ROOT], Some("C:/game/ReShade.ini"));
    begin_shared(&storage, id, &game_id, game_shared_roots());
    let runtime = PeerStorageRuntime::new(storage);
    let permit = typed_preparation(&runtime, id, &game_id, &manifest, &main_after(&game_id))
        .expect("typed shared preparation");
    let before_fingerprint = runtime
        .repositories()
        .with_transaction(|transaction| {
            super::super::super::permit::fingerprint::read_shared_fingerprint(transaction, id)
        })
        .expect("read original fingerprint")
        .expect("original fingerprint");
    runtime
        .repositories()
        .with_transaction(|transaction| {
            transaction
                .execute(
                    "UPDATE pending_shared_vulkan_mutations
                     SET root_capabilities_json = ?1
                     WHERE resource_key = ?2 AND id = ?3",
                    rusqlite::params![
                        r#"{ "version": 1, "roots": [
                        { "id": "game-0", "kind": "game", "canonical_path": "C:/game" },
                        { "id": "shared", "kind": "shared_vulkan", "canonical_path": "C:/shared" }
                    ] }"#,
                        crate::repositories::pending_shared_vulkan_mutations::RESOURCE_KEY,
                        id,
                    ],
                )
                .map_err(crate::error::storage_error)?;
            Ok(())
        })
        .expect("tamper root capabilities");
    let after_fingerprint = runtime
        .repositories()
        .with_transaction(|transaction| {
            super::super::super::permit::fingerprint::read_shared_fingerprint(transaction, id)
        })
        .expect("read tampered fingerprint")
        .expect("tampered fingerprint");
    assert_ne!(
        before_fingerprint.root_capabilities_sha256,
        after_fingerprint.root_capabilities_sha256
    );

    assert!(
        runtime
            .seal_and_commit_shared_peer(
                permit,
                typed_evidence(&manifest),
                crate::repositories::InstalledAddonMutation::Upsert(&main_after(&game_id)),
                SharedArtifactMutation::Keep,
            )
            .is_err()
    );
    assert_eq!(
        runtime
            .repositories()
            .get_pending_shared_vulkan_mutation(id)
            .expect("row")
            .expect("row exists")
            .state,
        PendingSharedVulkanMutationState::Prepared
    );
    assert!(
        runtime
            .repositories()
            .get_installed_addon(&game_id)
            .expect("peer")
            .is_none()
    );
}

#[test]
fn unchanged_shared_typed_commit_succeeds_with_retained_root_authority() {
    let storage = SqliteStorage::in_memory().expect("storage");
    let game_id = GameId::new("game:renodx-shared-commit").expect("game id");
    seed_game(&storage, &game_id);
    let id = "renodx-shared-commit";
    let manifest = shared_manifest(id, &[ROOT, SHARED_ROOT], Some("C:/game/ReShade.ini"));
    begin_shared(&storage, id, &game_id, game_shared_roots());
    let runtime = PeerStorageRuntime::new(storage);
    let after = main_after(&game_id);
    let permit = typed_preparation(&runtime, id, &game_id, &manifest, &after)
        .expect("typed shared preparation");
    runtime
        .seal_and_commit_shared_peer(
            permit,
            typed_evidence(&manifest),
            crate::repositories::InstalledAddonMutation::Upsert(&after),
            SharedArtifactMutation::Keep,
        )
        .expect("typed shared commit");
    assert_eq!(
        runtime
            .repositories()
            .get_pending_shared_vulkan_mutation(id)
            .expect("row")
            .expect("row exists")
            .state,
        PendingSharedVulkanMutationState::Committed
    );
}
