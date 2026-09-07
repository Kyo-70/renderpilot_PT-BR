use renderpilot_application::{AppError, AppResult};

use crate::error::storage_error;

use super::super::super::SqliteStorage;
use super::super::model::PendingFileMutationState;

impl SqliteStorage {
    /// Deletes only an unfinished preparation after its app-owned snapshots
    /// were cleaned without touching game paths.
    pub fn abandon_file_mutation_preparation(&self, id: &str) -> AppResult<()> {
        delete_pending_file_mutation_in_state(self, id, PendingFileMutationState::Preparing)
    }

    /// Deletes a committed row after app-owned snapshot cleanup.
    pub fn cleanup_committed_file_mutation(&self, id: &str) -> AppResult<()> {
        delete_pending_file_mutation_in_state(self, id, PendingFileMutationState::Committed)
    }
}

fn delete_pending_file_mutation_in_state(
    storage: &SqliteStorage,
    id: &str,
    state: PendingFileMutationState,
) -> AppResult<()> {
    storage.with_connection(|connection| {
        let deleted = connection
            .execute(
                "DELETE FROM pending_file_mutations WHERE id = ?1 AND state = ?2",
                [id, state.as_str()],
            )
            .map_err(storage_error)?;
        if deleted != 1 {
            return Err(AppError::storage_failed(format!(
                "pending file mutation '{id}' is not {}",
                state.as_str()
            )));
        }
        Ok(())
    })
}
