use renderpilot_domain::{AddonKind, GameId, InstalledAddon};

use super::super::{PeerStorageRuntime, PreparedPeerCommitPermit, SharedPeerCommitPreparation};
use super::shared::{
    SHARED_ROOT, begin_shared, game_shared_roots, roots_json, seed_game, shared_manifest,
    typed_preparation,
};
use super::{authority, main_after};
use crate::SqliteStorage;
use crate::repositories::PendingSharedVulkanMutationState;

#[test]
fn shared_typed_authority_requires_game_zero_and_exact_endpoint_root() {
    let cases = [
        (
            "missing-game-zero",
            roots_json(&[("shared", "shared_vulkan", SHARED_ROOT)]),
            vec![SHARED_ROOT],
            Some("C:/shared/ReShade.ini"),
        ),
        (
            "game-one-endpoint",
            roots_json(&[
                ("game-0", "game", super::ROOT),
                ("game-1", "game", "C:/other"),
                ("shared", "shared_vulkan", SHARED_ROOT),
            ]),
            vec![super::ROOT, "C:/other", SHARED_ROOT],
            Some("C:/other/ReShade.ini"),
        ),
        (
            "shared-endpoint",
            game_shared_roots(),
            vec![super::ROOT, SHARED_ROOT],
            Some("C:/shared/ReShade.ini"),
        ),
    ];
    for (suffix, roots, program_roots, typed_path) in cases {
        let storage = SqliteStorage::in_memory().expect("storage");
        let game_id = GameId::new(format!("game:renodx-shared-{suffix}")).expect("game id");
        seed_game(&storage, &game_id);
        let id = format!("renodx-shared-{suffix}");
        let manifest = shared_manifest(&id, &program_roots, typed_path);
        begin_shared(&storage, &id, &game_id, roots);
        let runtime = PeerStorageRuntime::new(storage);
        assert!(
            typed_preparation(&runtime, &id, &game_id, &manifest, &main_after(&game_id)).is_err(),
            "case {suffix} must reject"
        );
        assert_eq!(
            runtime
                .repositories()
                .get_pending_shared_vulkan_mutation(&id)
                .expect("row")
                .expect("row exists")
                .state,
            PendingSharedVulkanMutationState::Preparing
        );
    }
}

#[test]
fn shared_supplied_authority_without_typed_role_is_rejected_before_prepared() {
    let storage = SqliteStorage::in_memory().expect("storage");
    let game_id = GameId::new("game:renodx-shared-untyped-authority").expect("game id");
    seed_game(&storage, &game_id);
    let id = "renodx-shared-untyped-authority";
    let manifest = shared_manifest(id, &[super::ROOT, SHARED_ROOT], None);
    begin_shared(&storage, id, &game_id, game_shared_roots());
    let runtime = PeerStorageRuntime::new(storage);
    let supplied = authority(renderpilot_domain::RENODX_INSTALL);
    let result: renderpilot_application::AppResult<PreparedPeerCommitPermit> = runtime
        .finish_shared_peer_preparation(SharedPeerCommitPreparation {
            mutation_id: id,
            feature: renderpilot_domain::RENODX_INSTALL,
            game_id: &game_id,
            manifest_json: &manifest,
            before_peer: None,
            after_peer: Some(&main_after(&game_id)),
            before_topology: None,
            planned_after_topology: None,
            route: renderpilot_domain::ProxyPeerRoute::DurableDisjoint,
            renodx_reshade_ini: Some(&supplied),
        });
    assert!(result.is_err());
    assert_eq!(
        runtime
            .repositories()
            .get_pending_shared_vulkan_mutation(id)
            .expect("row")
            .expect("row exists")
            .state,
        PendingSharedVulkanMutationState::Preparing
    );
}

#[test]
fn shared_only_without_authority_preserves_untyped_preparation() {
    let storage = SqliteStorage::in_memory().expect("storage");
    let game_id = GameId::new("game:renodx-shared-only").expect("game id");
    seed_game(&storage, &game_id);
    let id = "renodx-shared-only";
    let manifest = shared_manifest(id, &[SHARED_ROOT], None);
    begin_shared(
        &storage,
        id,
        &game_id,
        roots_json(&[("shared", "shared_vulkan", SHARED_ROOT)]),
    );
    let runtime = PeerStorageRuntime::new(storage);
    let after = InstalledAddon::new(
        game_id.clone(),
        AddonKind::RenoDx,
        super::path("C:/game/RenoDx.addon64"),
    );
    runtime
        .finish_shared_peer_preparation(SharedPeerCommitPreparation {
            mutation_id: id,
            feature: renderpilot_domain::RENODX_INSTALL,
            game_id: &game_id,
            manifest_json: &manifest,
            before_peer: None,
            after_peer: Some(&after),
            before_topology: None,
            planned_after_topology: None,
            route: renderpilot_domain::ProxyPeerRoute::DurableDisjoint,
            renodx_reshade_ini: None,
        })
        .expect("untyped shared preparation");
}
