use renderpilot_application::{AppError, AppResult};
use rusqlite::{OptionalExtension, Transaction};

use super::super::aggregate::AggregateGeneration;
use super::super::aggregate::MAX_METADATA_BYTES;
use super::model::{OptiScalerJournalAggregateBegin, PendingJournalIdentity};
use crate::error::storage_error;
use crate::repositories::peer_aggregate_reservations::{
    PeerAggregateKind, PeerAggregateReservation, PeerAggregateReservationState,
    read_within_transaction,
};

pub(super) fn validate_begin(begin: &OptiScalerJournalAggregateBegin) -> AppResult<()> {
    validate_text("operation id", begin.operation_id())?;
    validate_text("feature", begin.feature())?;
    if let Some(subject_id) = begin.subject_id() {
        validate_text("subject id", subject_id)?;
    }
    if !is_optiscaler_feature(begin.feature()) {
        return Err(AppError::invalid_input(
            "OptiScaler journal aggregate requires an OptiScaler feature",
        ));
    }
    crate::repositories::validate_optiscaler_journal_for_begin(begin.initial_journal_json())
}

pub(super) fn validate_final_journal(final_journal_json: &str) -> AppResult<()> {
    crate::repositories::validate_optiscaler_journal_for_prepared(final_journal_json)
}

fn validate_text(field: &str, value: &str) -> AppResult<()> {
    if value.trim().is_empty() || value.contains('\0') {
        return Err(AppError::invalid_input(format!(
            "OptiScaler journal aggregate {field} is invalid"
        )));
    }
    if value.len() > MAX_METADATA_BYTES {
        return Err(AppError::invalid_input(format!(
            "OptiScaler journal aggregate {field} exceeds the metadata limit"
        )));
    }
    Ok(())
}

pub(super) fn is_optiscaler_feature(feature: &str) -> bool {
    matches!(
        feature,
        renderpilot_domain::mutation_features::OPTISCALER_INSTALL
            | renderpilot_domain::mutation_features::OPTISCALER_UPDATE
            | renderpilot_domain::mutation_features::OPTISCALER_RELOCATE
            | renderpilot_domain::mutation_features::OPTISCALER_UNINSTALL
    )
}

pub(super) fn read_pending(
    transaction: &Transaction<'_>,
    operation_id: &str,
) -> AppResult<Option<PendingJournalRow>> {
    transaction
        .query_row(
            "SELECT rowid, game_id, feature, subject_id, state, manifest_json,
                    created_at, updated_at, aggregate_kind, aggregate_revision
             FROM pending_file_mutations
             WHERE id = ?1",
            [operation_id],
            |row| {
                Ok(PendingJournalRow {
                    rowid: row.get(0)?,
                    game_id: row.get(1)?,
                    feature: row.get(2)?,
                    subject_id: row.get(3)?,
                    state: row.get(4)?,
                    manifest_json: row.get(5)?,
                    identity: PendingJournalIdentity {
                        rowid: row.get(0)?,
                        created_at: row.get(6)?,
                        updated_at: row.get(7)?,
                    },
                    aggregate_kind: row.get(8)?,
                    aggregate_revision: row.get(9)?,
                })
            },
        )
        .optional()
        .map_err(storage_error)
}

pub(super) fn validate_reservation(
    transaction: &Transaction<'_>,
    expected: &PeerAggregateReservation,
    state: PeerAggregateReservationState,
) -> AppResult<PeerAggregateReservation> {
    let current =
        read_within_transaction(transaction, expected.game_id(), expected.operation_id())?
            .ok_or_else(|| {
                AppError::storage_failed("OptiScaler journal aggregate reservation disappeared")
            })?;
    if &current != expected
        || current.kind() != PeerAggregateKind::OptiScalerJournal
        || current.binding() != PeerAggregateKind::OptiScalerJournal.binding()
        || current.state() != state
    {
        return Err(AppError::storage_failed(
            "OptiScaler journal aggregate reservation identity or state changed",
        ));
    }
    Ok(current)
}

pub(super) fn validate_game_revision(
    transaction: &Transaction<'_>,
    game_id: &renderpilot_domain::GameId,
    expected: AggregateGeneration,
) -> AppResult<()> {
    let current: Option<i64> = transaction
        .query_row(
            "SELECT peer_aggregate_revision FROM games WHERE id = ?1",
            [game_id.as_str()],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage_error)?;
    let current = current
        .ok_or_else(|| AppError::storage_failed("OptiScaler journal aggregate game disappeared"))
        .and_then(AggregateGeneration::from_persisted)?;
    if current != expected {
        return Err(AppError::storage_failed(
            "OptiScaler journal aggregate game revision changed",
        ));
    }
    Ok(())
}

pub(super) fn validate_preparing_row(
    row: &PendingJournalRow,
    begin: &OptiScalerJournalAggregateBegin,
    current_journal_json: &str,
    expected_revision: super::super::aggregate::AggregateGeneration,
    expected_identity: PendingJournalIdentity,
) -> AppResult<()> {
    if row.rowid != expected_identity.rowid
        || row.identity != expected_identity
        || row.game_id != begin.game_id().as_str()
        || row.feature != begin.feature()
        || row.subject_id.as_deref() != begin.subject_id()
        || row.state != "preparing"
        || row.manifest_json != current_journal_json
        || row.aggregate_kind.as_deref() != Some(PeerAggregateKind::OptiScalerJournal.as_str())
        || row.aggregate_revision != Some(expected_revision.as_i64())
    {
        return Err(AppError::storage_failed(
            "OptiScaler journal aggregate pending row identity or Preparing state changed",
        ));
    }
    Ok(())
}

pub(super) fn validate_prepared_row(
    row: &PendingJournalRow,
    begin: &OptiScalerJournalAggregateBegin,
    final_journal_json: &str,
    expected_revision: super::super::aggregate::AggregateGeneration,
    expected_identity: PendingJournalIdentity,
) -> AppResult<()> {
    if row.rowid != expected_identity.rowid
        || row.identity.rowid != expected_identity.rowid
        || row.identity.created_at != expected_identity.created_at
        || row.game_id != begin.game_id().as_str()
        || row.feature != begin.feature()
        || row.subject_id.as_deref() != begin.subject_id()
        || row.state != "prepared"
        || row.manifest_json != final_journal_json
        || row.aggregate_kind.as_deref() != Some(PeerAggregateKind::OptiScalerJournal.as_str())
        || row.aggregate_revision != Some(expected_revision.as_i64())
    {
        return Err(AppError::storage_failed(
            "OptiScaler journal aggregate pending row does not match Prepared proof",
        ));
    }
    Ok(())
}

pub(super) fn validate_committed_row(
    row: &PendingJournalRow,
    begin: &OptiScalerJournalAggregateBegin,
    final_journal_json: &str,
    expected_revision: super::super::aggregate::AggregateGeneration,
    expected_identity: PendingJournalIdentity,
) -> AppResult<()> {
    if row.rowid != expected_identity.rowid
        || row.identity.rowid != expected_identity.rowid
        || row.identity.created_at != expected_identity.created_at
        || row.identity.updated_at < expected_identity.updated_at
        || row.game_id != begin.game_id().as_str()
        || row.feature != begin.feature()
        || row.subject_id.as_deref() != begin.subject_id()
        || row.state != "committed"
        || row.manifest_json != final_journal_json
        || row.aggregate_kind.as_deref() != Some(PeerAggregateKind::OptiScalerJournal.as_str())
        || row.aggregate_revision != Some(expected_revision.as_i64())
    {
        return Err(AppError::storage_failed(
            "OptiScaler journal aggregate committed row does not match its proof",
        ));
    }
    Ok(())
}

/// Validates the row reread after a journal JSON CAS.  The row identity is
/// deliberately split here: rowid and creation time must remain stable, while
/// the storage-owned update clock may stay equal on coarse clocks but may
/// never move backwards.
pub(super) fn validate_cas_row(
    row: &PendingJournalRow,
    begin: &OptiScalerJournalAggregateBegin,
    expected_state: &str,
    expected_json: &str,
    expected_revision: super::super::aggregate::AggregateGeneration,
    before_identity: PendingJournalIdentity,
) -> AppResult<PendingJournalIdentity> {
    if row.rowid != before_identity.rowid
        || row.identity.rowid != before_identity.rowid
        || row.identity.created_at != before_identity.created_at
        || row.identity.updated_at < before_identity.updated_at
        || row.game_id != begin.game_id().as_str()
        || row.feature != begin.feature()
        || row.subject_id.as_deref() != begin.subject_id()
        || row.state != expected_state
        || row.manifest_json != expected_json
        || row.aggregate_kind.as_deref() != Some(PeerAggregateKind::OptiScalerJournal.as_str())
        || row.aggregate_revision != Some(expected_revision.as_i64())
    {
        return Err(AppError::storage_failed(
            "OptiScaler journal aggregate CAS row does not match its new proof",
        ));
    }
    Ok(row.identity)
}

/// Validates the exact row image used as the before-side of a journal JSON
/// compare-and-swap.  Unlike [`validate_cas_row`], every timestamp is part of
/// this fence because the SQL update must consume precisely the proof that
/// was handed to the caller.
pub(super) fn validate_cas_before_row(
    row: &PendingJournalRow,
    begin: &OptiScalerJournalAggregateBegin,
    expected_state: &str,
    expected_json: &str,
    expected_revision: super::super::aggregate::AggregateGeneration,
    expected_identity: PendingJournalIdentity,
) -> AppResult<()> {
    if row.rowid != expected_identity.rowid
        || row.identity != expected_identity
        || row.game_id != begin.game_id().as_str()
        || row.feature != begin.feature()
        || row.subject_id.as_deref() != begin.subject_id()
        || row.state != expected_state
        || row.manifest_json != expected_json
        || row.aggregate_kind.as_deref() != Some(PeerAggregateKind::OptiScalerJournal.as_str())
        || row.aggregate_revision != Some(expected_revision.as_i64())
    {
        return Err(AppError::storage_failed(
            "OptiScaler journal aggregate CAS before row does not match its proof",
        ));
    }
    Ok(())
}

#[derive(Debug)]
pub(super) struct PendingJournalRow {
    pub(super) rowid: i64,
    pub(super) game_id: String,
    pub(super) feature: String,
    pub(super) subject_id: Option<String>,
    pub(super) state: String,
    pub(super) manifest_json: String,
    pub(super) identity: PendingJournalIdentity,
    pub(super) aggregate_kind: Option<String>,
    pub(super) aggregate_revision: Option<i64>,
}
