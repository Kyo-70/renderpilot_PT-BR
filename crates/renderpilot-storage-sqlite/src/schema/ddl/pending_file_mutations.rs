//! Canonical DDL for `pending_file_mutations`.
//!
//! The v19 contract keeps only the OptiScaler aggregate fence; FilePeer
//! programs are not represented in either fresh catalogs or the v18 → v19
//! upgrade.

use renderpilot_application::AppResult;
use rusqlite::{Connection, OptionalExtension};

use crate::error::storage_context;

use super::super::objects::{SchemaObjectKind, object_exists};
use super::common::MS_UNIXEPOCH_DEFAULT;

const TABLE_NAME: &str = "pending_file_mutations";

fn current_table_body() -> String {
    format!(
        r"
    id             TEXT    PRIMARY KEY NOT NULL,
    game_id        TEXT    NOT NULL,
    feature        TEXT    NOT NULL,
    subject_id     TEXT,
    state          TEXT    NOT NULL,
    manifest_json  TEXT    NOT NULL,
    created_at     INTEGER NOT NULL DEFAULT ({MS_UNIXEPOCH_DEFAULT}),
    updated_at     INTEGER NOT NULL DEFAULT ({MS_UNIXEPOCH_DEFAULT}),
    aggregate_kind TEXT,
    aggregate_revision INTEGER,
    CHECK (length(trim(id)) > 0),
    CHECK (length(trim(game_id)) > 0),
    CHECK (length(trim(feature)) > 0),
    CHECK (subject_id IS NULL OR length(trim(subject_id)) > 0),
    CHECK (state IN ('preparing', 'prepared', 'committed')),
    CHECK (json_valid(manifest_json)),
    CHECK (json_type(manifest_json) = 'object'),
    CHECK (
        (aggregate_kind IS NULL AND aggregate_revision IS NULL)
        OR (aggregate_kind = 'optiscaler_journal'
            AND aggregate_revision IS NOT NULL
            AND aggregate_revision >= 0)
    ),
    CHECK (created_at >= 0),
    CHECK (updated_at >= created_at)
",
    )
}

fn released_v10_table_body() -> String {
    format!(
        r"
    id             TEXT    PRIMARY KEY NOT NULL,
    game_id        TEXT    NOT NULL,
    feature        TEXT    NOT NULL,
    subject_id     TEXT,
    state          TEXT    NOT NULL,
    manifest_json  TEXT    NOT NULL,
    created_at     INTEGER NOT NULL DEFAULT ({MS_UNIXEPOCH_DEFAULT}),
    updated_at     INTEGER NOT NULL DEFAULT ({MS_UNIXEPOCH_DEFAULT}),
    CHECK (length(trim(id)) > 0),
    CHECK (length(trim(game_id)) > 0),
    CHECK (length(trim(feature)) > 0),
    CHECK (subject_id IS NULL OR length(trim(subject_id)) > 0),
    CHECK (state IN ('preparing', 'prepared', 'committed')),
    CHECK (json_valid(manifest_json)),
    CHECK (json_type(manifest_json) = 'object'),
    CHECK (created_at >= 0),
    CHECK (updated_at >= created_at)
",
    )
}

fn create_table_sql(table_name: &str, if_not_exists: bool) -> String {
    let if_clause = if if_not_exists { "IF NOT EXISTS " } else { "" };
    format!(
        "CREATE TABLE {if_clause}{table_name} ({}) STRICT",
        released_v10_table_body()
    )
}

fn create_current_table_sql(table_name: &str, if_not_exists: bool) -> String {
    let if_clause = if if_not_exists { "IF NOT EXISTS " } else { "" };
    format!(
        "CREATE TABLE {if_clause}{table_name} ({}) STRICT",
        current_table_body()
    )
}

const CREATE_INDEX_SQL: &str = r"
CREATE INDEX IF NOT EXISTS idx_pending_file_mutations_game_id
    ON pending_file_mutations(game_id)
";

const CURRENT_TRIGGERS_SQL: &str = r"
CREATE TRIGGER IF NOT EXISTS trg_pending_file_mutations_freeze_aggregate_binding
BEFORE UPDATE OF aggregate_kind, aggregate_revision ON pending_file_mutations
FOR EACH ROW
WHEN NEW.aggregate_kind IS NOT OLD.aggregate_kind
  OR NEW.aggregate_revision IS NOT OLD.aggregate_revision
BEGIN
    SELECT RAISE(ABORT, 'pending file aggregate binding is immutable');
END;

CREATE TRIGGER IF NOT EXISTS trg_pending_file_mutations_restrict_state_transition
BEFORE UPDATE OF state ON pending_file_mutations
FOR EACH ROW
WHEN NEW.state IS NOT OLD.state
 AND NOT (
    (OLD.state = 'preparing' AND NEW.state = 'prepared')
    OR (OLD.state = 'prepared' AND NEW.state = 'committed')
 )
BEGIN
    SELECT RAISE(ABORT, 'pending file mutation state transition is invalid');
END;
";

pub(super) fn baseline_sql() -> String {
    format!(
        "{table};\n{index};\n{triggers}",
        table = create_current_table_sql(TABLE_NAME, true),
        index = CREATE_INDEX_SQL.trim(),
        triggers = CURRENT_TRIGGERS_SQL.trim(),
    )
}

pub(in crate::schema) fn create_for_released_v9_to_v10(connection: &Connection) -> AppResult<()> {
    connection
        .execute_batch(&format!(
            "{};\n{};",
            create_table_sql(TABLE_NAME, true),
            CREATE_INDEX_SQL
        ))
        .map_err(|error| storage_context("could not create pending_file_mutations", error))
}

pub(in crate::schema) fn allows_preparing(connection: &Connection) -> AppResult<bool> {
    if !object_exists(connection, SchemaObjectKind::Table, TABLE_NAME)? {
        return Ok(false);
    }
    connection
        .execute_batch("SAVEPOINT probe_pending_preparing")
        .map_err(|error| storage_context("could not open pending_file_mutations probe", error))?;
    let probe = connection.execute(
        "INSERT INTO pending_file_mutations (id, game_id, feature, subject_id, state, manifest_json)
         VALUES ('__schema_probe_preparing__', 'probe:game', 'schema_probe', NULL,
                 'preparing', '{\"snapshots\":[]}')",
        [],
    );
    let _ = connection.execute_batch("ROLLBACK TO probe_pending_preparing");
    let _ = connection.execute_batch("RELEASE probe_pending_preparing");
    match probe {
        Ok(_) => Ok(true),
        Err(rusqlite::Error::SqliteFailure(error, _))
            if error.code == rusqlite::ErrorCode::ConstraintViolation =>
        {
            Ok(false)
        }
        Err(error) => Err(storage_context(
            "could not probe pending_file_mutations preparing state",
            error,
        )),
    }
}

pub(in crate::schema) fn allows_preparing_observational(
    connection: &Connection,
) -> AppResult<bool> {
    let Some(sql) = connection
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = ?1",
            [TABLE_NAME],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()
        .map_err(|error| storage_context("could not read pending_file_mutations DDL", error))?
        .flatten()
    else {
        return Ok(false);
    };
    let normalized = sql
        .chars()
        .filter(|character| !character.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect::<String>();
    Ok(
        normalized.contains("check(statein('preparing','prepared','committed'))")
            && normalized.contains("aggregate_kind='optiscaler_journal'"),
    )
}

pub(in crate::schema) fn apply_v19_columns(connection: &Connection) -> AppResult<()> {
    if !has_released_columns(connection)? {
        return Ok(());
    }
    let has_kind = has_column(connection, "aggregate_kind")?;
    let has_revision = has_column(connection, "aggregate_revision")?;
    match (has_kind, has_revision) {
        (true, true) => connection
            .execute_batch(&format!("{CREATE_INDEX_SQL};\n{CURRENT_TRIGGERS_SQL}"))
            .map_err(|error| {
                storage_context("could not add v19 pending file aggregate triggers", error)
            }),
        (false, false) => rebuild_as_v19(connection),
        _ => Err(renderpilot_application::AppError::storage_failed(
            "pending_file_mutations has a partial v19 aggregate column set",
        )),
    }
}

fn has_column(connection: &Connection, column: &str) -> AppResult<bool> {
    connection
        .query_row(
            "SELECT 1 FROM pragma_table_info(?1) WHERE name = ?2 LIMIT 1",
            [TABLE_NAME, column],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .map(|value| value.is_some())
        .map_err(|error| storage_context("could not inspect pending file columns", error))
}

fn has_released_columns(connection: &Connection) -> AppResult<bool> {
    [
        "id",
        "game_id",
        "feature",
        "subject_id",
        "state",
        "manifest_json",
        "created_at",
        "updated_at",
    ]
    .into_iter()
    .try_fold(true, |ready, column| {
        has_column(connection, column).map(|present| ready && present)
    })
}

fn rebuild_as_v19(connection: &Connection) -> AppResult<()> {
    connection
        .execute_batch("ALTER TABLE pending_file_mutations RENAME TO pending_file_mutations_v18;")
        .map_err(|error| storage_context("could not stage released pending file table", error))?;
    connection
        .execute_batch(&format!(
            "{};
         INSERT INTO pending_file_mutations
             (id, game_id, feature, subject_id, state, manifest_json, created_at, updated_at)
         SELECT id, game_id, feature, subject_id, state, manifest_json, created_at, updated_at
           FROM pending_file_mutations_v18;
         DROP TABLE pending_file_mutations_v18;
         {CREATE_INDEX_SQL};
         {CURRENT_TRIGGERS_SQL}",
            create_current_table_sql(TABLE_NAME, false),
        ))
        .map_err(|error| storage_context("could not rebuild pending file table for v19", error))
}
