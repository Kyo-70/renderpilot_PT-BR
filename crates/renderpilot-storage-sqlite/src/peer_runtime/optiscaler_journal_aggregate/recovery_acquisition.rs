//! Restart-safe acquisition of durable pending mutation candidates.
//!
//! The selection callback runs while one IMMEDIATE transaction is holding the
//! database write reservation. It must therefore be pure and non-reentrant:
//! it may inspect the supplied public row only and must not call storage or
//! perform filesystem work. The transaction is committed before any candidate
//! is returned, so no SQLite transaction is held across native recovery.

use renderpilot_application::{AppError, AppResult};
use renderpilot_domain::GameId;
use rusqlite::{OptionalExtension, Transaction};

use super::super::aggregate::AggregateGeneration;
use super::super::permit::PeerStorageRuntime;
use super::model::{
    PendingFileMutationRecoveryCandidate, PreparedRecoveryCatalogBinding,
    RecoveringOptiScalerJournalAggregate,
};
use super::validation::read_pending;
use crate::error::storage_error;
use crate::repositories::peer_aggregate_reservations::{
    PeerAggregateKind, PeerAggregateReservation, PeerAggregateReservationState,
    read_within_transaction,
};
use crate::repositories::pending_file_mutations::{
    PendingFileMutationRow, PendingFileMutationState,
};

impl PeerStorageRuntime {
    /// Acquires the selected pending mutation candidates for one game.
    ///
    /// Rows are visited in deterministic `created_at, id` order. `select` is
    /// evaluated inside the IMMEDIATE transaction and must be a pure,
    /// non-reentrant predicate: it may only inspect its argument and must not
    /// call this runtime, another storage API, or touch the filesystem. Only
    /// selected rows are deeply validated. The transaction commits before the
    /// returned owned candidates can be used by restart recovery.
    pub fn recover_pending_file_mutation_candidates_for_game(
        &self,
        game_id: &GameId,
        select: impl Fn(&PendingFileMutationRow) -> bool,
    ) -> AppResult<Vec<PendingFileMutationRecoveryCandidate>> {
        self.repositories()
            .with_immediate_transaction(|transaction| {
                let rows = read_public_rows(transaction, game_id)?;
                let mut candidates = Vec::new();
                for row in rows {
                    if !select(&row) {
                        continue;
                    }
                    candidates.push(acquire_selected(transaction, row)?);
                }
                Ok(candidates)
            })
    }
}

fn read_public_rows(
    transaction: &Transaction<'_>,
    game_id: &GameId,
) -> AppResult<Vec<PendingFileMutationRow>> {
    let mut statement = transaction
        .prepare(
            "SELECT id, game_id, feature, subject_id, state, manifest_json
             FROM pending_file_mutations
             WHERE game_id = ?1
             ORDER BY created_at, id",
        )
        .map_err(storage_error)?;
    let rows = statement
        .query_map([game_id.as_str()], |row| {
            let game_id: String = row.get(1)?;
            let state: String = row.get(4)?;
            Ok(PendingFileMutationRow {
                id: row.get(0)?,
                game_id: GameId::new(game_id).map_err(|error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        1,
                        rusqlite::types::Type::Text,
                        Box::new(error),
                    )
                })?,
                feature: row.get(2)?,
                subject_id: row.get(3)?,
                state: state.parse().map_err(|error: AppError| {
                    rusqlite::Error::FromSqlConversionFailure(
                        4,
                        rusqlite::types::Type::Text,
                        Box::new(error),
                    )
                })?,
                manifest_json: row.get(5)?,
            })
        })
        .map_err(storage_error)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(storage_error)
}

fn acquire_selected(
    transaction: &Transaction<'_>,
    public_row: PendingFileMutationRow,
) -> AppResult<PendingFileMutationRecoveryCandidate> {
    let hidden = read_pending(transaction, &public_row.id)?.ok_or_else(|| {
        AppError::storage_failed("selected pending file mutation disappeared during recovery")
    })?;
    validate_public_projection(&hidden, &public_row)?;

    let exact_reservation =
        read_within_transaction(transaction, &public_row.game_id, &public_row.id)?;
    let any_reservation = read_reservation_for_game(transaction, &public_row.game_id)?;
    let marker = aggregate_marker(&hidden)?;

    match (marker, exact_reservation) {
        (None, None) if any_reservation.is_none() => {
            if super::validation::is_optiscaler_feature(&public_row.feature) {
                return Err(AppError::storage_failed(format!(
                    "OptiScaler feature '{}' is missing its optiscaler_journal aggregate binding",
                    public_row.feature
                )));
            }
            Ok(PendingFileMutationRecoveryCandidate::NotAggregate(
                public_row,
            ))
        }
        (None, _) => Err(AppError::storage_failed(
            "pending file mutation has a reservation without an aggregate binding",
        )),
        (Some(marker), Some(reservation)) => {
            validate_aggregate_pair(&public_row, &marker, &reservation)?;
            let expected_generation = expected_generation(public_row.state, marker.revision)?;
            validate_game_generation(transaction, &public_row.game_id, expected_generation)?;

            if marker.kind != PeerAggregateKind::OptiScalerJournal {
                return Err(AppError::storage_failed(
                    "pending file mutation carries an aggregate kind that is not valid for its table",
                ));
            }
            if !super::validation::is_optiscaler_feature(&public_row.feature) {
                return Err(AppError::storage_failed(
                    "optiscaler_journal binding is attached to a non-OptiScaler feature",
                ));
            }
            let journal = crate::repositories::parse_optiscaler_journal_for_recovery(
                &hidden.manifest_json,
                state_as_str(public_row.state),
            )?;
            let catalog_binding = if public_row.state == PendingFileMutationState::Prepared {
                let binding = crate::repositories::pending_file_mutations::
                    read_optiscaler_catalog_binding_for_recovery(
                        transaction,
                        &public_row.game_id,
                        &public_row.id,
                    )?;
                Some(match binding {
                    None => PreparedRecoveryCatalogBinding::CatalogAbsent,
                    Some((authority_epoch, mutation_token)) => {
                        PreparedRecoveryCatalogBinding::CatalogInvalidated {
                            authority_epoch,
                            mutation_token,
                        }
                    }
                })
            } else {
                None
            };
            Ok(PendingFileMutationRecoveryCandidate::OptiScaler(Box::new(
                RecoveringOptiScalerJournalAggregate {
                    operation_id: public_row.id,
                    game_id: public_row.game_id,
                    feature: public_row.feature,
                    subject_id: public_row.subject_id,
                    state: public_row.state,
                    journal,
                    current_journal_json: hidden.manifest_json,
                    pending_identity: hidden.identity,
                    aggregate_kind: marker.kind,
                    aggregate_revision: marker.revision,
                    reservation,
                    expected_generation,
                    catalog_binding,
                },
            )))
        }
        (Some(_), None) => Err(AppError::storage_failed(
            "pending file mutation aggregate binding has no matching reservation",
        )),
    }
}

fn validate_public_projection(
    hidden: &super::validation::PendingJournalRow,
    public: &PendingFileMutationRow,
) -> AppResult<()> {
    if hidden.game_id != public.game_id.as_str()
        || hidden.feature != public.feature
        || hidden.subject_id != public.subject_id
        || hidden.state != state_as_str(public.state)
        || hidden.manifest_json != public.manifest_json
    {
        return Err(AppError::storage_failed(
            "pending file mutation public projection changed during recovery",
        ));
    }
    Ok(())
}

#[derive(Debug)]
struct AggregateMarker {
    kind: PeerAggregateKind,
    revision: AggregateGeneration,
}

fn aggregate_marker(
    row: &super::validation::PendingJournalRow,
) -> AppResult<Option<AggregateMarker>> {
    match (row.aggregate_kind.as_deref(), row.aggregate_revision) {
        (None, None) => Ok(None),
        (None, _) => Err(AppError::storage_failed(
            "pending file mutation has a partial aggregate binding",
        )),
        (Some(kind), Some(revision)) => Ok(Some(AggregateMarker {
            kind: parse_aggregate_kind(kind)?,
            revision: AggregateGeneration::from_persisted(revision).map_err(|error| {
                AppError::storage_failed(format!(
                    "pending file mutation aggregate revision is invalid: {error}"
                ))
            })?,
        })),
        (Some(_), None) => Err(AppError::storage_failed(
            "pending file mutation aggregate kind has no revision",
        )),
    }
}

fn parse_aggregate_kind(value: &str) -> AppResult<PeerAggregateKind> {
    match value {
        "optiscaler_journal" => Ok(PeerAggregateKind::OptiScalerJournal),
        "shared_peer" => Ok(PeerAggregateKind::SharedPeer),
        "metadata" => Ok(PeerAggregateKind::Metadata),
        _ => Err(AppError::storage_failed(format!(
            "pending file mutation has an invalid aggregate kind `{value}`",
        ))),
    }
}

fn validate_aggregate_pair(
    public: &PendingFileMutationRow,
    marker: &AggregateMarker,
    reservation: &PeerAggregateReservation,
) -> AppResult<()> {
    if reservation.game_id() != &public.game_id
        || reservation.operation_id() != public.id
        || reservation.kind() != marker.kind
        || reservation.binding() != marker.kind.binding()
        || reservation.state() != reservation_state(public.state)
        || reservation.expected_revision() != marker.revision
    {
        return Err(AppError::storage_failed(
            "pending file mutation aggregate binding and reservation disagree",
        ));
    }

    match marker.kind {
        PeerAggregateKind::OptiScalerJournal => Ok(()),
        PeerAggregateKind::SharedPeer | PeerAggregateKind::Metadata => {
            Err(AppError::storage_failed(
                "pending file mutation carries an aggregate kind that is not valid for its table",
            ))
        }
    }
}

fn expected_generation(
    state: PendingFileMutationState,
    revision: AggregateGeneration,
) -> AppResult<AggregateGeneration> {
    match state {
        PendingFileMutationState::Preparing | PendingFileMutationState::Prepared => Ok(revision),
        PendingFileMutationState::Committed => revision.checked_successor().map_err(|error| {
            AppError::storage_failed(format!(
                "committed pending aggregate generation cannot advance: {error}"
            ))
        }),
    }
}

fn validate_game_generation(
    transaction: &Transaction<'_>,
    game_id: &GameId,
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
        .ok_or_else(|| AppError::storage_failed("pending aggregate game is missing"))
        .and_then(|value| {
            AggregateGeneration::from_persisted(value).map_err(|error| {
                AppError::storage_failed(format!(
                    "pending aggregate game generation is invalid: {error}"
                ))
            })
        })?;
    if current != expected {
        return Err(AppError::storage_failed(
            "pending aggregate game generation does not match its reservation",
        ));
    }
    Ok(())
}

fn read_reservation_for_game(
    transaction: &Transaction<'_>,
    game_id: &GameId,
) -> AppResult<Option<String>> {
    transaction
        .query_row(
            "SELECT operation_id FROM peer_aggregate_reservations WHERE game_id = ?1",
            [game_id.as_str()],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage_error)
}

fn reservation_state(state: PendingFileMutationState) -> PeerAggregateReservationState {
    match state {
        PendingFileMutationState::Preparing => PeerAggregateReservationState::Preparing,
        PendingFileMutationState::Prepared => PeerAggregateReservationState::Prepared,
        PendingFileMutationState::Committed => PeerAggregateReservationState::Committed,
    }
}

fn state_as_str(state: PendingFileMutationState) -> &'static str {
    match state {
        PendingFileMutationState::Preparing => "preparing",
        PendingFileMutationState::Prepared => "prepared",
        PendingFileMutationState::Committed => "committed",
    }
}
