//! Typed OptiScaler lifecycle-state persistence.

pub(crate) mod codec;

use renderpilot_application::{AppResult, OptiScalerStateRepository};
use renderpilot_domain::{
    GameId, OptiScalerInstallState, OptiScalerInstallStateParts, OptiScalerPrerequisiteBinding,
    PathRef, Sha256Hash, from_persisted,
};
use rusqlite::{OptionalExtension, Row};

use crate::error::{invalid_row, storage_error};
use crate::repositories::observation::RowObservation;
use crate::{mapping, sqlite_clock};

use super::SqliteStorage;

const STATE_SELECT_SQL: &str = "SELECT game_id, release_id, manifest_revision, archive_sha256, source, target_exe_path, target_dir, \
     modules_json, release_files_json, runtime_bindings_json, directory_receipts_json, \
     proxy_topology_id, config_schema, config_base_release, \
     adoption_state, prerequisite_binding, created_at, updated_at, \
     configuration_baseline_json \
     FROM optiscaler_install_states WHERE game_id=?1";

impl OptiScalerStateRepository for SqliteStorage {
    fn get_optiscaler_install_state(
        &self,
        game_id: &GameId,
    ) -> AppResult<Option<OptiScalerInstallState>> {
        self.with_connection(|connection| {
            observe_on_connection(connection, game_id)?.into_optional()
        })
    }

    fn list_optiscaler_install_states(&self) -> AppResult<Vec<OptiScalerInstallState>> {
        self.with_connection(|connection| {
            let mut statement = connection
                .prepare(
                    "SELECT game_id, release_id, manifest_revision, archive_sha256, source, target_exe_path, target_dir, \
                 modules_json, release_files_json, runtime_bindings_json, directory_receipts_json, \
                 proxy_topology_id, config_schema, config_base_release, \
                 adoption_state, prerequisite_binding, created_at, updated_at, \
                 configuration_baseline_json \
                 FROM optiscaler_install_states ORDER BY game_id ASC",
                )
                .map_err(storage_error)?;
            let rows = statement
                .query_map([], raw_state_from_row)
                .map_err(storage_error)?;
            rows.map(|row| decode_state(row.map_err(storage_error)?))
                .collect()
        })
    }
}

pub(super) fn get_within_transaction(
    transaction: &rusqlite::Transaction<'_>,
    game_id: &GameId,
) -> AppResult<Option<OptiScalerInstallState>> {
    observe_within_aggregate_transaction(transaction, game_id)?.into_optional()
}

/// Reads the typed OptiScaler participant through a caller-owned transaction.
///
/// The repository keeps its row decoder private; this crate-visible seam is
/// the only aggregate runtime access point and therefore cannot open a second
/// transaction or bypass the repository's validation.
pub(crate) fn get_within_aggregate_transaction(
    transaction: &rusqlite::Transaction<'_>,
    game_id: &GameId,
) -> AppResult<Option<OptiScalerInstallState>> {
    get_within_transaction(transaction, game_id)
}

pub(crate) fn observe_within_aggregate_transaction(
    transaction: &rusqlite::Transaction<'_>,
    game_id: &GameId,
) -> AppResult<RowObservation<OptiScalerInstallState>> {
    observe_raw(
        transaction
            .query_row(STATE_SELECT_SQL, [game_id.as_str()], raw_state_from_row)
            .optional(),
    )
}

pub(super) fn upsert_within_transaction(
    transaction: &rusqlite::Transaction<'_>,
    state: &OptiScalerInstallState,
) -> AppResult<()> {
    state.validate().map_err(invalid_row)?;
    let configuration_baseline_json = codec::encode(state.configuration_baseline())?;
    let now_ms = sqlite_clock::now_ms(transaction)?;
    // Persistence timestamps are storage-owned. The single `now_ms` value is
    // intentionally used for both columns on insert; an upsert keeps the
    // existing `created_at` and refreshes only `updated_at` below.
    transaction.execute(
        "INSERT INTO optiscaler_install_states \
         (game_id, release_id, manifest_revision, archive_sha256, source, target_exe_path, target_dir, modules_json, \
          release_files_json, runtime_bindings_json, directory_receipts_json, proxy_topology_id, \
          config_schema, config_base_release, adoption_state, prerequisite_binding, \
           created_at, updated_at, configuration_baseline_json) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?17,?18) \
         ON CONFLICT(game_id) DO UPDATE SET release_id=excluded.release_id, \
          manifest_revision=excluded.manifest_revision, archive_sha256=excluded.archive_sha256, \
          source=excluded.source, target_exe_path=excluded.target_exe_path, \
          target_dir=excluded.target_dir, modules_json=excluded.modules_json, \
          release_files_json=excluded.release_files_json, proxy_topology_id=excluded.proxy_topology_id, \
          runtime_bindings_json=excluded.runtime_bindings_json, \
          directory_receipts_json=excluded.directory_receipts_json, config_schema=excluded.config_schema, \
          config_base_release=excluded.config_base_release, adoption_state=excluded.adoption_state, \
          prerequisite_binding=excluded.prerequisite_binding, \
           updated_at=excluded.updated_at, \
           configuration_baseline_json=optiscaler_install_states.configuration_baseline_json",
        rusqlite::params![
            state.game_id.as_str(), state.release_id, state.manifest_revision,
            state.archive_sha256.as_ref().map(Sha256Hash::as_str), state.source.as_deref(),
            state.target_exe_path.as_str(), state.target_dir.as_str(), mapping::serialize_json(&state.modules)?,
            mapping::serialize_json(&state.release_files)?,
            mapping::serialize_json(&state.runtime_bindings)?,
            mapping::serialize_json(&state.directory_receipts)?, state.proxy_topology_id,
            state.config_schema, state.config_base_release, state.adoption_state.as_str(),
            state.prerequisite_binding.as_str(), now_ms,
            configuration_baseline_json,
        ],
    ).map_err(storage_error)?;
    Ok(())
}

fn raw_state_from_row(row: &Row<'_>) -> rusqlite::Result<RawOptiScalerInstallState> {
    Ok(RawOptiScalerInstallState {
        game_id: row.get(0)?,
        release_id: row.get(1)?,
        manifest_revision: row.get(2)?,
        archive_sha256: row.get(3)?,
        source: row.get(4)?,
        target_exe_path: row.get(5)?,
        target_dir: row.get(6)?,
        modules_json: row.get(7)?,
        release_files_json: row.get(8)?,
        runtime_bindings_json: row.get(9)?,
        directory_receipts_json: row.get(10)?,
        proxy_topology_id: row.get(11)?,
        config_schema: row.get(12)?,
        config_base_release: row.get(13)?,
        adoption_state: row.get(14)?,
        prerequisite_binding: row.get(15)?,
        created_at: row.get(16)?,
        updated_at: row.get(17)?,
        configuration_baseline_json: row.get(18)?,
    })
}

fn observe_on_connection(
    connection: &rusqlite::Connection,
    game_id: &GameId,
) -> AppResult<RowObservation<OptiScalerInstallState>> {
    observe_raw(
        connection
            .query_row(STATE_SELECT_SQL, [game_id.as_str()], raw_state_from_row)
            .optional(),
    )
}

fn observe_raw(
    result: rusqlite::Result<Option<RawOptiScalerInstallState>>,
) -> AppResult<RowObservation<OptiScalerInstallState>> {
    match result.map_err(storage_error)? {
        None => Ok(RowObservation::Missing),
        Some(raw) => Ok(match decode_state(raw) {
            Ok(state) => RowObservation::Present(state),
            Err(error) => RowObservation::Invalid(error),
        }),
    }
}

#[derive(Debug)]
struct RawOptiScalerInstallState {
    game_id: String,
    release_id: String,
    manifest_revision: String,
    archive_sha256: Option<String>,
    source: Option<String>,
    target_exe_path: String,
    target_dir: String,
    modules_json: String,
    release_files_json: String,
    runtime_bindings_json: String,
    directory_receipts_json: String,
    proxy_topology_id: Option<String>,
    config_schema: u32,
    config_base_release: String,
    adoption_state: String,
    prerequisite_binding: String,
    created_at: Option<i64>,
    updated_at: Option<i64>,
    configuration_baseline_json: String,
}

fn decode_state(raw: RawOptiScalerInstallState) -> AppResult<OptiScalerInstallState> {
    let adoption = raw.adoption_state;
    let parts = OptiScalerInstallStateParts {
        game_id: GameId::new(raw.game_id).map_err(invalid_row)?,
        release_id: raw.release_id,
        manifest_revision: raw.manifest_revision,
        archive_sha256: raw
            .archive_sha256
            .map(|value| Sha256Hash::new(value).map_err(invalid_row))
            .transpose()?,
        source: raw.source,
        target_exe_path: PathRef::new(raw.target_exe_path).map_err(invalid_row)?,
        target_dir: PathRef::new(raw.target_dir).map_err(invalid_row)?,
        modules: mapping::deserialize_json(&raw.modules_json)?,
        release_files: mapping::deserialize_json(&raw.release_files_json)?,
        runtime_bindings: mapping::deserialize_json(&raw.runtime_bindings_json)?,
        directory_receipts: mapping::deserialize_json(&raw.directory_receipts_json)?,
        proxy_topology_id: raw.proxy_topology_id,
        config_schema: raw.config_schema,
        config_base_release: raw.config_base_release,
        adoption_state: adoption
            .parse()
            .map_err(|()| invalid_row(format!("unknown adoption state {adoption}")))?,
        prerequisite_binding: raw
            .prerequisite_binding
            .parse::<OptiScalerPrerequisiteBinding>()
            .map_err(|()| invalid_row("unknown OptiScaler prerequisite binding"))?,
        created_at: raw.created_at,
        updated_at: raw.updated_at,
    };
    from_persisted(parts, codec::decode(&raw.configuration_baseline_json)?).map_err(invalid_row)
}

#[cfg(test)]
mod tests {
    use renderpilot_application::{
        GameRepository, OptiScalerStateRepository, ProxyTopologyRepository,
    };
    use renderpilot_domain::{
        FileReceipt, GameIdentity, GameInstallation, GameProxyTopology, GameRuntime, Launcher,
        OptiScalerAdoptionState, OptiScalerConfigurationBaseline, OptiScalerFileBaseline,
        OptiScalerFileCleanup, OptiScalerFileReceipt, OptiScalerFileRole,
        OptiScalerInstallStateParts, OptiScalerModuleRuntimeBinding, OptiScalerPrerequisiteBinding,
        Platform, ProxyImplementation, ProxyLink, Sha256Hash, from_new_install,
    };

    use super::*;
    use crate::repositories::proxy_topologies;

    fn state(game_id: &GameId, topology_id: &str) -> OptiScalerInstallState {
        from_persisted(
            OptiScalerInstallStateParts {
                game_id: game_id.clone(),
                release_id: "v0.9.4".to_owned(),
                manifest_revision: "test".to_owned(),
                archive_sha256: None,
                source: None,
                target_exe_path: PathRef::new("C:/Games/Test/Game.exe").expect("exe"),
                target_dir: PathRef::new("C:/Games/Test").expect("target"),
                modules: vec!["core".to_owned()],
                release_files: vec![OptiScalerFileReceipt {
                    path: PathRef::new("C:/Games/Test/OptiScaler.ini").expect("config"),
                    installed: FileReceipt::owned(
                        "config-id",
                        Sha256Hash::new("b".repeat(64)).expect("hash"),
                    )
                    .expect("receipt"),
                    role: OptiScalerFileRole::Configuration,
                    cleanup: OptiScalerFileCleanup::RemoveIfUnchanged,
                    baseline: renderpilot_domain::OptiScalerReleaseFileBaseline::Absent,
                }],
                runtime_bindings: vec![OptiScalerModuleRuntimeBinding {
                    module: "core".to_owned(),
                    path: PathRef::new("C:/Games/Test/core.dll").expect("runtime"),
                    installed: FileReceipt::owned(
                        "runtime-id",
                        Sha256Hash::new("c".repeat(64)).expect("runtime hash"),
                    )
                    .expect("receipt"),
                    baseline: OptiScalerFileBaseline::Absent,
                }],
                directory_receipts: Vec::new(),
                proxy_topology_id: Some(topology_id.to_owned()),
                config_schema: 1,
                config_base_release: "v0.9.4".to_owned(),
                adoption_state: OptiScalerAdoptionState::Managed,
                prerequisite_binding: OptiScalerPrerequisiteBinding::None,
                created_at: None,
                updated_at: None,
            },
            OptiScalerConfigurationBaseline::absent(),
        )
        .expect("state")
    }

    fn replacement_state(game_id: &GameId, topology_id: &str) -> OptiScalerInstallState {
        let previous = state(game_id, topology_id);
        let old_bytes = b"[OptiScaler]\nUser=true\n";
        let new_bytes = b"[OptiScaler]\nUser=false\n";
        let mut parts = OptiScalerInstallStateParts::from(&previous);
        parts.release_files[0].installed = FileReceipt::owned(
            "original-config",
            renderpilot_detection::sha256_bytes(new_bytes).expect("new digest"),
        )
        .expect("replacement receipt");
        parts.release_files[0].cleanup = OptiScalerFileCleanup::PreserveCurrentThenRestoreBaseline;
        from_new_install(
            parts,
            OptiScalerConfigurationBaseline::present(
                FileReceipt::reused(
                    "original-config",
                    renderpilot_detection::sha256_bytes(old_bytes).expect("old digest"),
                )
                .expect("original receipt"),
                old_bytes.to_vec(),
            )
            .expect("configuration baseline"),
        )
        .expect("replacement state")
    }

    fn topology(game_id: &GameId, id: &str) -> GameProxyTopology {
        let proxy = PathRef::new("C:/Games/Test/dxgi.dll").expect("proxy");
        GameProxyTopology {
            id: id.to_owned(),
            game_id: game_id.clone(),
            root_slot: proxy.clone(),
            outer: ProxyLink {
                implementation: ProxyImplementation::OptiScaler,
                path: proxy,
                receipt: FileReceipt::owned(
                    "proxy-id",
                    Sha256Hash::new("a".repeat(64)).expect("hash"),
                )
                .expect("receipt"),
            },
            downstream: None,
            downstream_origin: None,
            root_prestate: renderpilot_domain::ProxyRootPrestate::Absent,
        }
    }

    fn raw_state(state: &OptiScalerInstallState) -> RawOptiScalerInstallState {
        RawOptiScalerInstallState {
            game_id: state.game_id.as_str().to_owned(),
            release_id: state.release_id.clone(),
            manifest_revision: state.manifest_revision.clone(),
            archive_sha256: state
                .archive_sha256
                .as_ref()
                .map(|hash| hash.as_str().to_owned()),
            source: state.source.clone(),
            target_exe_path: state.target_exe_path.as_str().to_owned(),
            target_dir: state.target_dir.as_str().to_owned(),
            modules_json: mapping::serialize_json(&state.modules).expect("modules"),
            release_files_json: mapping::serialize_json(&state.release_files).expect("files"),
            runtime_bindings_json: mapping::serialize_json(&state.runtime_bindings)
                .expect("bindings"),
            directory_receipts_json: mapping::serialize_json(&state.directory_receipts)
                .expect("directories"),
            proxy_topology_id: state.proxy_topology_id.clone(),
            config_schema: state.config_schema,
            config_base_release: state.config_base_release.clone(),
            adoption_state: state.adoption_state.as_str().to_owned(),
            prerequisite_binding: state.prerequisite_binding.as_str().to_owned(),
            created_at: state.created_at,
            updated_at: state.updated_at,
            configuration_baseline_json: codec::encode(state.configuration_baseline())
                .expect("baseline"),
        }
    }

    #[test]
    fn aggregate_observation_preserves_missing_invalid_and_sql_outcomes() {
        assert!(matches!(
            observe_raw(Ok(None)).expect("missing row"),
            RowObservation::Missing
        ));

        let game_id = GameId::new("steam:observation-outcomes").expect("game id");
        let mut invalid = raw_state(&state(&game_id, "topology:observation-outcomes"));
        invalid.modules_json = "{".to_owned();
        assert!(matches!(
            observe_raw(Ok(Some(invalid))).expect("invalid row observation"),
            RowObservation::Invalid(_)
        ));

        assert!(observe_raw(Err(rusqlite::Error::InvalidQuery)).is_err());
    }

    #[test]
    fn release_receipt_round_trips_without_column_drift() {
        let storage = SqliteStorage::in_memory().expect("storage");
        let game_id = GameId::new("steam:receipt-roundtrip").expect("game id");
        storage
            .upsert_game(&GameInstallation::new(
                GameIdentity::new(game_id.clone(), "Test Game", Launcher::Steam).expect("identity"),
                Platform::Windows,
                GameRuntime::NativeWindows,
                PathRef::new("C:/Games/Test").expect("install path"),
            ))
            .expect("game");
        let topology = topology(&game_id, "optiscaler:receipt-roundtrip");
        let state = state(&game_id, &topology.id);

        storage
            .with_transaction(|transaction| {
                proxy_topologies::upsert_within_transaction(transaction, &topology)?;
                upsert_within_transaction(transaction, &state)
            })
            .expect("persist state");

        let persisted = storage
            .get_optiscaler_install_state(&game_id)
            .expect("read state")
            .expect("state");
        assert_eq!(persisted.release_id, state.release_id);
        assert_eq!(persisted.release_files, state.release_files);
        assert_eq!(persisted.runtime_bindings, state.runtime_bindings);
        assert_eq!(persisted.adoption_state, state.adoption_state);
    }

    #[test]
    fn prerequisite_binding_round_trips_lists_and_is_immutable() {
        let storage = SqliteStorage::in_memory().expect("storage");
        let game_id = GameId::new("steam:prerequisite-binding").expect("game id");
        storage
            .upsert_game(&GameInstallation::new(
                GameIdentity::new(game_id.clone(), "Test Game", Launcher::Steam).expect("identity"),
                Platform::Windows,
                GameRuntime::NativeWindows,
                PathRef::new("C:/Games/Test").expect("install path"),
            ))
            .expect("game");
        let topology = topology(&game_id, "optiscaler:prerequisite-binding");
        let mut state = state(&game_id, &topology.id);
        state.prerequisite_binding = OptiScalerPrerequisiteBinding::Luma;
        storage
            .with_transaction(|transaction| {
                proxy_topologies::upsert_within_transaction(transaction, &topology)?;
                upsert_within_transaction(transaction, &state)
            })
            .expect("persist state");

        let persisted = storage
            .get_optiscaler_install_state(&game_id)
            .expect("read")
            .expect("state");
        assert_eq!(
            persisted.prerequisite_binding,
            OptiScalerPrerequisiteBinding::Luma
        );
        assert_eq!(
            storage.list_optiscaler_install_states().expect("list")[0].prerequisite_binding,
            OptiScalerPrerequisiteBinding::Luma,
        );
        storage
            .with_connection_mut(|connection| {
                assert!(connection
                    .execute(
                        "UPDATE optiscaler_install_states SET prerequisite_binding='none' WHERE game_id=?1",
                        [game_id.as_str()],
                    )
                    .is_err());
                Ok(())
            })
            .expect("verify immutable binding");
    }

    #[test]
    fn raw_unknown_or_missing_prerequisite_binding_fails_closed() {
        let game_id = GameId::new("steam:invalid-prerequisite-binding").expect("game id");
        let state = state(&game_id, "topology:invalid-prerequisite-binding");
        let mut unknown = raw_state(&state);
        unknown.prerequisite_binding = "unknown".to_owned();
        assert!(decode_state(unknown).is_err());

        let storage = SqliteStorage::in_memory().expect("storage");
        storage
            .upsert_game(&GameInstallation::new(
                GameIdentity::new(game_id.clone(), "Test Game", Launcher::Steam).expect("identity"),
                Platform::Windows,
                GameRuntime::NativeWindows,
                PathRef::new("C:/Games/Test").expect("install path"),
            ))
            .expect("game");
        let topology = topology(&game_id, "topology:invalid-prerequisite-binding");
        storage
            .with_transaction(|transaction| {
                proxy_topologies::upsert_within_transaction(transaction, &topology)?;
                upsert_within_transaction(transaction, &state)
            })
            .expect("persist state");
        assert!(storage
            .with_connection(|connection| {
                connection
                    .query_row(
                        "SELECT game_id, release_id, manifest_revision, archive_sha256, source, \
                         target_exe_path, target_dir, modules_json, release_files_json, \
                         runtime_bindings_json, directory_receipts_json, proxy_topology_id, \
                         config_schema, config_base_release, adoption_state, created_at, updated_at, \
                         configuration_baseline_json \
                         FROM optiscaler_install_states LIMIT 1",
                        [],
                        raw_state_from_row,
                    )
                    .map_err(storage_error)
            })
            .is_err());
    }

    #[test]
    fn persistence_timestamps_are_storage_owned_and_created_at_survives_upsert() {
        let storage = SqliteStorage::in_memory().expect("storage");
        let game_id = GameId::new("steam:storage-owned-timestamps").expect("game id");
        storage
            .upsert_game(&GameInstallation::new(
                GameIdentity::new(game_id.clone(), "Test Game", Launcher::Steam).expect("identity"),
                Platform::Windows,
                GameRuntime::NativeWindows,
                PathRef::new("C:/Games/Test").expect("install path"),
            ))
            .expect("game");
        let topology = topology(&game_id, "optiscaler:storage-owned-timestamps");
        let mut state = state(&game_id, &topology.id);
        state.created_at = Some(1);
        state.updated_at = Some(1);

        storage
            .with_transaction(|transaction| {
                proxy_topologies::upsert_within_transaction(transaction, &topology)?;
                upsert_within_transaction(transaction, &state)
            })
            .expect("persist state");
        let first = storage
            .get_optiscaler_install_state(&game_id)
            .expect("read first state")
            .expect("first state");
        assert_ne!(first.created_at, Some(1));
        assert_ne!(first.updated_at, Some(1));

        storage
            .with_connection_mut(|connection| {
                connection
                    .execute(
                        "UPDATE optiscaler_install_states SET created_at=100, updated_at=200 WHERE game_id=?1",
                        [game_id.as_str()],
                    )
                    .map_err(storage_error)?;
                Ok(())
            })
            .expect("seed persisted timestamps");

        let mut changed = state;
        changed.release_id = "v0.9.5".to_owned();
        changed.created_at = Some(7);
        changed.updated_at = Some(8);
        storage
            .with_transaction(|transaction| upsert_within_transaction(transaction, &changed))
            .expect("upsert changed state");

        let second = storage
            .get_optiscaler_install_state(&game_id)
            .expect("read second state")
            .expect("second state");
        assert_eq!(second.created_at, Some(100));
        assert!(second.updated_at.expect("updated timestamp") > 200);
    }

    #[test]
    fn present_baseline_round_trips_immutably_and_revision_advances_once() {
        let storage = SqliteStorage::in_memory().expect("storage");
        let game_id = GameId::new("steam:baseline-revision").expect("game id");
        storage
            .upsert_game(&GameInstallation::new(
                GameIdentity::new(game_id.clone(), "Test Game", Launcher::Steam).expect("identity"),
                Platform::Windows,
                GameRuntime::NativeWindows,
                PathRef::new("C:/Games/Test").expect("install path"),
            ))
            .expect("game");
        let topology = topology(&game_id, "optiscaler:baseline-revision");
        let initial = replacement_state(&game_id, &topology.id);

        storage
            .with_transaction(|transaction| {
                proxy_topologies::upsert_within_transaction(transaction, &topology)?;
                upsert_within_transaction(transaction, &initial)
            })
            .expect("persist initial state");
        let first_json: String = storage
            .with_connection(|connection| {
                connection
                    .query_row(
                        "SELECT configuration_baseline_json FROM optiscaler_install_states WHERE game_id=?1",
                        [game_id.as_str()],
                        |row| row.get(0),
                    )
                    .map_err(storage_error)
            })
            .expect("read baseline json");
        assert!(first_json.contains("W09wdGlTY2FsZXJdClVzZXI9dHJ1ZQo="));

        let mut next_parts = OptiScalerInstallStateParts::from(&initial);
        next_parts.release_id = "v0.9.5".to_owned();
        let next = OptiScalerInstallState::from_existing(&initial, next_parts).expect("successor");
        storage
            .with_transaction(|transaction| upsert_within_transaction(transaction, &next))
            .expect("persist successor");

        let second_json: String = storage
            .with_connection(|connection| {
                connection
                    .query_row(
                        "SELECT configuration_baseline_json FROM optiscaler_install_states WHERE game_id=?1",
                        [game_id.as_str()],
                        |row| row.get(0),
                    )
                    .map_err(storage_error)
            })
            .expect("read baseline");
        assert_eq!(second_json, first_json);
        assert_eq!(
            storage
                .get_optiscaler_install_state(&game_id)
                .expect("read state")
                .expect("state")
                .configuration_baseline(),
            next.configuration_baseline()
        );

        let absent_json =
            codec::encode(&OptiScalerConfigurationBaseline::absent()).expect("absent json");
        storage
            .with_connection_mut(|connection| {
                let result = connection.execute(
                    "UPDATE optiscaler_install_states SET configuration_baseline_json=?1 WHERE game_id=?2",
                    rusqlite::params![absent_json, game_id.as_str()],
                );
                assert!(result.is_err(), "baseline rebasing must trigger");
                Ok(())
            })
            .expect("verify immutable trigger");
    }

    #[test]
    fn unknown_runtime_binding_field_fails_closed_on_rehydration() {
        let storage = SqliteStorage::in_memory().expect("storage");
        let game_id = GameId::new("steam:unknown-runtime-binding").expect("game id");
        storage
            .upsert_game(&GameInstallation::new(
                GameIdentity::new(game_id.clone(), "Test Game", Launcher::Steam).expect("identity"),
                Platform::Windows,
                GameRuntime::NativeWindows,
                PathRef::new("C:/Games/Test").expect("install path"),
            ))
            .expect("game");
        let topology = topology(&game_id, "optiscaler:runtime-binding-roundtrip");
        let state = state(&game_id, &topology.id);
        storage
            .with_transaction(|transaction| {
                proxy_topologies::upsert_within_transaction(transaction, &topology)?;
                upsert_within_transaction(transaction, &state)
            })
            .expect("persist state");

        let unknown_bindings = serde_json::json!([{
            "module": "core",
            "path": "C:/Games/Test/core.dll",
            "installed": {"unexpected":"foreign", "identity":"foreign", "digest":"c".repeat(64), "ownership":"reused"}
        }])
        .to_string();
        storage
            .with_connection_mut(|connection| {
                connection
                    .execute(
                        "UPDATE optiscaler_install_states SET runtime_bindings_json=?1 WHERE game_id=?2",
                         rusqlite::params![unknown_bindings, game_id.as_str()],
                    )
                    .map_err(storage_error)?;
                Ok(())
            })
            .expect("write unknown shape");

        storage
            .get_optiscaler_install_state(&game_id)
            .expect_err("unknown receipt shape must fail closed");
    }

    #[test]
    fn owned_runtime_with_present_baseline_fails_closed_on_rehydration() {
        let storage = SqliteStorage::in_memory().expect("storage");
        let game_id = GameId::new("steam:invalid-owned-runtime-baseline").expect("game id");
        storage
            .upsert_game(&GameInstallation::new(
                GameIdentity::new(game_id.clone(), "Test Game", Launcher::Steam).expect("identity"),
                Platform::Windows,
                GameRuntime::NativeWindows,
                PathRef::new("C:/Games/Test").expect("install path"),
            ))
            .expect("game");
        let topology = topology(&game_id, "optiscaler:invalid-owned-runtime-baseline");
        let state = state(&game_id, &topology.id);
        storage
            .with_transaction(|transaction| {
                proxy_topologies::upsert_within_transaction(transaction, &topology)?;
                upsert_within_transaction(transaction, &state)
            })
            .expect("persist state");

        let digest = Sha256Hash::new("c".repeat(64)).expect("runtime hash");
        let invalid_bindings = serde_json::to_string(&[OptiScalerModuleRuntimeBinding {
            module: "core".to_owned(),
            path: PathRef::new("C:/Games/Test/core.dll").expect("runtime"),
            installed: FileReceipt::owned("runtime-id", digest.clone()).expect("installed"),
            baseline: OptiScalerFileBaseline::Present {
                receipt: FileReceipt::reused("runtime-id", digest).expect("baseline"),
            },
        }])
        .expect("serialize invalid unreleased row");
        storage
            .with_connection_mut(|connection| {
                connection
                    .execute(
                        "UPDATE optiscaler_install_states SET runtime_bindings_json=?1 WHERE game_id=?2",
                        rusqlite::params![invalid_bindings, game_id.as_str()],
                    )
                    .map_err(storage_error)?;
                Ok(())
            })
            .expect("write invalid unreleased row");

        let error = storage
            .get_optiscaler_install_state(&game_id)
            .expect_err("Owned + Present must fail closed on repository load");
        assert!(
            error
                .to_string()
                .contains("owned runtime binding has a present baseline")
        );
    }

    #[test]
    fn database_rejects_an_empty_release_receipt_atomically() {
        let storage = SqliteStorage::in_memory().expect("storage");
        let game_id = GameId::new("steam:empty-receipt").expect("game id");
        storage
            .upsert_game(&GameInstallation::new(
                GameIdentity::new(game_id.clone(), "Test Game", Launcher::Steam).expect("identity"),
                Platform::Windows,
                GameRuntime::NativeWindows,
                PathRef::new("C:/Games/Test").expect("install path"),
            ))
            .expect("game");
        let topology = topology(&game_id, "optiscaler:empty-receipt");
        let mut state = state(&game_id, &topology.id);
        state.release_files.clear();

        storage
            .with_transaction(|transaction| {
                proxy_topologies::upsert_within_transaction(transaction, &topology)?;
                upsert_within_transaction(transaction, &state)
            })
            .expect_err("empty release evidence must not be persisted");
        assert!(
            storage
                .get_proxy_topology(&game_id)
                .expect("read topology")
                .is_none(),
            "the failed state write must roll back the topology too"
        );
    }
}
