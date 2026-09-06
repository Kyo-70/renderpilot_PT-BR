//! Add the typed OptiScaler state and neutral proxy-topology aggregates.

use renderpilot_application::AppResult;
use rusqlite::{Connection, OptionalExtension};

use crate::error::storage_context;

use super::super::{
    ddl::{
        optiscaler_state, peer_aggregate_reservations, pending_file_mutations,
        pending_shared_vulkan_mutations,
    },
    version,
};

pub(super) const SOURCE_VERSION: i32 = 18;
pub(super) const TARGET_VERSION: i32 = 19;

pub(super) fn apply(connection: &Connection) -> AppResult<()> {
    // The current-schema test fixtures may deliberately remove one of the
    // tables before replaying this edge.  Recreate this cross-table guard
    // after all three pending tables have reached their final shape.
    connection
        .execute_batch("DROP TRIGGER IF EXISTS trg_games_restrict_peer_aggregate_delete;")
        .map_err(|error| {
            storage_context("could not stage the game aggregate delete guard", error)
        })?;
    if !has_game_aggregate_revision(connection)? {
        connection
            .execute_batch(
                "ALTER TABLE games
                     ADD COLUMN peer_aggregate_revision INTEGER NOT NULL DEFAULT 0
                     CHECK (peer_aggregate_revision >= 0);",
            )
            .map_err(|error| storage_context("could not add v19 game aggregate revision", error))?;
    }
    pending_file_mutations::apply_v19_columns(connection)?;
    pending_shared_vulkan_mutations::apply_v19_columns(connection)?;
    peer_aggregate_reservations::apply(connection)?;
    optiscaler_state::apply(connection)?;
    version::write(connection, TARGET_VERSION)
}

fn has_game_aggregate_revision(connection: &Connection) -> AppResult<bool> {
    connection
        .query_row(
            "SELECT 1 FROM pragma_table_info('games')
              WHERE name = 'peer_aggregate_revision'
              LIMIT 1",
            [],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .map(|value| value.is_some())
        .map_err(|error| storage_context("could not inspect game aggregate columns", error))
}
