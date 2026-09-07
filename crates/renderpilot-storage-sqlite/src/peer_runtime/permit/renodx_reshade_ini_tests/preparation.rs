use renderpilot_domain::mutation_features::{
    RENODX_DLSS_FIX_UPDATE, RENODX_INSTALL, RENODX_UPDATE,
};
use renderpilot_domain::{ComponentId, GameId, ProxyPeerRoute};
use serde_json::json;

use super::super::{PeerCommitPreparation, PeerStorageRuntime};
use super::{
    FinishInstallWithCatalogInput, ROOT, authority, begin, catalog_claim, finish_install,
    finish_install_with_authority, finish_install_with_catalog, install_manifest,
};
use crate::repositories::ComponentBaselineMutation;
use crate::{PendingFileMutationState, SqliteStorage};

#[test]
fn ordinary_install_binds_authority_and_prepares_the_row() {
    let storage = SqliteStorage::in_memory().expect("storage");
    let game_id = GameId::new("game:renodx-ordinary").expect("game id");
    let id = "renodx-ordinary";
    begin(&storage, id, &game_id, RENODX_INSTALL);
    let runtime = PeerStorageRuntime::new(storage);
    let after = super::main_after(&game_id);
    let mut manifest_value: serde_json::Value =
        serde_json::from_str(&install_manifest(id)).expect("manifest");
    manifest_value["transaction_dir"] = json!(format!("C:/transaction/{id}"));
    let manifest = serde_json::to_string(&manifest_value).expect("manifest");

    let permit = finish_install(&runtime, id, &game_id, &after, &manifest, None)
        .expect("typed ordinary preparation");
    assert_eq!(
        permit
            .contract()
            .renodx_reshade_ini_authority()
            .expect("sealed authority"),
        &authority(RENODX_INSTALL)
    );
    assert_eq!(
        runtime
            .repositories()
            .get_pending_file_mutation(id)
            .expect("row")
            .expect("row exists")
            .state,
        PendingFileMutationState::Prepared
    );
    let persisted = runtime
        .repositories()
        .get_pending_file_mutation(id)
        .expect("row")
        .expect("row exists");
    let recovered = crate::validated_file_peer_recovery_program_for_feature(
        id,
        RENODX_INSTALL,
        &persisted.manifest_json,
    )
    .expect("storage recovery validation")
    .expect("typed recovery program");
    assert_eq!(
        recovered.endpoints()[1].role(),
        renderpilot_domain::PeerEndpointRole::RenoDxReshadeIni
    );
    assert_eq!(
        recovered.renodx_reshade_ini_authority(),
        Some(&authority(RENODX_INSTALL))
    );
}

#[test]
fn typed_catalog_projection_is_rejected_before_prepared_transition() {
    let storage = SqliteStorage::in_memory().expect("storage");
    let game_id = GameId::new("game:renodx-catalog").expect("game id");
    let id = "renodx-catalog";
    begin(&storage, id, &game_id, RENODX_INSTALL);
    let runtime = PeerStorageRuntime::new(storage);
    let after = super::main_after(&game_id);
    let manifest = install_manifest(id);
    let empty_components = [];

    assert!(
        finish_install(
            &runtime,
            id,
            &game_id,
            &after,
            &manifest,
            Some(&empty_components),
        )
        .is_err()
    );
    assert_eq!(
        runtime
            .repositories()
            .get_pending_file_mutation(id)
            .expect("row")
            .expect("row exists")
            .state,
        PendingFileMutationState::Preparing
    );
}

#[test]
fn typed_baseline_projection_is_rejected_before_prepared_transition() {
    let storage = SqliteStorage::in_memory().expect("storage");
    let game_id = GameId::new("game:renodx-baseline").expect("game id");
    let id = "renodx-baseline";
    begin(&storage, id, &game_id, RENODX_INSTALL);
    let runtime = PeerStorageRuntime::new(storage);
    let after = super::main_after(&game_id);
    let manifest = install_manifest(id);
    let component_id = ComponentId::new("component:renodx-baseline").expect("component id");
    let baseline_mutations = [ComponentBaselineMutation::Delete {
        component_id: &component_id,
    }];

    assert!(
        finish_install_with_catalog(FinishInstallWithCatalogInput {
            runtime: &runtime,
            id,
            game_id: &game_id,
            after: &after,
            manifest: &manifest,
            component_set: None,
            baseline_mutations: &baseline_mutations,
            catalog_claim: None,
            renodx_reshade_ini: Some(&authority(RENODX_INSTALL)),
        })
        .is_err()
    );
    assert_eq!(
        runtime
            .repositories()
            .get_pending_file_mutation(id)
            .expect("row")
            .expect("row exists")
            .state,
        PendingFileMutationState::Preparing
    );
}

#[test]
fn typed_catalog_claim_is_rejected_before_prepared_transition() {
    let storage = SqliteStorage::in_memory().expect("storage");
    let game_id = GameId::new("game:renodx-claim").expect("game id");
    let id = "renodx-claim";
    begin(&storage, id, &game_id, RENODX_INSTALL);
    let runtime = PeerStorageRuntime::new(storage);
    let after = super::main_after(&game_id);
    let manifest = install_manifest(id);
    let claim = catalog_claim(&game_id);

    assert!(
        finish_install_with_catalog(FinishInstallWithCatalogInput {
            runtime: &runtime,
            id,
            game_id: &game_id,
            after: &after,
            manifest: &manifest,
            component_set: Some(claim.after_components()),
            baseline_mutations: &[],
            catalog_claim: Some(&claim),
            renodx_reshade_ini: Some(&authority(RENODX_INSTALL)),
        })
        .is_err()
    );
    assert_eq!(
        runtime
            .repositories()
            .get_pending_file_mutation(id)
            .expect("row")
            .expect("row exists")
            .state,
        PendingFileMutationState::Preparing
    );
}

#[test]
fn preparation_rejects_missing_wrong_or_unexpected_authority_before_prepared() {
    let storage = SqliteStorage::in_memory().expect("storage");
    let runtime = PeerStorageRuntime::new(storage);
    let after = super::main_after(&GameId::new("game:renodx-authority").expect("game id"));

    for (id, manifest, supplied) in [
        (
            "renodx-authority-missing",
            install_manifest("renodx-authority-missing"),
            None,
        ),
        (
            "renodx-authority-wrong",
            install_manifest("renodx-authority-wrong"),
            Some(authority(RENODX_DLSS_FIX_UPDATE)),
        ),
    ] {
        let game_id = after.game_id().clone();
        begin(runtime.repositories(), id, &game_id, RENODX_INSTALL);
        assert!(
            finish_install_with_authority(
                &runtime,
                id,
                &game_id,
                &after,
                &manifest,
                None,
                supplied.as_ref(),
            )
            .is_err()
        );
        assert_eq!(
            runtime
                .repositories()
                .get_pending_file_mutation(id)
                .expect("row")
                .expect("row exists")
                .state,
            PendingFileMutationState::Preparing
        );
    }

    let id = "renodx-authority-unexpected";
    let game_id = after.game_id().clone();
    let mut value: serde_json::Value =
        serde_json::from_str(&install_manifest(id)).expect("manifest");
    value["peer_program"]["endpoints"][1]["role"] = json!("disjoint");
    let manifest = serde_json::to_string(&value).expect("manifest");
    begin(runtime.repositories(), id, &game_id, RENODX_INSTALL);
    assert!(
        finish_install_with_authority(
            &runtime,
            id,
            &game_id,
            &after,
            &manifest,
            None,
            Some(&authority(RENODX_INSTALL)),
        )
        .is_err()
    );
    assert_eq!(
        runtime
            .repositories()
            .get_pending_file_mutation(id)
            .expect("row")
            .expect("row exists")
            .state,
        PendingFileMutationState::Preparing
    );
}

#[test]
fn preparation_rejects_typed_feature_root_path_duplicate_and_missing_shapes() {
    let mut wrong_path: serde_json::Value =
        serde_json::from_str(&install_manifest("renodx-prep-path")).expect("manifest");
    wrong_path["peer_program"]["endpoints"][1]["path"] = json!("C:/game/Other.ini");
    let wrong_path = serde_json::to_string(&wrong_path).expect("manifest");

    let mut duplicate: serde_json::Value =
        serde_json::from_str(&install_manifest("renodx-prep-duplicate")).expect("manifest");
    let mut duplicate_endpoint = duplicate["peer_program"]["endpoints"][1].clone();
    duplicate_endpoint["ordinal"] = json!(2);
    duplicate_endpoint["path"] = json!("C:/game/Other.ini");
    duplicate["peer_program"]["endpoints"]
        .as_array_mut()
        .expect("endpoints")
        .push(duplicate_endpoint);
    let duplicate = serde_json::to_string(&duplicate).expect("manifest");

    let mut missing: serde_json::Value =
        serde_json::from_str(&install_manifest("renodx-prep-missing")).expect("manifest");
    missing["peer_program"]["endpoints"][1]["role"] = json!("disjoint");
    let missing = serde_json::to_string(&missing).expect("manifest");

    assert_rejected_preparation(
        "renodx-prep-feature",
        RENODX_UPDATE,
        renderpilot_domain::RENODX_INSTALL,
        ROOT,
        &install_manifest("renodx-prep-feature"),
    );
    assert_rejected_preparation(
        "renodx-prep-root",
        RENODX_INSTALL,
        RENODX_INSTALL,
        "C:/other",
        &install_manifest("renodx-prep-root"),
    );
    assert_rejected_preparation(
        "renodx-prep-path",
        RENODX_INSTALL,
        RENODX_INSTALL,
        ROOT,
        &wrong_path,
    );
    assert_rejected_preparation(
        "renodx-prep-duplicate",
        RENODX_INSTALL,
        RENODX_INSTALL,
        ROOT,
        &duplicate,
    );
    assert_rejected_preparation(
        "renodx-prep-missing",
        RENODX_INSTALL,
        RENODX_INSTALL,
        ROOT,
        &missing,
    );
}

fn assert_rejected_preparation(
    id: &str,
    row_feature: &str,
    preparation_feature: &str,
    canonical_game_root: &str,
    manifest: &str,
) {
    let storage = SqliteStorage::in_memory().expect("storage");
    let game_id = GameId::new(format!("game:{id}")).expect("game id");
    begin(&storage, id, &game_id, row_feature);
    let runtime = PeerStorageRuntime::new(storage);
    let after = super::main_after(&game_id);
    let supplied_authority = authority(RENODX_INSTALL);

    assert!(
        runtime
            .finish_file_peer_preparation(PeerCommitPreparation {
                mutation_id: id,
                game_id: &game_id,
                feature: preparation_feature,
                subject_id: None,
                manifest_json: manifest,
                canonical_game_root,
                initial_read_guards: &[],
                before_peer: None,
                after_peer: Some(&after),
                before_topology: None,
                planned_after_topology: None,
                route: ProxyPeerRoute::DurableDisjoint,
                component_set: None,
                baseline_mutations: &[],
                catalog_claim: None,
                renodx_reshade_ini: Some(&supplied_authority),
            })
            .is_err()
    );
    assert_eq!(
        runtime
            .repositories()
            .get_pending_file_mutation(id)
            .expect("row")
            .expect("row exists")
            .state,
        PendingFileMutationState::Preparing
    );
}
