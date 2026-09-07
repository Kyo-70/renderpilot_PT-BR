use renderpilot_application::{AppError, AppResult};
use renderpilot_domain::GameId;
use rusqlite::{OptionalExtension, Transaction, named_params};

use crate::error::storage_error;
use crate::repositories::observation::RowObservation;
use crate::{peer_runtime::aggregate::AggregateGeneration, sqlite_clock};

use super::model::{
    PeerAggregateKind, PeerAggregateReservation, PeerAggregateReservationState,
    RawPeerAggregateReservation,
};

const RESERVATION_SELECT: &str = "SELECT game_id, operation_id, aggregate_kind,
    pending_binding, state, expected_revision, created_at, updated_at
    FROM peer_aggregate_reservations";

/// Rejects an ordinary game-scoped write while any aggregate reservation exists.
pub(crate) fn ensure_no_peer_aggregate_reservation_within_transaction(
    transaction: &Transaction<'_>,
    game_id: &GameId,
) -> AppResult<()> {
    let operation_id: Option<String> = transaction
        .query_row(
            "SELECT operation_id
             FROM peer_aggregate_reservations
             WHERE game_id = ?1
             LIMIT 1",
            [game_id.as_str()],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage_error)?;

    if let Some(operation_id) = operation_id {
        return Err(AppError::storage_failed(format!(
            "peer aggregate reservation for game `{}` (operation `{operation_id}`) blocks an ordinary mutation",
            game_id.as_str()
        )));
    }

    Ok(())
}

/// Reads and parses one reservation by its complete durable identity.
pub(crate) fn observe_within_transaction(
    transaction: &Transaction<'_>,
    game_id: &GameId,
    operation_id: &str,
) -> AppResult<RowObservation<PeerAggregateReservation>> {
    validate_operation_id(operation_id)?;
    let sql = format!(
        "{RESERVATION_SELECT}
         WHERE game_id = ?1 AND operation_id = ?2"
    );
    observe_query(transaction, &sql, [game_id.as_str(), operation_id])
}

/// Reads and parses one reservation by its complete durable identity.
pub(crate) fn read_within_transaction(
    transaction: &Transaction<'_>,
    game_id: &GameId,
    operation_id: &str,
) -> AppResult<Option<PeerAggregateReservation>> {
    observe_within_transaction(transaction, game_id, operation_id)?.into_optional()
}

/// Begins one aggregate reservation in the caller's existing IMMEDIATE write
/// transaction. The initial state is deliberately not caller-selectable.
pub(crate) fn begin_within_transaction(
    transaction: &Transaction<'_>,
    game_id: &GameId,
    operation_id: &str,
    kind: PeerAggregateKind,
) -> AppResult<PeerAggregateReservation> {
    validate_operation_id(operation_id)?;
    let expected_revision = read_game_revision(transaction, game_id)?;
    if let Some(existing) = read_for_game_within_transaction(transaction, game_id)? {
        return Err(AppError::storage_failed(format!(
            "game `{}` already has peer aggregate reservation `{}`",
            game_id.as_str(),
            existing.operation_id()
        )));
    }
    if pending_file_exists(transaction, game_id)? {
        return Err(AppError::storage_failed(format!(
            "game `{}` has an unresolved ordinary file mutation",
            game_id.as_str()
        )));
    }
    if pending_shared_game_exists(transaction, game_id)? {
        return Err(AppError::storage_failed(format!(
            "game `{}` has an unresolved game-shared Vulkan mutation",
            game_id.as_str()
        )));
    }

    let now_ms = sqlite_clock::now_ms(transaction)?;
    transaction
        .execute(
            "INSERT INTO peer_aggregate_reservations
                 (game_id, operation_id, aggregate_kind, pending_binding, state,
                  expected_revision, created_at, updated_at)
             VALUES (:game_id, :operation_id, :aggregate_kind, :pending_binding,
                     'preparing', :expected_revision, :now_ms, :now_ms)",
            named_params! {
                ":game_id": game_id.as_str(),
                ":operation_id": operation_id,
                ":aggregate_kind": kind.as_str(),
                ":pending_binding": kind.binding().as_str(),
                ":expected_revision": expected_revision.as_i64(),
                ":now_ms": now_ms,
            },
        )
        .map_err(storage_error)?;
    read_within_transaction(transaction, game_id, operation_id)?.ok_or_else(|| {
        AppError::storage_failed(format!(
            "peer aggregate reservation `{operation_id}` disappeared after begin"
        ))
    })
}

/// Performs only the legal Preparing → Prepared transition.
pub(crate) fn transition_to_prepared_within_transaction(
    transaction: &Transaction<'_>,
    game_id: &GameId,
    operation_id: &str,
    kind: PeerAggregateKind,
    expected_revision: AggregateGeneration,
) -> AppResult<PeerAggregateReservation> {
    transition_within_transaction(
        transaction,
        game_id,
        operation_id,
        kind,
        expected_revision,
        PeerAggregateReservationState::Preparing,
        PeerAggregateReservationState::Prepared,
    )
}

/// Performs only the legal Prepared → Committed transition.
pub(crate) fn transition_to_committed_within_transaction(
    transaction: &Transaction<'_>,
    game_id: &GameId,
    operation_id: &str,
    kind: PeerAggregateKind,
    expected_revision: AggregateGeneration,
) -> AppResult<PeerAggregateReservation> {
    transition_within_transaction(
        transaction,
        game_id,
        operation_id,
        kind,
        expected_revision,
        PeerAggregateReservationState::Prepared,
        PeerAggregateReservationState::Committed,
    )
}

fn transition_within_transaction(
    transaction: &Transaction<'_>,
    game_id: &GameId,
    operation_id: &str,
    kind: PeerAggregateKind,
    expected_revision: AggregateGeneration,
    expected_state: PeerAggregateReservationState,
    next_state: PeerAggregateReservationState,
) -> AppResult<PeerAggregateReservation> {
    validate_operation_id(operation_id)?;
    let current =
        read_within_transaction(transaction, game_id, operation_id)?.ok_or_else(|| {
            AppError::storage_failed(format!(
                "peer aggregate reservation `{operation_id}` is missing"
            ))
        })?;
    if current.kind() != kind
        || current.binding() != kind.binding()
        || current.expected_revision() != expected_revision
        || current.state() != expected_state
    {
        return Err(AppError::storage_failed(format!(
            "peer aggregate reservation `{operation_id}` identity or state does not match transition"
        )));
    }
    let now_ms = sqlite_clock::now_ms(transaction)?;
    let updated = transaction
        .execute(
            "UPDATE peer_aggregate_reservations
             SET state = :next_state, updated_at = :now_ms
             WHERE game_id = :game_id AND operation_id = :operation_id
               AND aggregate_kind = :aggregate_kind
               AND pending_binding = :pending_binding
               AND state = :expected_state
               AND expected_revision = :expected_revision",
            named_params! {
                ":next_state": next_state.as_str(),
                ":now_ms": now_ms,
                ":game_id": game_id.as_str(),
                ":operation_id": operation_id,
                ":aggregate_kind": kind.as_str(),
                ":pending_binding": kind.binding().as_str(),
                ":expected_state": expected_state.as_str(),
                ":expected_revision": expected_revision.as_i64(),
            },
        )
        .map_err(storage_error)?;
    if updated != 1 {
        return Err(AppError::storage_failed(format!(
            "peer aggregate reservation `{operation_id}` changed before transition"
        )));
    }
    read_within_transaction(transaction, current.game_id(), operation_id)?.ok_or_else(|| {
        AppError::storage_failed(format!(
            "peer aggregate reservation `{operation_id}` disappeared after transition"
        ))
    })
}

/// Deletes one reservation only after its complete identity and expected state
/// match. Preparing and Prepared are rollback states; Committed is cleanup.
pub(crate) fn delete_within_transaction(
    transaction: &Transaction<'_>,
    game_id: &GameId,
    operation_id: &str,
    kind: PeerAggregateKind,
    expected_revision: AggregateGeneration,
    expected_state: PeerAggregateReservationState,
) -> AppResult<()> {
    validate_operation_id(operation_id)?;
    let current =
        read_within_transaction(transaction, game_id, operation_id)?.ok_or_else(|| {
            AppError::storage_failed(format!(
                "peer aggregate reservation `{operation_id}` is missing"
            ))
        })?;
    if current.kind() != kind
        || current.binding() != kind.binding()
        || current.expected_revision() != expected_revision
        || current.state() != expected_state
    {
        return Err(AppError::storage_failed(format!(
            "peer aggregate reservation `{operation_id}` identity or state does not match deletion"
        )));
    }
    let deleted = transaction
        .execute(
            "DELETE FROM peer_aggregate_reservations
             WHERE game_id = :game_id AND operation_id = :operation_id
               AND aggregate_kind = :aggregate_kind
               AND pending_binding = :pending_binding
               AND state = :state AND expected_revision = :expected_revision",
            named_params! {
                ":game_id": game_id.as_str(),
                ":operation_id": operation_id,
                ":aggregate_kind": kind.as_str(),
                ":pending_binding": kind.binding().as_str(),
                ":state": expected_state.as_str(),
                ":expected_revision": expected_revision.as_i64(),
            },
        )
        .map_err(storage_error)?;
    if deleted != 1 {
        return Err(AppError::storage_failed(format!(
            "peer aggregate reservation `{operation_id}` changed before deletion"
        )));
    }
    Ok(())
}

pub(crate) fn read_for_game_within_transaction(
    transaction: &Transaction<'_>,
    game_id: &GameId,
) -> AppResult<Option<PeerAggregateReservation>> {
    let sql = format!("{RESERVATION_SELECT}\n WHERE game_id = ?1");
    read_query(transaction, &sql, [game_id.as_str()])
}

fn read_query<P: rusqlite::Params>(
    transaction: &Transaction<'_>,
    sql: &str,
    params: P,
) -> AppResult<Option<PeerAggregateReservation>> {
    observe_query(transaction, sql, params)?.into_optional()
}

fn observe_query<P: rusqlite::Params>(
    transaction: &Transaction<'_>,
    sql: &str,
    params: P,
) -> AppResult<RowObservation<PeerAggregateReservation>> {
    let raw: Option<RawPeerAggregateReservation> = transaction
        .query_row(sql, params, |row| {
            Ok(RawPeerAggregateReservation {
                game_id: row.get(0)?,
                operation_id: row.get(1)?,
                kind: row.get(2)?,
                binding: row.get(3)?,
                state: row.get(4)?,
                expected_revision: row.get(5)?,
                created_at: row.get(6)?,
                updated_at: row.get(7)?,
            })
        })
        .optional()
        .map_err(storage_error)?;
    let Some(row) = raw else {
        return Ok(RowObservation::Missing);
    };
    Ok(match PeerAggregateReservation::from_row(row) {
        Ok(reservation) => RowObservation::Present(reservation),
        Err(error) => RowObservation::Invalid(error),
    })
}

fn read_game_revision(
    transaction: &Transaction<'_>,
    game_id: &GameId,
) -> AppResult<AggregateGeneration> {
    let revision: Option<i64> = transaction
        .query_row(
            "SELECT peer_aggregate_revision FROM games WHERE id = ?1",
            [game_id.as_str()],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage_error)?;
    let revision = revision.ok_or_else(|| {
        AppError::storage_failed(format!("game `{}` is missing", game_id.as_str()))
    })?;
    AggregateGeneration::from_persisted(revision)
}

fn pending_file_exists(transaction: &Transaction<'_>, game_id: &GameId) -> AppResult<bool> {
    transaction
        .query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM pending_file_mutations
                 WHERE game_id = ?1 AND state IN ('preparing', 'prepared')
             )",
            [game_id.as_str()],
            |row| row.get(0),
        )
        .map_err(storage_error)
}

fn pending_shared_game_exists(transaction: &Transaction<'_>, game_id: &GameId) -> AppResult<bool> {
    transaction
        .query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM pending_shared_vulkan_mutations
                 WHERE scope = 'game_shared' AND game_id = ?1
                   AND state IN ('preparing', 'prepared')
             )",
            [game_id.as_str()],
            |row| row.get(0),
        )
        .map_err(storage_error)
}

fn validate_operation_id(operation_id: &str) -> AppResult<()> {
    if operation_id.trim().is_empty()
        || operation_id.contains('\0')
        || operation_id.len() > crate::peer_runtime::aggregate::MAX_METADATA_BYTES
    {
        return Err(AppError::invalid_input(
            "aggregate operation identity is invalid",
        ));
    }
    Ok(())
}
