//! Restart-safe, same-lifecycle CAS for an acquired OptiScaler journal.
//!
//! Recovery proofs are deliberately independent from the runtime permit used
//! by an active operation.  Every use therefore re-fences the complete
//! durable fingerprint inside one IMMEDIATE transaction.  Lifecycle changes
//! that publish a game aggregate are not part of this API: recovery can only
//! advance the journal JSON while its row and reservation remain in the same
//! state.

use renderpilot_application::{AppError, AppResult};
use rusqlite::{Transaction, named_params};

use super::super::aggregate::AggregateGeneration;
use super::model::{PreparedRecoveryCatalogBinding, RecoveringOptiScalerJournalAggregate};
use super::validation::{
    PendingJournalRow, read_pending, validate_game_revision, validate_reservation,
};
use crate::error::storage_error;
use crate::peer_runtime::permit::PeerStorageRuntime;
use crate::repositories::peer_aggregate_reservations::{
    PeerAggregateKind, PeerAggregateReservationState,
};
use crate::repositories::pending_file_mutations::PendingFileMutationState;
use crate::sqlite_clock;

impl RecoveringOptiScalerJournalAggregate {
    /// Returns the durable lifecycle state of this recovery proof.
    #[must_use]
    pub fn state(&self) -> PendingFileMutationState {
        self.state
    }

    /// Returns the exact serialized journal CAS token carried by this proof.
    #[must_use]
    pub fn current_journal_json(&self) -> &str {
        &self.current_journal_json
    }
}

impl PeerStorageRuntime {
    /// Compare-and-swaps one acquired recovery journal without changing its
    /// durable lifecycle.  Preparing, Prepared, and Committed each use their
    /// own canonical journal rules; row state, reservation state, aggregate
    /// generation, and Prepared catalog authority remain unchanged.
    ///
    /// The proof is consumed and replaced by a successor proof containing the
    /// newly persisted JSON and storage-owned row update timestamp.
    pub fn cas_recovering_optiscaler_journal_aggregate(
        &self,
        recovering: RecoveringOptiScalerJournalAggregate,
        next_journal_json: impl Into<String>,
    ) -> AppResult<RecoveringOptiScalerJournalAggregate> {
        let RecoveringOptiScalerJournalAggregate {
            operation_id,
            game_id,
            feature,
            subject_id,
            state,
            journal,
            current_journal_json,
            pending_identity,
            aggregate_kind,
            aggregate_revision,
            reservation,
            expected_generation,
            catalog_binding,
        } = recovering;
        let next_journal_json = next_journal_json.into();
        let row_state = pending_state_as_str(state);
        let reservation_state = reservation_state_for(state);

        let (next_journal, next_identity, next_reservation) = self
            .repositories()
            .with_immediate_transaction(|transaction| {
                let proof = RecoveringOptiScalerJournalAggregate {
                    operation_id: operation_id.clone(),
                    game_id: game_id.clone(),
                    feature: feature.clone(),
                    subject_id: subject_id.clone(),
                    state,
                    journal,
                    current_journal_json: current_journal_json.clone(),
                    pending_identity,
                    aggregate_kind,
                    aggregate_revision,
                    reservation: reservation.clone(),
                    expected_generation,
                    catalog_binding: catalog_binding.as_ref().map(|binding| match binding {
                        PreparedRecoveryCatalogBinding::CatalogAbsent => {
                            PreparedRecoveryCatalogBinding::CatalogAbsent
                        }
                        PreparedRecoveryCatalogBinding::CatalogInvalidated {
                            authority_epoch,
                            mutation_token,
                        } => PreparedRecoveryCatalogBinding::CatalogInvalidated {
                            authority_epoch: *authority_epoch,
                            mutation_token: mutation_token.clone(),
                        },
                    }),
                };
                validate_recovery_proof_within_transaction(transaction, &proof)?;
                if state == PendingFileMutationState::Prepared {
                    validate_prepared_catalog_binding(
                        transaction,
                        &game_id,
                        &operation_id,
                        catalog_binding.as_ref().ok_or_else(|| {
                            AppError::storage_failed(
                                "Prepared OptiScaler recovery proof has no catalog binding",
                            )
                        })?,
                    )?;
                }
                let next_journal = crate::repositories::parse_optiscaler_journal_for_recovery(
                    &next_journal_json,
                    row_state,
                )?;
                crate::repositories::validate_optiscaler_journal_for_cas(
                    &current_journal_json,
                    &next_journal_json,
                    row_state,
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
                            ":id": operation_id.as_str(),
                            ":game_id": game_id.as_str(),
                            ":feature": feature.as_str(),
                            ":subject_id": subject_id.as_deref(),
                            ":state": row_state,
                            ":current_json": current_journal_json.as_str(),
                            ":next_json": next_journal_json.as_str(),
                            ":aggregate_kind": PeerAggregateKind::OptiScalerJournal.as_str(),
                            ":aggregate_revision": aggregate_revision.as_i64(),
                            ":created_at": pending_identity.created_at,
                            ":updated_at": pending_identity.updated_at,
                            ":now_ms": sqlite_clock::now_ms(transaction)?,
                        },
                    )
                    .map_err(storage_error)?;
                if updated != 1 {
                    return Err(AppError::storage_failed(
                        "OptiScaler recovery journal changed before CAS",
                    ));
                }

                let updated_row = read_pending(transaction, &operation_id)?.ok_or_else(|| {
                    AppError::storage_failed("OptiScaler recovery journal disappeared after CAS")
                })?;
                validate_recovery_successor_row(
                    &updated_row,
                    &proof,
                    &next_journal_json,
                    &next_journal,
                )?;
                let next_reservation =
                    validate_reservation(transaction, &reservation, reservation_state)?;
                if next_reservation != reservation {
                    return Err(AppError::storage_failed(
                        "OptiScaler recovery reservation changed during CAS",
                    ));
                }
                validate_game_revision(transaction, &game_id, expected_generation)?;
                if state == PendingFileMutationState::Prepared {
                    validate_prepared_catalog_binding(
                        transaction,
                        &game_id,
                        &operation_id,
                        catalog_binding.as_ref().ok_or_else(|| {
                            AppError::storage_failed(
                                "Prepared OptiScaler recovery proof has no catalog binding",
                            )
                        })?,
                    )?;
                }
                Ok((next_journal, updated_row.identity, next_reservation))
            })?;

        Ok(RecoveringOptiScalerJournalAggregate {
            operation_id,
            game_id,
            feature,
            subject_id,
            state,
            journal: next_journal,
            current_journal_json: next_journal_json,
            pending_identity: next_identity,
            aggregate_kind,
            aggregate_revision,
            reservation: next_reservation,
            expected_generation,
            catalog_binding,
        })
    }
}

/// Re-fences every private value in a recovery proof.  This helper is shared
/// by same-state CAS and terminal cleanup so the two paths cannot drift in
/// their row/reservation/generation checks.
pub(super) fn validate_recovery_proof_within_transaction(
    transaction: &Transaction<'_>,
    proof: &RecoveringOptiScalerJournalAggregate,
) -> AppResult<()> {
    if !super::validation::is_optiscaler_feature(&proof.feature)
        || proof.aggregate_kind != PeerAggregateKind::OptiScalerJournal
        || proof.aggregate_revision != proof.reservation.expected_revision()
        || proof.reservation.game_id() != &proof.game_id
        || proof.reservation.operation_id() != proof.operation_id
        || proof.reservation.kind() != PeerAggregateKind::OptiScalerJournal
        || proof.reservation.binding() != PeerAggregateKind::OptiScalerJournal.binding()
        || proof.expected_generation
            != expected_generation_for(proof.state, proof.aggregate_revision)?
    {
        return Err(AppError::storage_failed(
            "OptiScaler recovery proof has an invalid aggregate fingerprint",
        ));
    }
    let expected_reservation_state = reservation_state_for(proof.state);
    let current_reservation = super::validation::validate_reservation(
        transaction,
        &proof.reservation,
        expected_reservation_state,
    )?;
    if current_reservation != proof.reservation {
        return Err(AppError::storage_failed(
            "OptiScaler recovery reservation changed before operation",
        ));
    }
    let row = read_pending(transaction, &proof.operation_id)?
        .ok_or_else(|| AppError::storage_failed("OptiScaler recovery journal row disappeared"))?;
    validate_recovery_row(&row, proof, &proof.current_journal_json, &proof.journal)?;
    validate_game_revision(transaction, &proof.game_id, proof.expected_generation)
}

fn validate_recovery_row(
    row: &PendingJournalRow,
    proof: &RecoveringOptiScalerJournalAggregate,
    expected_json: &str,
    expected_journal: &renderpilot_domain::OptiScalerJournal,
) -> AppResult<()> {
    if row.rowid != proof.pending_identity.rowid
        || row.identity.rowid != proof.pending_identity.rowid
        || row.identity.created_at != proof.pending_identity.created_at
        || row.identity.updated_at < proof.pending_identity.updated_at
        || row.game_id != proof.game_id.as_str()
        || row.feature != proof.feature
        || row.subject_id != proof.subject_id
        || row.state != pending_state_as_str(proof.state)
        || row.manifest_json != expected_json
        || row.aggregate_kind.as_deref() != Some(PeerAggregateKind::OptiScalerJournal.as_str())
        || row.aggregate_revision != Some(proof.aggregate_revision.as_i64())
    {
        return Err(AppError::storage_failed(
            "OptiScaler recovery journal row identity or state changed",
        ));
    }
    let parsed = crate::repositories::parse_optiscaler_journal_for_recovery(
        expected_json,
        pending_state_as_str(proof.state),
    )?;
    if &parsed != expected_journal {
        return Err(AppError::storage_failed(
            "OptiScaler recovery journal JSON does not match its proof",
        ));
    }
    Ok(())
}

fn validate_recovery_successor_row(
    row: &PendingJournalRow,
    proof: &RecoveringOptiScalerJournalAggregate,
    next_json: &str,
    next_journal: &renderpilot_domain::OptiScalerJournal,
) -> AppResult<()> {
    if row.rowid != proof.pending_identity.rowid
        || row.identity.rowid != proof.pending_identity.rowid
        || row.identity.created_at != proof.pending_identity.created_at
        || row.identity.updated_at < proof.pending_identity.updated_at
    {
        return Err(AppError::storage_failed(
            "OptiScaler recovery successor changed row identity or timestamp",
        ));
    }
    if row.game_id != proof.game_id.as_str()
        || row.feature != proof.feature
        || row.subject_id != proof.subject_id
        || row.state != pending_state_as_str(proof.state)
        || row.manifest_json != next_json
        || row.aggregate_kind.as_deref() != Some(PeerAggregateKind::OptiScalerJournal.as_str())
        || row.aggregate_revision != Some(proof.aggregate_revision.as_i64())
    {
        return Err(AppError::storage_failed(
            "OptiScaler recovery successor row identity or state changed",
        ));
    }
    let parsed = crate::repositories::parse_optiscaler_journal_for_recovery(
        next_json,
        pending_state_as_str(proof.state),
    )?;
    if &parsed != next_journal {
        return Err(AppError::storage_failed(
            "OptiScaler recovery successor journal JSON does not match its proof",
        ));
    }
    Ok(())
}

pub(super) fn validate_prepared_catalog_binding(
    transaction: &Transaction<'_>,
    game_id: &renderpilot_domain::GameId,
    operation_id: &str,
    expected: &PreparedRecoveryCatalogBinding,
) -> AppResult<()> {
    let observed =
        crate::repositories::pending_file_mutations::read_optiscaler_catalog_binding_for_recovery(
            transaction,
            game_id,
            operation_id,
        )?;
    match (expected, observed) {
        (PreparedRecoveryCatalogBinding::CatalogAbsent, None) => Ok(()),
        (
            PreparedRecoveryCatalogBinding::CatalogInvalidated {
                authority_epoch,
                mutation_token,
            },
            Some((observed_epoch, observed_token)),
        ) if *authority_epoch == observed_epoch && mutation_token == &observed_token => Ok(()),
        _ => Err(AppError::storage_failed(
            "Prepared OptiScaler recovery catalog binding changed",
        )),
    }
}

pub(super) fn pending_state_as_str(state: PendingFileMutationState) -> &'static str {
    match state {
        PendingFileMutationState::Preparing => "preparing",
        PendingFileMutationState::Prepared => "prepared",
        PendingFileMutationState::Committed => "committed",
    }
}

pub(super) fn reservation_state_for(
    state: PendingFileMutationState,
) -> PeerAggregateReservationState {
    match state {
        PendingFileMutationState::Preparing => PeerAggregateReservationState::Preparing,
        PendingFileMutationState::Prepared => PeerAggregateReservationState::Prepared,
        PendingFileMutationState::Committed => PeerAggregateReservationState::Committed,
    }
}

pub(super) fn expected_generation_for(
    state: PendingFileMutationState,
    revision: AggregateGeneration,
) -> AppResult<AggregateGeneration> {
    match state {
        PendingFileMutationState::Preparing | PendingFileMutationState::Prepared => Ok(revision),
        PendingFileMutationState::Committed => revision.checked_successor().map_err(|error| {
            AppError::storage_failed(format!(
                "committed OptiScaler recovery generation cannot advance: {error}"
            ))
        }),
    }
}
