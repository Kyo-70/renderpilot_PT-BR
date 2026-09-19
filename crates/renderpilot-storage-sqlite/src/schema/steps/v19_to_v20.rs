//! Add the optional RenoDX receipt and shared Engine.ini journal columns.

use renderpilot_application::AppResult;
use rusqlite::Connection;

use super::util::{ensure_column, table_exists};
use super::version;

pub(super) const SOURCE_VERSION: i32 = 19;
pub(super) const TARGET_VERSION: i32 = 20;

pub(super) fn apply(connection: &Connection) -> AppResult<()> {
    if !table_exists(connection, "installed_addons")? {
        // A stamped but malformed v19 catalog is allowed to continue through
        // the linear chain; the post-step contract validator will select the
        // normal transactional rebuild. The migration itself must not invent
        // an installed_addons table or otherwise synthesize data.
        return version::write(connection, TARGET_VERSION);
    }

    ensure_column(
        connection,
        "installed_addons",
        "renodx_config_receipt_json",
        "ALTER TABLE installed_addons
         ADD COLUMN renodx_config_receipt_json TEXT
         CHECK (renodx_config_receipt_json IS NULL OR json_valid(renodx_config_receipt_json))
         CHECK (renodx_config_receipt_json IS NULL OR json_type(renodx_config_receipt_json) = 'object');",
    )?;
    ensure_column(
        connection,
        "installed_addons",
        "engine_config_journal_json",
        "ALTER TABLE installed_addons
         ADD COLUMN engine_config_journal_json TEXT
         CHECK (engine_config_journal_json IS NULL OR json_valid(engine_config_journal_json))
         CHECK (engine_config_journal_json IS NULL OR json_type(engine_config_journal_json) = 'object');",
    )?;

    version::write(connection, TARGET_VERSION)
}
