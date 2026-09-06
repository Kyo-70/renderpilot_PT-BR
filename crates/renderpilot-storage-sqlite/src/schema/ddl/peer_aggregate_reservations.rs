//! Canonical DDL for the game-scoped peer aggregate reservation fence.

use renderpilot_application::AppResult;
use rusqlite::Connection;

use crate::error::storage_context;

use super::common::MS_UNIXEPOCH_DEFAULT;

const TABLE_NAME: &str = "peer_aggregate_reservations";

/// Final-v19 baseline and migration DDL. FilePeer programs are deliberately
/// absent from this durable reservation contract.
pub(super) fn baseline_sql() -> String {
    format!(
        r"
CREATE TABLE IF NOT EXISTS {TABLE_NAME} (
    game_id           TEXT    PRIMARY KEY NOT NULL,
    operation_id      TEXT    UNIQUE NOT NULL,
    aggregate_kind    TEXT    NOT NULL,
    pending_binding   TEXT    NOT NULL,
    state             TEXT    NOT NULL,
    expected_revision INTEGER NOT NULL,
    created_at        INTEGER NOT NULL DEFAULT ({MS_UNIXEPOCH_DEFAULT}),
    updated_at        INTEGER NOT NULL DEFAULT ({MS_UNIXEPOCH_DEFAULT}),

    FOREIGN KEY (game_id) REFERENCES games(id) ON DELETE RESTRICT,
    CHECK (length(trim(game_id)) > 0),
    CHECK (length(trim(operation_id)) > 0),
    CHECK (instr(operation_id, char(0)) = 0),
    CHECK (aggregate_kind IN ('optiscaler_journal', 'shared_peer', 'metadata')),
    CHECK (pending_binding IN ('file', 'shared', 'metadata')),
    CHECK ((aggregate_kind = 'optiscaler_journal' AND pending_binding = 'file')
        OR (aggregate_kind = 'shared_peer' AND pending_binding = 'shared')
        OR (aggregate_kind = 'metadata' AND pending_binding = 'metadata')),
    CHECK (state IN ('preparing', 'prepared', 'committed')),
    CHECK (expected_revision >= 0),
    CHECK (created_at >= 0),
    CHECK (updated_at >= created_at)
) STRICT;

CREATE TRIGGER IF NOT EXISTS trg_peer_aggregate_reservations_freeze_identity
BEFORE UPDATE OF game_id, operation_id, aggregate_kind, pending_binding, expected_revision
ON {TABLE_NAME}
FOR EACH ROW
WHEN NEW.game_id IS NOT OLD.game_id
  OR NEW.operation_id IS NOT OLD.operation_id
  OR NEW.aggregate_kind IS NOT OLD.aggregate_kind
  OR NEW.pending_binding IS NOT OLD.pending_binding
  OR NEW.expected_revision IS NOT OLD.expected_revision
BEGIN
    SELECT RAISE(ABORT, 'peer aggregate reservation identity is immutable');
END;

CREATE TRIGGER IF NOT EXISTS trg_peer_aggregate_reservations_touch_updated_at
AFTER UPDATE ON {TABLE_NAME}
FOR EACH ROW
WHEN NEW.updated_at = OLD.updated_at
BEGIN
    UPDATE {TABLE_NAME}
       SET updated_at = max(CAST(unixepoch('subsec') * 1000 AS INTEGER), OLD.updated_at + 1)
     WHERE game_id = NEW.game_id;
END;

CREATE TRIGGER IF NOT EXISTS trg_peer_aggregate_reservations_restrict_state_transition
BEFORE UPDATE OF state ON {TABLE_NAME}
FOR EACH ROW
WHEN NEW.state IS NOT OLD.state
 AND NOT (
    (OLD.state = 'preparing' AND NEW.state = 'prepared')
    OR (OLD.state = 'prepared' AND NEW.state = 'committed')
 )
BEGIN
    SELECT RAISE(ABORT, 'peer aggregate reservation state transition is invalid');
END;

CREATE TRIGGER IF NOT EXISTS trg_games_restrict_peer_aggregate_delete
BEFORE DELETE ON games
FOR EACH ROW
WHEN EXISTS (
        SELECT 1 FROM {TABLE_NAME} AS r WHERE r.game_id = OLD.id
    )
    OR EXISTS (
        SELECT 1 FROM pending_file_mutations AS p
         WHERE p.game_id = OLD.id AND p.aggregate_kind IS NOT NULL
    )
    OR EXISTS (
        SELECT 1 FROM pending_shared_vulkan_mutations AS p
         WHERE p.scope = 'game_shared'
           AND p.game_id = OLD.id
           AND p.aggregate_kind = 'shared_peer'
    )
BEGIN
    SELECT RAISE(ABORT, 'game has an unresolved peer aggregate mutation');
END;
",
    )
}

pub(in crate::schema) fn apply(connection: &Connection) -> AppResult<()> {
    connection
        .execute_batch(&baseline_sql())
        .map_err(|error| storage_context("could not add peer aggregate reservation schema", error))
}
