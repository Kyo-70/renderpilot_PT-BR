use renderpilot_application::{AppError, AppResult};
use rusqlite::named_params;

use super::super::permit::{PeerStorageRuntime, ensure_runtime};
use super::model::{
    OptiScalerJournalAggregateBegin, PreparedOptiScalerJournalAggregate,
    PreparingOptiScalerJournalAggregate,
};
use super::validation::{
    read_pending, validate_begin, validate_final_journal, validate_game_revision,
    validate_prepared_row, validate_preparing_row, validate_reservation,
};
use crate::error::storage_error;
use crate::repositories::peer_aggregate_reservations::{
    PeerAggregateKind, PeerAggregateReservationState, begin_within_transaction,
    transition_to_prepared_within_transaction,
};
use crate::sqlite_clock;

impl PeerStorageRuntime {
    /// Reserves one OptiScaler journal row before native namespace work starts.
    /// The reservation and pending row are inserted atomically, and both are
    /// reread before an opaque Preparing proof is returned.
    pub fn begin_optiscaler_journal_aggregate(
        &self,
        begin: OptiScalerJournalAggregateBegin,
    ) -> AppResult<PreparingOptiScalerJournalAggregate> {
        validate_begin(&begin)?;
        let operation_id = begin.operation_id().to_owned();
        let game_id = begin.game_id().clone();

        self.repositories()
            .with_immediate_transaction(|transaction| {
                let reservation = begin_within_transaction(
                    transaction,
                    &game_id,
                    &operation_id,
                    PeerAggregateKind::OptiScalerJournal,
                )?;
                let now_ms = sqlite_clock::now_ms(transaction)?;
                transaction
                    .execute(
                        "INSERT INTO pending_file_mutations
                         (id, game_id, feature, subject_id, state, manifest_json,
                          created_at, updated_at, aggregate_kind, aggregate_revision)
                         VALUES (:id, :game_id, :feature, :subject_id, :state,
                                 :manifest_json, :now_ms, :now_ms, :aggregate_kind,
                                 :aggregate_revision)",
                        named_params! {
                            ":id": operation_id.as_str(),
                            ":game_id": game_id.as_str(),
                            ":feature": begin.feature(),
                            ":subject_id": begin.subject_id(),
                            ":state": "preparing",
                            ":manifest_json": begin.initial_journal_json(),
                            ":now_ms": now_ms,
                            ":aggregate_kind": PeerAggregateKind::OptiScalerJournal.as_str(),
                            ":aggregate_revision": reservation.expected_revision().as_i64(),
                        },
                    )
                    .map_err(storage_error)?;
                let row = read_pending(transaction, &operation_id)?.ok_or_else(|| {
                    AppError::storage_failed(
                        "OptiScaler journal aggregate pending row disappeared after begin",
                    )
                })?;
                validate_game_revision(transaction, &game_id, reservation.expected_revision())?;
                validate_preparing_row(
                    &row,
                    &begin,
                    begin.initial_journal_json(),
                    reservation.expected_revision(),
                    row.identity,
                )?;
                Ok(PreparingOptiScalerJournalAggregate {
                    runtime_identity: self.runtime_instance(),
                    reservation,
                    current_journal_json: begin.initial_journal_json().to_owned(),
                    begin,
                    pending_identity: row.identity,
                })
            })
    }

    /// Moves one exact journal reservation from Preparing to Prepared after
    /// validating the complete journal. Participant rows remain untouched;
    /// when catalog authority exists, its observations are invalidated with
    /// the exact operation token in the same transaction.
    pub fn finish_optiscaler_journal_aggregate(
        &self,
        preparing: PreparingOptiScalerJournalAggregate,
        final_journal_json: impl Into<String>,
    ) -> AppResult<PreparedOptiScalerJournalAggregate> {
        ensure_runtime(&self.runtime_instance(), &preparing.runtime_identity)?;
        let final_journal_json = final_journal_json.into();
        validate_final_journal(&final_journal_json)?;
        let operation_id = preparing.begin.operation_id().to_owned();
        let game_id = preparing.begin.game_id().clone();

        self.repositories()
            .with_immediate_transaction(|transaction| {
                let reservation = validate_reservation(
                    transaction,
                    &preparing.reservation,
                    PeerAggregateReservationState::Preparing,
                )?;
                let row = read_pending(transaction, &operation_id)?.ok_or_else(|| {
                    AppError::storage_failed(
                        "OptiScaler journal aggregate pending row disappeared before finish",
                    )
                })?;
                validate_game_revision(
                    transaction,
                    &game_id,
                    reservation.expected_revision(),
                )?;
                validate_preparing_row(
                    &row,
                    &preparing.begin,
                    preparing.current_journal_json.as_str(),
                    reservation.expected_revision(),
                    preparing.pending_identity,
                )?;
                let catalog_binding = crate::repositories::pending_file_mutations::
                    prepare_optiscaler_catalog_binding(transaction, &game_id, &operation_id)?;
                let updated = transaction
                    .execute(
                        "UPDATE pending_file_mutations
                         SET state = :prepared, manifest_json = :manifest_json,
                             updated_at = :now_ms
                         WHERE rowid = :rowid AND id = :id
                           AND game_id = :game_id AND feature = :feature
                           AND subject_id IS :subject_id
                           AND state = :preparing
                           AND manifest_json = :current_json
                           AND aggregate_kind = :aggregate_kind
                           AND aggregate_revision = :aggregate_revision
                           AND created_at = :created_at
                           AND updated_at = :updated_at",
                        named_params! {
                            ":rowid": preparing.pending_identity.rowid,
                            ":id": operation_id.as_str(),
                            ":game_id": game_id.as_str(),
                            ":feature": preparing.begin.feature(),
                            ":subject_id": preparing.begin.subject_id(),
                            ":prepared": "prepared",
                            ":preparing": "preparing",
                            ":manifest_json": final_journal_json.as_str(),
                            ":current_json": preparing.current_journal_json.as_str(),
                            ":aggregate_kind": PeerAggregateKind::OptiScalerJournal.as_str(),
                            ":aggregate_revision": reservation.expected_revision().as_i64(),
                            ":created_at": preparing.pending_identity.created_at,
                            ":updated_at": preparing.pending_identity.updated_at,
                            ":now_ms": sqlite_clock::now_ms(transaction)?,
                        },
                    )
                    .map_err(storage_error)?;
                if updated != 1 {
                    return Err(AppError::storage_failed(
                        "OptiScaler journal aggregate pending row changed before Prepared transition",
                    ));
                }
                let prepared_reservation = transition_to_prepared_within_transaction(
                    transaction,
                    &game_id,
                    &operation_id,
                    PeerAggregateKind::OptiScalerJournal,
                    reservation.expected_revision(),
                )?;
                let prepared_row = read_pending(transaction, &operation_id)?.ok_or_else(|| {
                    AppError::storage_failed(
                        "OptiScaler journal aggregate pending row disappeared after finish",
                    )
                })?;
                validate_prepared_row(
                    &prepared_row,
                    &preparing.begin,
                    &final_journal_json,
                    reservation.expected_revision(),
                    preparing.pending_identity,
                )?;
                Ok(PreparedOptiScalerJournalAggregate {
                    runtime_identity: self.runtime_instance(),
                    reservation: prepared_reservation,
                    begin: preparing.begin,
                    catalog_binding,
                    current_journal_json: final_journal_json,
                    pending_identity: prepared_row.identity,
                })
            })
    }
}
