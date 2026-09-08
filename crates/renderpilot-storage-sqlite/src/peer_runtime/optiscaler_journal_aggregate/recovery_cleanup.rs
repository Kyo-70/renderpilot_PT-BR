//! Terminal cleanup for an acquired OptiScaler recovery proof.
//!
//! Recovery owns no filesystem or participant authority.  These operations
//! only remove the pending journal and its matching reservation after the
//! native recovery caller has persisted the exact terminal journal state.

use renderpilot_application::{AppError, AppResult};
use rusqlite::named_params;

use super::super::permit::PeerStorageRuntime;
use super::model::RecoveringOptiScalerJournalAggregate;
use super::recovery_cas::{
    pending_state_as_str, reservation_state_for, validate_prepared_catalog_binding,
    validate_recovery_proof_within_transaction,
};
use super::validation::read_pending;
use crate::error::storage_error;
use crate::repositories::peer_aggregate_reservations::{
    PeerAggregateKind, delete_within_transaction, read_within_transaction,
};
use crate::repositories::pending_file_mutations::PendingFileMutationState;

impl PeerStorageRuntime {
    /// Deletes a rollback-terminal Preparing recovery pair atomically.
    pub fn delete_preparing_recovering_optiscaler_journal_aggregate_after_rollback(
        &self,
        recovering: RecoveringOptiScalerJournalAggregate,
    ) -> AppResult<()> {
        delete_recovering_terminal(
            self,
            recovering,
            PendingFileMutationState::Preparing,
            crate::repositories::validate_optiscaler_journal_for_rollback_terminal,
        )
    }

    /// Deletes a rollback-terminal Prepared recovery pair atomically.  The
    /// captured catalog binding is read and compared but never advanced.
    pub fn delete_prepared_recovering_optiscaler_journal_aggregate_after_rollback(
        &self,
        recovering: RecoveringOptiScalerJournalAggregate,
    ) -> AppResult<()> {
        delete_recovering_terminal(
            self,
            recovering,
            PendingFileMutationState::Prepared,
            crate::repositories::validate_optiscaler_journal_for_rollback_terminal,
        )
    }

    /// Deletes only committed durable cleanup evidence.  No filesystem or
    /// participant repository is touched by this recovery-only operation.
    pub fn delete_committed_recovering_optiscaler_journal_aggregate(
        &self,
        recovering: RecoveringOptiScalerJournalAggregate,
    ) -> AppResult<()> {
        delete_recovering_terminal(
            self,
            recovering,
            PendingFileMutationState::Committed,
            crate::repositories::validate_optiscaler_journal_for_committed_terminal,
        )
    }
}

fn delete_recovering_terminal(
    runtime: &PeerStorageRuntime,
    recovering: RecoveringOptiScalerJournalAggregate,
    expected_state: PendingFileMutationState,
    validate_terminal: fn(&str) -> AppResult<()>,
) -> AppResult<()> {
    if recovering.state() != expected_state {
        return Err(AppError::storage_failed(
            "OptiScaler recovery terminal operation received the wrong row state",
        ));
    }

    runtime
        .repositories()
        .with_immediate_transaction(move |transaction| {
            validate_recovery_proof_within_transaction(transaction, &recovering)?;
            if expected_state == PendingFileMutationState::Prepared {
                let binding = recovering.catalog_binding.as_ref().ok_or_else(|| {
                    AppError::storage_failed(
                        "Prepared OptiScaler recovery proof has no catalog binding",
                    )
                })?;
                validate_prepared_catalog_binding(
                    transaction,
                    &recovering.game_id,
                    &recovering.operation_id,
                    binding,
                )?;
            }
            validate_terminal(&recovering.current_journal_json)?;

            let row_state = pending_state_as_str(expected_state);
            let deleted = transaction
                .execute(
                    "DELETE FROM pending_file_mutations
                     WHERE rowid = :rowid AND id = :id
                       AND game_id = :game_id AND feature = :feature
                       AND subject_id IS :subject_id
                       AND state = :state AND manifest_json = :manifest_json
                       AND aggregate_kind = :aggregate_kind
                       AND aggregate_revision = :aggregate_revision
                       AND created_at = :created_at AND updated_at = :updated_at",
                    named_params! {
                        ":rowid": recovering.pending_identity.rowid,
                        ":id": recovering.operation_id.as_str(),
                        ":game_id": recovering.game_id.as_str(),
                        ":feature": recovering.feature.as_str(),
                        ":subject_id": recovering.subject_id.as_deref(),
                        ":state": row_state,
                        ":manifest_json": recovering.current_journal_json.as_str(),
                        ":aggregate_kind": PeerAggregateKind::OptiScalerJournal.as_str(),
                        ":aggregate_revision": recovering.aggregate_revision.as_i64(),
                        ":created_at": recovering.pending_identity.created_at,
                        ":updated_at": recovering.pending_identity.updated_at,
                    },
                )
                .map_err(storage_error)?;
            if deleted != 1 {
                return Err(AppError::storage_failed(
                    "OptiScaler recovery terminal row changed before deletion",
                ));
            }

            let reservation_state = reservation_state_for(expected_state);
            delete_within_transaction(
                transaction,
                &recovering.game_id,
                &recovering.operation_id,
                PeerAggregateKind::OptiScalerJournal,
                recovering.aggregate_revision,
                reservation_state,
            )?;
            if read_pending(transaction, &recovering.operation_id)?.is_some() {
                return Err(AppError::storage_failed(
                    "OptiScaler recovery terminal row remained after deletion",
                ));
            }
            if read_within_transaction(transaction, &recovering.game_id, &recovering.operation_id)?
                .is_some()
            {
                return Err(AppError::storage_failed(
                    "OptiScaler recovery terminal reservation remained after deletion",
                ));
            }
            super::validation::validate_game_revision(
                transaction,
                &recovering.game_id,
                recovering.expected_generation,
            )?;
            Ok(())
        })
}
