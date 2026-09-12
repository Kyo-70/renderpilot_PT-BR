use renderpilot_application::{AppError, AppResult};
use renderpilot_domain::GameId;
use rusqlite::{OptionalExtension, named_params};

use crate::{error::storage_error, sqlite_clock};

use super::super::super::{SqliteStorage, observations, peer_aggregate_reservations};
use super::super::binding::classify_catalog_binding_within_transaction;
use super::super::model::{
    BeginFileMutationPreparation, CatalogBinding, PendingFileMutationRow, PendingFileMutationState,
};
use super::validation::{
    row_to_pending_mutation, validate_begin_preparation, validate_prepared_manifest_for_feature,
};

impl SqliteStorage {
    /// Reserves a mutation before its first game-folder write.
    ///
    /// This always inserts literal `preparing`. The initial manifest may be
    /// incomplete but must be a JSON object; finishing requires the full
    /// before-snapshot manifest and atomically invalidates catalog authority.
    pub fn begin_file_mutation_preparation(
        &self,
        begin: &BeginFileMutationPreparation,
    ) -> AppResult<()> {
        validate_begin_preparation(begin)?;
        self.with_immediate_transaction(|transaction| {
            peer_aggregate_reservations::ensure_no_peer_aggregate_reservation_within_transaction(
                transaction,
                &begin.game_id,
            )?;
            super::super::super::pending_shared_vulkan_mutations::assert_no_shared_mutation_id_within_transaction(
                transaction,
                &begin.id,
            )?;
            super::super::super::pending_shared_vulkan_mutations::assert_no_shared_mutation_for_game_within_transaction(
                transaction,
                &begin.game_id,
            )?;
            let now_ms = sqlite_clock::now_ms(transaction)?;
            transaction
                .execute(
                    "
                    INSERT INTO pending_file_mutations
                        (id, game_id, feature, subject_id, state, manifest_json,
                         created_at, updated_at)
                    VALUES
                        (:id, :game_id, :feature, :subject_id, :state, :manifest_json,
                         :now_ms, :now_ms)
                    ",
                    named_params! {
                        ":id": begin.id.as_str(),
                        ":game_id": begin.game_id.as_str(),
                        ":feature": begin.feature.as_str(),
                        ":subject_id": begin.subject_id.as_deref(),
                        ":state": PendingFileMutationState::Preparing.as_str(),
                        ":manifest_json": begin.initial_manifest_json.as_str(),
                        ":now_ms": now_ms,
                    },
                )
                .map_err(storage_error)?;
            Ok(())
        })
    }

    /// Lists unfinished or not-yet-cleaned mutations for one game oldest first.
    pub fn pending_file_mutations_for_game(
        &self,
        game_id: &GameId,
    ) -> AppResult<Vec<PendingFileMutationRow>> {
        self.query_list(
            "
            SELECT id, game_id, feature, subject_id, state, manifest_json
            FROM pending_file_mutations
            WHERE game_id = ?1
            ORDER BY created_at, id
            ",
            [game_id.as_str()],
            |row| Ok(row_to_pending_mutation(row)),
        )
    }

    /// Publishes the completed before-snapshot manifest and opens the game-file
    /// mutation phase.
    pub fn finish_preparing_file_mutation(&self, id: &str, manifest_json: &str) -> AppResult<()> {
        self.with_transaction(|transaction| {
            let (game_id, feature): (String, String) = transaction
                .query_row(
                    "SELECT game_id, feature FROM pending_file_mutations
                     WHERE id = ?1 AND state = 'preparing'",
                    [id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()
                .map_err(storage_error)?
                .ok_or_else(|| {
                    AppError::storage_failed(format!(
                        "pending file mutation '{id}' is missing or is not preparing"
                    ))
                })?;
            validate_prepared_manifest_for_feature(&feature, manifest_json)?;
            let game_id = GameId::new(game_id)
                .map_err(|error| AppError::storage_failed(error.to_string()))?;
            let binding = classify_catalog_binding_within_transaction(transaction, &game_id)?;
            let now_ms = sqlite_clock::now_ms(transaction)?;
            let updated = transaction
                .execute(
                    "
                    UPDATE pending_file_mutations
                    SET state = 'prepared', manifest_json = :manifest_json, updated_at = :now_ms
                    WHERE id = :id AND state = 'preparing'
                    ",
                    named_params! {
                        ":id": id,
                        ":manifest_json": manifest_json,
                        ":now_ms": now_ms,
                    },
                )
                .map_err(storage_error)?;
            if updated != 1 {
                return Err(AppError::storage_failed(format!(
                    "pending file mutation `{id}` is missing or is not preparing"
                )));
            }
            if matches!(binding, CatalogBinding::CatalogPresent(_)) {
                observations::invalidate_game_authority_within_transaction(
                    transaction,
                    &game_id,
                    "prepared_file_mutation",
                    Some(id),
                )?;
            }
            Ok(())
        })
    }

    /// Reads one mutation by id for tests and recovery diagnostics.
    pub fn get_pending_file_mutation(&self, id: &str) -> AppResult<Option<PendingFileMutationRow>> {
        self.with_connection(|connection| {
            connection
                .query_row(
                    "
                    SELECT id, game_id, feature, subject_id, state, manifest_json
                    FROM pending_file_mutations WHERE id = ?1
                    ",
                    [id],
                    |row| Ok(row_to_pending_mutation(row)),
                )
                .optional()
                .map_err(storage_error)?
                .transpose()
        })
    }

    /// Test-only malformed-state fixture. Production callers cannot insert a
    /// caller-selected state; they must use `begin_file_mutation_preparation`.
    #[cfg(test)]
    pub(crate) fn prepare_file_mutation(&self, row: &PendingFileMutationRow) -> AppResult<()> {
        self.with_immediate_transaction(|transaction| {
            super::super::super::pending_shared_vulkan_mutations::assert_no_shared_mutation_id_within_transaction(
                transaction,
                &row.id,
            )?;
            super::super::super::pending_shared_vulkan_mutations::assert_no_shared_mutation_for_game_within_transaction(
                transaction,
                &row.game_id,
            )?;
            let now_ms = sqlite_clock::now_ms(transaction)?;
            transaction
                .execute(
                    "INSERT INTO pending_file_mutations
                     (id, game_id, feature, subject_id, state, manifest_json, created_at, updated_at)
                     VALUES (:id, :game_id, :feature, :subject_id, :state, :manifest_json, :now_ms, :now_ms)",
                    named_params! {
                        ":id": row.id.as_str(),
                        ":game_id": row.game_id.as_str(),
                        ":feature": row.feature.as_str(),
                        ":subject_id": row.subject_id.as_deref(),
                        ":state": row.state.as_str(),
                        ":manifest_json": row.manifest_json.as_str(),
                        ":now_ms": now_ms,
                    },
                )
                .map_err(storage_error)?;
            Ok(())
        })
    }
}
