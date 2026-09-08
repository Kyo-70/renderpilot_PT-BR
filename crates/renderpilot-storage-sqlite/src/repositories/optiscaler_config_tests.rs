use renderpilot_application::{AppError, AppResult, GameRepository, OptiScalerStateRepository};
use renderpilot_domain::{
    ExactOptiConfigProjection, FileReceipt, GameId, GameIdentity, GameInstallation,
    GameProxyTopology, GameRuntime, Launcher, OptiConfigOperation, OptiScalerAdoptionState,
    OptiScalerConfigAuthority, OptiScalerConfigurationBaseline, OptiScalerFileCleanup,
    OptiScalerFileReceipt, OptiScalerFileRole, OptiScalerInstallState, OptiScalerInstallStateParts,
    PathRef, Platform, ProxyImplementation, ProxyLink, ProxyRootPrestate, Sha256Hash,
    from_persisted,
};

use super::{
    SqliteStorage, commit_optiscaler_config_companion_within_transaction, optiscaler_states,
    proxy_topologies,
};

fn path(value: &str) -> PathRef {
    PathRef::new(value).expect("path")
}

fn receipt(identity: &str, digest: char) -> FileReceipt {
    FileReceipt::owned(
        identity,
        Sha256Hash::new(digest.to_string().repeat(64)).expect("digest"),
    )
    .expect("receipt")
}

fn fixture() -> (SqliteStorage, GameId, OptiScalerInstallState) {
    let storage = SqliteStorage::in_memory().expect("storage");
    let game_id = GameId::new("game:opti-config-companion").expect("game id");
    let root = path("C:/Games/OptiConfigCompanion");
    let game = GameInstallation::new(
        GameIdentity::new(game_id.clone(), "Opti config", Launcher::Steam).expect("identity"),
        Platform::Windows,
        GameRuntime::NativeWindows,
        root.clone(),
    );
    storage.upsert_game(&game).expect("store game");

    let topology_id = "optiscaler:opti-config-companion".to_owned();
    let root_slot = path("C:/Games/OptiConfigCompanion/dxgi.dll");
    let topology = GameProxyTopology {
        id: topology_id.clone(),
        game_id: game_id.clone(),
        root_slot: root_slot.clone(),
        outer: ProxyLink {
            implementation: ProxyImplementation::OptiScaler,
            path: root_slot,
            receipt: receipt("outer", 'b'),
        },
        downstream: None,
        downstream_origin: None,
        root_prestate: ProxyRootPrestate::Absent,
    };
    let state = from_persisted(
        OptiScalerInstallStateParts {
            game_id: game_id.clone(),
            release_id: "release".to_owned(),
            manifest_revision: "revision".to_owned(),
            archive_sha256: None,
            source: None,
            target_exe_path: path("C:/Games/OptiConfigCompanion/Game.exe"),
            target_dir: root,
            modules: vec!["core".to_owned()],
            release_files: vec![OptiScalerFileReceipt {
                path: path("C:/Games/OptiConfigCompanion/OptiScaler.ini"),
                installed: receipt("config", 'a'),
                role: OptiScalerFileRole::Configuration,
                cleanup: OptiScalerFileCleanup::RemoveIfUnchanged,
                baseline: renderpilot_domain::OptiScalerReleaseFileBaseline::Absent,
            }],
            runtime_bindings: Vec::new(),
            directory_receipts: Vec::new(),
            proxy_topology_id: Some(topology_id),
            config_schema: 1,
            config_base_release: "release".to_owned(),
            adoption_state: OptiScalerAdoptionState::Managed,
            prerequisite_binding: renderpilot_domain::OptiScalerPrerequisiteBinding::None,
            created_at: None,
            updated_at: None,
        },
        OptiScalerConfigurationBaseline::absent(),
    )
    .expect("state");
    storage
        .with_transaction(|transaction| {
            proxy_topologies::upsert_within_transaction(transaction, &topology)?;
            optiscaler_states::upsert_within_transaction(transaction, &state)
        })
        .expect("store aggregate");
    let persisted = storage
        .get_optiscaler_install_state(&game_id)
        .expect("read state")
        .expect("state exists");
    (storage, game_id, persisted)
}

fn successor(before: &OptiScalerInstallState, digest: char) -> OptiScalerInstallState {
    let mut config = before
        .configuration_receipt()
        .expect("configuration receipt")
        .clone();
    config.installed = receipt("config", digest);
    before
        .with_configuration_receipt(&config)
        .expect("exact configuration successor")
}

fn config_manifest(
    id: &str,
    before: &OptiScalerInstallState,
    after: &OptiScalerInstallState,
) -> String {
    let before_receipt = before.configuration_receipt().expect("before config");
    let after_receipt = after.configuration_receipt().expect("after config");
    serde_json::json!({
        "format_version": 1,
        "roots": [before.target_dir.as_str()],
        "snapshots": [{
            "path": before_receipt.path.as_str(),
            "snapshot": "C:/transaction/0.before",
        }],
        "peer_program": {
            "format": 1,
            "transaction_owner": id,
            "execution_class": "ordinary",
            "roots": [before.target_dir.as_str()],
            "stage": [],
            "custody": [format!("{}:optiscaler.ini", before.target_dir.as_str())],
            "created_ancestors": [],
            "endpoints": [{
                "ordinal": 0,
                "path": before_receipt.path.as_str(),
                "role": "optiscaler_config",
                "operation": "replace",
                "planned_sha256": after_receipt.installed.digest().as_str(),
                "planned_length": 4,
                "before": {
                    "identity": before_receipt.installed.identity(),
                    "sha256": before_receipt.installed.digest().as_str(),
                    "length": 4,
                },
                "read_guards": [format!("{}:optiscaler.ini", before.target_dir.as_str())],
                "subtree_publishes": [],
            }],
        },
    })
    .to_string()
}

#[test]
fn optiscaler_config_companion_cas_is_exact_and_participates_in_outer_rollback() {
    let (storage, game_id, before) = fixture();
    let enabled = successor(&before, 'c');

    let rollback: AppResult<()> = storage.with_transaction(|transaction| {
        commit_optiscaler_config_companion_within_transaction(
            transaction,
            &game_id,
            &before,
            &enabled,
        )?;
        Err(AppError::storage_failed("force peer transaction rollback"))
    });
    assert!(rollback.is_err());
    let after_rollback = storage
        .get_optiscaler_install_state(&game_id)
        .expect("read after rollback")
        .expect("state exists");
    assert_eq!(after_rollback, before);

    storage
        .with_transaction(|transaction| {
            commit_optiscaler_config_companion_within_transaction(
                transaction,
                &game_id,
                &before,
                &enabled,
            )
        })
        .expect("commit companion");
    let committed = storage
        .get_optiscaler_install_state(&game_id)
        .expect("read committed")
        .expect("state exists");
    assert!(committed.eq_ignoring_persistence_timestamps(&enabled));

    let stale_successor = successor(&before, 'd');
    assert!(
        storage
            .with_transaction(|transaction| {
                commit_optiscaler_config_companion_within_transaction(
                    transaction,
                    &game_id,
                    &before,
                    &stale_successor,
                )
            })
            .is_err()
    );
    let after_conflict = storage
        .get_optiscaler_install_state(&game_id)
        .expect("read after conflict")
        .expect("state exists");
    assert_eq!(after_conflict, committed);
}

#[test]
fn optiscaler_config_manifest_requires_its_exclusive_renodx_preflight() {
    let (_storage, _game_id, before) = fixture();
    let after = successor(&before, 'c');
    let authority = OptiScalerConfigAuthority::new(before.target_dir.clone()).expect("authority");
    let projection = ExactOptiConfigProjection::new(
        authority,
        after.configuration_receipt().expect("config").clone(),
        OptiConfigOperation::EnableLoadReshade,
    )
    .expect("projection");
    let manifest = config_manifest("opti-config-preflight", &before, &after);

    assert!(crate::validate_file_peer_program_manifest(&manifest).is_err());
    crate::validate_file_peer_program_manifest_with_renodx_optiscaler_config(
        renderpilot_domain::mutation_features::RENODX_INSTALL,
        before.target_dir.as_str(),
        None,
        &before,
        &projection,
        &manifest,
    )
    .expect("exclusive config preflight");
    assert!(
        crate::validate_file_peer_program_manifest_with_renodx_optiscaler_config(
            renderpilot_domain::mutation_features::RENODX_UPDATE,
            before.target_dir.as_str(),
            None,
            &before,
            &projection,
            &manifest,
        )
        .is_err()
    );
}
