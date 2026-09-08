use renderpilot_application::{AppError, AppResult};
use rusqlite::named_params;

use super::super::aggregate::AggregateGeneration;
use super::super::permit::{PeerStorageRuntime, ensure_runtime};
use super::model::{
    CommittedOptiScalerJournalAggregate, OptiScalerJournalAggregateBegin,
    PreparedOptiScalerJournalAggregate, PreparingOptiScalerJournalAggregate,
};
use super::validation::{
    read_pending, validate_cas_before_row, validate_cas_row, validate_game_revision,
    validate_reservation,
};
use crate::error::storage_error;
use crate::repositories::peer_aggregate_reservations::{
    PeerAggregateKind, PeerAggregateReservation, PeerAggregateReservationState,
};
use crate::sqlite_clock;

/// Exact preconditions for one journal-row compare-and-swap.
///
/// Grouping them keeps the row identity, reservation state, and game-generation
/// fence inseparable at the storage CAS boundary.
#[derive(Clone, Copy)]
struct JournalCas<'a> {
    begin: &'a OptiScalerJournalAggregateBegin,
    reservation: &'a PeerAggregateReservation,
    pending_identity: super::model::PendingJournalIdentity,
    current_json: &'a str,
    next_json: &'a str,
    expected_state: PeerAggregateReservationState,
    expected_generation: AggregateGeneration,
    catalog_binding:
        Option<&'a crate::repositories::pending_file_mutations::PreparedOptiScalerCatalogBinding>,
}

impl PeerStorageRuntime {
    /// Advances a Preparing journal through one canonical journal CAS.
    ///
    /// The proof is consumed and replaced.  The reservation and game
    /// generation remain untouched; only the pending row's JSON and storage
    /// update clock are changed under the exact proof identity.
    pub fn cas_preparing_optiscaler_journal_aggregate(
        &self,
        preparing: PreparingOptiScalerJournalAggregate,
        next_journal_json: impl Into<String>,
    ) -> AppResult<PreparingOptiScalerJournalAggregate> {
        let PreparingOptiScalerJournalAggregate {
            runtime_identity,
            reservation,
            begin,
            current_journal_json,
            pending_identity,
        } = preparing;
        ensure_runtime(&self.runtime_instance(), &runtime_identity)?;
        let next_journal_json = next_journal_json.into();
        let result = cas_journal_row(
            self,
            &JournalCas {
                begin: &begin,
                reservation: &reservation,
                pending_identity,
                current_json: &current_journal_json,
                next_json: &next_journal_json,
                expected_state: PeerAggregateReservationState::Preparing,
                expected_generation: reservation.expected_revision(),
                catalog_binding: None,
            },
        )?;
        Ok(PreparingOptiScalerJournalAggregate {
            runtime_identity: self.runtime_instance(),
            reservation: result.0,
            begin,
            current_journal_json: next_journal_json,
            pending_identity: result.1,
        })
    }

    /// Advances a Prepared journal through one canonical journal CAS.
    ///
    /// The reservation stays Prepared until the aggregate commit consumes the
    /// returned proof.  No catalog or participant write is performed here.
    pub fn cas_prepared_optiscaler_journal_aggregate(
        &self,
        prepared: PreparedOptiScalerJournalAggregate,
        next_journal_json: impl Into<String>,
    ) -> AppResult<PreparedOptiScalerJournalAggregate> {
        let PreparedOptiScalerJournalAggregate {
            runtime_identity,
            reservation,
            begin,
            catalog_binding,
            current_journal_json,
            pending_identity,
        } = prepared;
        ensure_runtime(&self.runtime_instance(), &runtime_identity)?;
        let next_journal_json = next_journal_json.into();
        let result = cas_journal_row(
            self,
            &JournalCas {
                begin: &begin,
                reservation: &reservation,
                pending_identity,
                current_json: &current_journal_json,
                next_json: &next_journal_json,
                expected_state: PeerAggregateReservationState::Prepared,
                expected_generation: reservation.expected_revision(),
                catalog_binding: Some(&catalog_binding),
            },
        )?;
        Ok(PreparedOptiScalerJournalAggregate {
            runtime_identity: self.runtime_instance(),
            reservation: result.0,
            begin,
            catalog_binding,
            current_journal_json: next_journal_json,
            pending_identity: result.1,
        })
    }

    /// Advances the committed cleanup journal through one canonical journal
    /// CAS.  Its game-generation fence is the generation published by the
    /// aggregate commit, not the reservation's pre-commit revision.
    pub fn cas_committed_optiscaler_journal_aggregate(
        &self,
        committed: CommittedOptiScalerJournalAggregate,
        next_journal_json: impl Into<String>,
    ) -> AppResult<CommittedOptiScalerJournalAggregate> {
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
        let next_journal_json = next_journal_json.into();
        let result = cas_journal_row(
            self,
            &JournalCas {
                begin: &begin,
                reservation: &reservation,
                pending_identity,
                current_json: &current_journal_json,
                next_json: &next_journal_json,
                expected_state: PeerAggregateReservationState::Committed,
                expected_generation: aggregate.generation(),
                catalog_binding: None,
            },
        )?;
        Ok(CommittedOptiScalerJournalAggregate {
            runtime_identity: self.runtime_instance(),
            aggregate,
            reservation: result.0,
            begin,
            current_journal_json: next_journal_json,
            pending_identity: result.1,
        })
    }
}

fn cas_journal_row(
    runtime: &PeerStorageRuntime,
    input: &JournalCas<'_>,
) -> AppResult<(
    PeerAggregateReservation,
    super::model::PendingJournalIdentity,
)> {
    let JournalCas {
        begin,
        reservation,
        pending_identity,
        current_json,
        next_json,
        expected_state,
        expected_generation,
        catalog_binding,
    } = *input;
    let operation_id = begin.operation_id();
    let game_id = begin.game_id();
    let expected_revision = reservation.expected_revision();
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
            let current_reservation =
                validate_reservation(transaction, reservation, expected_state)?;
            let row = read_pending(transaction, operation_id)?.ok_or_else(|| {
                AppError::storage_failed("OptiScaler journal aggregate CAS row disappeared")
            })?;
            validate_cas_before_row(
                &row,
                begin,
                expected_state.as_str(),
                current_json,
                expected_revision,
                pending_identity,
            )?;
            validate_game_revision(transaction, game_id, expected_generation)?;
            crate::repositories::validate_optiscaler_journal_for_cas(
                current_json,
                next_json,
                expected_state.as_str(),
            )?;

            let updated = transaction
                .execute(
                    "UPDATE pending_file_mutations
                 SET manifest_json = :next_json, updated_at = :now_ms
                 WHERE rowid = :rowid AND id = :id
                   AND game_id = :game_id AND feature = :feature
                   AND subject_id IS :subject_id
                   AND state = :state AND manifest_json = :current_json
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
                        ":current_json": current_json,
                        ":next_json": next_json,
                        ":aggregate_kind": PeerAggregateKind::OptiScalerJournal.as_str(),
                        ":aggregate_revision": expected_revision.as_i64(),
                        ":created_at": pending_identity.created_at,
                        ":updated_at": pending_identity.updated_at,
                        ":now_ms": sqlite_clock::now_ms(transaction)?,
                    },
                )
                .map_err(storage_error)?;
            if updated != 1 {
                return Err(AppError::storage_failed(
                    "OptiScaler journal aggregate CAS row changed before update",
                ));
            }

            let updated_row = read_pending(transaction, operation_id)?.ok_or_else(|| {
                AppError::storage_failed(
                    "OptiScaler journal aggregate CAS row disappeared after update",
                )
            })?;
            let updated_identity = validate_cas_row(
                &updated_row,
                begin,
                expected_state.as_str(),
                next_json,
                expected_revision,
                pending_identity,
            )?;
            let reread_reservation =
                validate_reservation(transaction, reservation, expected_state)?;
            if reread_reservation != current_reservation {
                return Err(AppError::storage_failed(
                    "OptiScaler journal aggregate reservation changed during CAS",
                ));
            }
            validate_game_revision(transaction, game_id, expected_generation)?;
            Ok((reread_reservation, updated_identity))
        })
}
