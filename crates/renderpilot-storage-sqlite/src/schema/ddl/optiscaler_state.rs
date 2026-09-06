//! Current-v19 OptiScaler and proxy-topology schema facade.

use renderpilot_application::AppResult;
use rusqlite::Connection;

use crate::error::storage_context;

const CURRENT_V19_SQL: &str = include_str!("optiscaler_state/current_v19.sql");

/// Current-v19 DDL fragment used by both fresh catalogs and the v18→v19 edge.
pub(super) const fn baseline_sql() -> &'static str {
    CURRENT_V19_SQL
}

/// Applies the released v19 OptiScaler and proxy-topology schema.
pub(in crate::schema) fn apply(connection: &Connection) -> AppResult<()> {
    connection
        .execute_batch(CURRENT_V19_SQL)
        .map_err(|error| storage_context("could not create v19 OptiScaler state tables", error))
}
