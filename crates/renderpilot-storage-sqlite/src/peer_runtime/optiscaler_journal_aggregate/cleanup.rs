use renderpilot_application::{AppError, AppResult};
use rusqlite::named_params;

use super::super::aggregate::AggregateGeneration;
use super::super::permit::{PeerStorageRuntime, ensure_runtime};
use super::model::{
    CommittedOptiScalerJournalAggregate, OptiScalerJournalAggregateBegin,
    PreparedOptiScalerJournalAggregate, PreparingOptiScalerJournalAggregate,
};
use super::validation::{
    read_pending, validate_cas_before_row, validate_game_revision, validate_reservation,
};
use crate::error::storage_error;
use crate::repositories::peer_aggregate_reservations::{
    PeerAggregateKind, PeerAggregateReservation, PeerAggregateReservationState,
    delete_within_transaction, read_within_transaction,
};

/// Exact durable identity and terminal validator for one journal cleanup.
///
/// These values form one proof: separating them at call sites makes it too
/// easy to accidentally pair a valid row identity with the wrong terminal
/// state or game-generation fence.
#[derive(Clone, Copy)]
struct TerminalJournal<'a> {
    begin: &'a OptiScalerJournalAggregateBegin,
    reservation: &'a PeerAggregateReservation,
    pending_identity: super::model::PendingJournalIdentity,
    current_journal_json: &'a str,
    expected_state: PeerAggregateReservationState,
    expected_generation: AggregateGeneration,
    catalog_binding:
        Option<&'a crate::repositories::pending_file_mutations::PreparedOptiScalerCatalogBinding>,
    validate_terminal: fn(&str) -> AppResult<()>,
}

impl PeerStorageRuntime {
    /// Deletes a terminal rollback journal while it is still in Preparing.
    /// The pending row and its reservation are removed atomically; no game
    /// path or participant repository is touched.
    pub fn delete_preparing_optiscaler_journal_aggregate_after_rollback(
        &self,
        preparing: PreparingOptiScalerJournalAggregate,
    ) -> AppResult<()> {
        let PreparingOptiScalerJournalAggregate {
            runtime_identity,
            reservation,
            begin,
            current_journal_json,
            pending_identity,
        } = preparing;
        ensure_runtime(&self.runtime_instance(), &runtime_identity)?;
        delete_terminal(
            self,
            &TerminalJournal {
                begin: &begin,
                reservation: &reservation,
                pending_identity,
                current_journal_json: &current_journal_json,
                expected_state: PeerAggregateReservationState::Preparing,
                expected_generation: reservation.expected_revision(),
                catalog_binding: None,
                validate_terminal:
                    crate::repositories::validate_optiscaler_journal_for_rollback_terminal,
            },
        )
    }

    /// Deletes a terminal rollback journal while it is in Prepared.
    /// Prepared cleanup remains reservation-bound and is never widened into a
    /// committed cleanup route.
    pub fn delete_prepared_optiscaler_journal_aggregate_after_rollback(
        &self,
        prepared: PreparedOptiScalerJournalAggregate,
    ) -> AppResult<()> {
        let PreparedOptiScalerJournalAggregate {
            runtime_identity,
            reservation,
            begin,
            catalog_binding,
            current_journal_json,
            pending_identity,
        } = prepared;
        ensure_runtime(&self.runtime_instance(), &runtime_identity)?;
        delete_terminal(
            self,
            &TerminalJournal {
                begin: &begin,
                reservation: &reservation,
                pending_identity,
                current_journal_json: &current_journal_json,
                expected_state: PeerAggregateReservationState::Prepared,
                expected_generation: reservation.expected_revision(),
                catalog_binding: Some(&catalog_binding),
                validate_terminal:
                    crate::repositories::validate_optiscaler_journal_for_rollback_terminal,
            },
        )
    }

    /// Deletes only the committed journal's durable cleanup evidence.  This
    /// operation intentionally accepts no filesystem authority and performs
    /// no live-file or participant restoration.
    pub fn delete_committed_optiscaler_journal_aggregate(
        &self,
        committed: CommittedOptiScalerJournalAggregate,
    ) -> AppResult<()> {
        let CommittedOptiScalerJournalAggregate {
            runtime_identity,
            aggregate,
            reservation,
            begin,
            current_journal_json,
            pending_identity,
        } = committed;
        ensure_runtime(&self.runtime_instance(), &runtime_identity)?;
        let expected_generation = reservation.expected_revision().checked_successor()?;
        if aggregate.operation_id() != begin.operation_id()
            || aggregate.after().game_id() != begin.game_id()
            || aggregate.generation() != expected_generation
        {
            return Err(AppError::storage_failed(
                "OptiScaler committed journal proof generation or identity changed",
            ));
        }
        delete_terminal(
            self,
            &TerminalJournal {
                begin: &begin,
                reservation: &reservation,
                pending_identity,
                current_journal_json: &current_journal_json,
                expected_state: PeerAggregateReservationState::Committed,
                expected_generation: aggregate.generation(),
                catalog_binding: None,
                validate_terminal:
                    crate::repositories::validate_optiscaler_journal_for_committed_terminal,
            },
        )
    }
}

fn delete_terminal(runtime: &PeerStorageRuntime, input: &TerminalJournal<'_>) -> AppResult<()> {
    let TerminalJournal {
        begin,
        reservation,
        pending_identity,
        current_journal_json,
        expected_state,
        expected_generation,
        catalog_binding,
        validate_terminal,
    } = *input;
    let operation_id = begin.operation_id();
    let game_id = begin.game_id();
    let expected_revision = reservation.expected_revision();
    if reservation.operation_id() != operation_id || reservation.game_id() != game_id {
        return Err(AppError::storage_failed(
            "OptiScaler journal aggregate proof reservation is not bound to its begin identity",
        ));
    }
    runtime
        .repositories()
        .with_immediate_transaction(|transaction| {
            if let Some(catalog_binding) = catalog_binding {
                crate::repositories::pending_file_mutations::validate_optiscaler_catalog_binding(
                    transaction,
                    game_id,
                    operation_id,
                    catalog_binding,
                )?;
            }
            validate_reservation(transaction, reservation, expected_state)?;
            let row = read_pending(transaction, operation_id)?.ok_or_else(|| {
                AppError::storage_failed("OptiScaler journal aggregate terminal row disappeared")
            })?;
            validate_cas_before_row(
                &row,
                begin,
                expected_state.as_str(),
                current_journal_json,
                expected_revision,
                pending_identity,
            )?;
            validate_game_revision(transaction, game_id, expected_generation)?;
            validate_terminal(current_journal_json)?;

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
                        ":rowid": pending_identity.rowid,
                        ":id": operation_id,
                        ":game_id": game_id.as_str(),
                        ":feature": begin.feature(),
                        ":subject_id": begin.subject_id(),
                        ":state": expected_state.as_str(),
                        ":manifest_json": current_journal_json,
                        ":aggregate_kind": PeerAggregateKind::OptiScalerJournal.as_str(),
                        ":aggregate_revision": expected_revision.as_i64(),
                        ":created_at": pending_identity.created_at,
                        ":updated_at": pending_identity.updated_at,
                    },
                )
                .map_err(storage_error)?;
            if deleted != 1 {
                return Err(AppError::storage_failed(
                    "OptiScaler journal aggregate terminal row changed before deletion",
                ));
            }
            delete_within_transaction(
                transaction,
                game_id,
                operation_id,
                PeerAggregateKind::OptiScalerJournal,
                expected_revision,
                expected_state,
            )?;
            if read_pending(transaction, operation_id)?.is_some() {
                return Err(AppError::storage_failed(
                    "OptiScaler journal aggregate terminal row remained after deletion",
                ));
            }
            if read_within_transaction(transaction, game_id, operation_id)?.is_some() {
                return Err(AppError::storage_failed(
                    "OptiScaler journal aggregate reservation remained after deletion",
                ));
            }
            validate_game_revision(transaction, game_id, expected_generation)?;
            Ok(())
        })
}
