//! Persistence for installed add-on records (initially RenoDX).
//!
//! One row per game records everything needed to reverse an install: the files
//! RenderPilot created and the pre-existing files it backed up. This is the
//! source of truth for uninstall, so the table intentionally has no foreign
//! key to `games` and survives catalog pruning/rescans.

use renderpilot_application::AppResult;
use renderpilot_application::{AppError, InstalledAddonRepository};
#[cfg(test)]
use renderpilot_domain::TrackedSourceRole;
use renderpilot_domain::{
    AddonKind, EngineConfigJournal, GameId, InstalledAddon, InstalledAddonHostKind,
    InstalledAddonParts, ManagedAddonFile, PathRef, TrackedSource,
};
#[cfg(test)]
use renderpilot_domain::{EngineConfigContribution, EngineConfigReceipt, EngineConfigTransition};
use rusqlite::{OptionalExtension, Row, Transaction, named_params};

use crate::error::{invalid_row, storage_error};
use crate::repositories::observation::RowObservation;
use crate::{mapping, sqlite_clock};

use super::{SqliteStorage, peer_aggregate_reservations};

impl SqliteStorage {
    /// Reads the exact persisted Engine.ini journal token.  The raw token is
    /// intentionally exposed for the compare-and-swap boundary; callers must
    /// not deserialize and reserialize it between the read and CAS.
    pub fn engine_config_journal_token(&self, game_id: &GameId) -> AppResult<Option<String>> {
        self.with_connection(|connection| {
            let row = connection
                .query_row(
                    "SELECT kind, engine_config_journal_json
                       FROM installed_addons WHERE game_id = :game_id",
                    named_params! { ":game_id": game_id.as_str() },
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
                )
                .optional()
                .map_err(storage_error)?;
            let Some((kind, Some(raw))) = row else {
                return Ok(None);
            };
            let kind: AddonKind = mapping::enum_from_text(&kind)?;
            let journal: EngineConfigJournal = mapping::deserialize_json(&raw)?;
            if journal.is_empty() {
                return Err(AppError::storage_failed(
                    "empty Engine.ini journal must be stored as SQL NULL",
                ));
            }
            journal
                .validate_for_kind(kind)
                .map_err(|error| AppError::storage_failed(error.to_string()))?;
            if mapping::serialize_json(&journal)? != raw {
                return Err(AppError::storage_failed(
                    "Engine.ini journal token is not canonical",
                ));
            }
            Ok(Some(raw))
        })
    }

    /// Atomically replaces the Engine.ini journal only when the row still has
    /// the exact expected raw token.  Ordinary add-on upserts deliberately do
    /// not include this column in their update projection.
    pub fn compare_and_swap_engine_config_journal(
        &self,
        game_id: &GameId,
        kind: AddonKind,
        expected_raw: Option<&str>,
        journal: Option<&EngineConfigJournal>,
    ) -> AppResult<bool> {
        if let Some(value) = journal {
            value
                .validate_for_kind(kind)
                .map_err(|error| AppError::invalid_input(error.to_string()))?;
        }
        let replacement = journal
            .filter(|value| !value.is_empty())
            .map(mapping::serialize_json)
            .transpose()?;
        self.with_transaction(|transaction| {
            let changed = transaction
                .execute(
                    "UPDATE installed_addons
                        SET engine_config_journal_json = :replacement
                      WHERE game_id = :game_id
                        AND kind = :kind
                        AND ((engine_config_journal_json IS NULL AND :expected IS NULL)
                             OR engine_config_journal_json = :expected)",
                    named_params! {
                        ":replacement": replacement,
                        ":game_id": game_id.as_str(),
                        ":kind": kind.as_str(),
                        ":expected": expected_raw,
                    },
                )
                .map_err(storage_error)?;
            Ok(changed == 1)
        })
    }
}

const EXISTING_KIND_SQL: &str = "SELECT kind FROM installed_addons WHERE game_id = :game_id";

const UPSERT_SQL: &str = "
    INSERT INTO installed_addons
        (game_id, kind, addon_file, addon_version,
         created_files_json, backed_up_files_json, managed_files_json, tracked_sources_json,
         host_kind, reshade_channel, registered_exe_path, renodx_config_receipt_json,
         engine_config_journal_json,
         created_at, updated_at)
    VALUES
        (:game_id, :kind, :addon_file, :addon_version,
         :created_files, :backed_up_files, :managed_files, :tracked_sources,
         :host_kind, :reshade_channel, :registered_exe_path, :renodx_config_receipt,
         NULL,
         :now_ms, :now_ms)
    ON CONFLICT(game_id) DO UPDATE SET
        kind                 = excluded.kind,
        addon_file           = excluded.addon_file,
        addon_version        = excluded.addon_version,
        created_files_json   = excluded.created_files_json,
        backed_up_files_json = excluded.backed_up_files_json,
        managed_files_json   = excluded.managed_files_json,
        tracked_sources_json = excluded.tracked_sources_json,
        host_kind            = excluded.host_kind,
        reshade_channel      = excluded.reshade_channel,
        registered_exe_path  = excluded.registered_exe_path,
        renodx_config_receipt_json = excluded.renodx_config_receipt_json,
        updated_at           = excluded.updated_at
";

const GET_SQL: &str = "
    SELECT game_id, kind, addon_file, addon_version,
           created_files_json, backed_up_files_json, managed_files_json, tracked_sources_json,
           host_kind, reshade_channel, registered_exe_path, renodx_config_receipt_json,
           engine_config_journal_json,
           created_at, updated_at
    FROM installed_addons
    WHERE game_id = :game_id
";

const LIST_SQL: &str = "
    SELECT game_id, kind, addon_file, addon_version,
           created_files_json, backed_up_files_json, managed_files_json, tracked_sources_json,
           host_kind, reshade_channel, registered_exe_path, renodx_config_receipt_json,
           engine_config_journal_json,
           created_at, updated_at
    FROM installed_addons
    ORDER BY game_id
";

const DELETE_SQL: &str = "DELETE FROM installed_addons WHERE game_id = :game_id";

impl InstalledAddonRepository for SqliteStorage {
    fn upsert_installed_addon(&self, addon: &InstalledAddon) -> AppResult<()> {
        self.with_transaction(|transaction| {
            ensure_independent_peer_mutation_allowed(transaction, addon.game_id(), addon.kind())?;
            upsert_within_transaction(transaction, addon)
        })
    }

    fn get_installed_addon(&self, game_id: &GameId) -> AppResult<Option<InstalledAddon>> {
        self.with_connection(|connection| {
            observe_on_connection(connection, game_id)?.into_optional()
        })
    }

    fn list_installed_addons(&self) -> AppResult<Vec<InstalledAddon>> {
        self.query_list(LIST_SQL, [], |row| {
            Ok(decode_installed_addon(raw_installed_addon_from_row(row)?))
        })
    }

    fn delete_installed_addon(&self, game_id: &GameId, kind: AddonKind) -> AppResult<()> {
        self.with_transaction(|transaction| {
            ensure_independent_peer_mutation_allowed(transaction, game_id, kind)?;
            delete_within_transaction(transaction, game_id, kind)
        })
    }
}

/// Public peer receipt writes cannot participate in OptiScaler's closed
/// topology aggregate. Internal exact aggregate commits intentionally call the
/// within-transaction helpers directly after validating their full transition.
pub(super) fn ensure_independent_peer_mutation_allowed(
    transaction: &Transaction<'_>,
    game_id: &GameId,
    kind: AddonKind,
) -> AppResult<()> {
    peer_aggregate_reservations::ensure_no_peer_aggregate_reservation_within_transaction(
        transaction,
        game_id,
    )?;
    if matches!(kind, AddonKind::RenoDx | AddonKind::Luma)
        && super::proxy_topologies::get_within_transaction(transaction, game_id)?.is_some()
    {
        return Err(AppError::peer_topology_conflict(kind));
    }
    Ok(())
}

pub(crate) fn get_within_transaction(
    transaction: &Transaction<'_>,
    game_id: &GameId,
) -> AppResult<Option<InstalledAddon>> {
    observe_within_transaction(transaction, game_id)?.into_optional()
}

pub(crate) fn observe_within_transaction(
    transaction: &Transaction<'_>,
    game_id: &GameId,
) -> AppResult<RowObservation<InstalledAddon>> {
    observe_raw(
        transaction
            .query_row(
                GET_SQL,
                named_params! { ":game_id": game_id.as_str() },
                raw_installed_addon_from_row,
            )
            .optional(),
    )
}

pub(crate) fn upsert_within_transaction(
    transaction: &Transaction<'_>,
    addon: &InstalledAddon,
) -> AppResult<()> {
    if addon.kind() == AddonKind::OptiScaler {
        return Err(AppError::invalid_input(
            "OptiScaler lifecycle state must be committed through its aggregate repository",
        ));
    }
    let existing_kind: Option<String> = transaction
        .prepare_cached(EXISTING_KIND_SQL)
        .map_err(storage_error)?
        .query_row(
            named_params! { ":game_id": addon.game_id().as_str() },
            |row| row.get("kind"),
        )
        .optional()
        .map_err(storage_error)?;
    let new_kind = addon.kind().as_str();
    if let Some(existing_kind) = &existing_kind
        && existing_kind != new_kind
    {
        return Err(AppError::invalid_input(format!(
            "refusing to overwrite a '{existing_kind}' install record with a \
             '{new_kind}' one for {}; uninstall it first",
            addon.game_id()
        )));
    }

    let now_ms = sqlite_clock::now_ms(transaction)?;
    let host_kind = addon
        .host_kind()
        .map(|kind| mapping::enum_to_text(&kind))
        .transpose()?;
    transaction
        .prepare_cached(UPSERT_SQL)
        .map_err(storage_error)?
        .execute(named_params! {
            ":game_id": addon.game_id().as_str(),
            ":kind": mapping::enum_to_text(&addon.kind())?,
            ":addon_file": addon.addon_file().as_str(),
            ":addon_version": addon.addon_version(),
            ":created_files": mapping::serialize_json(addon.created_files())?,
            ":backed_up_files": mapping::serialize_json(addon.backed_up_files())?,
            ":managed_files": mapping::serialize_json(addon.managed_files())?,
            ":tracked_sources": mapping::serialize_json(addon.tracked_sources())?,
            ":host_kind": host_kind.as_deref(),
            ":reshade_channel": addon.reshade_channel(),
            ":registered_exe_path": addon.registered_exe_path().map(PathRef::as_str),
            ":renodx_config_receipt": addon
                .renodx_config_receipt()
                .map(mapping::serialize_json)
                .transpose()?,
            ":now_ms": now_ms,
        })
        .map_err(storage_error)?;
    Ok(())
}

pub(crate) fn delete_within_transaction(
    transaction: &Transaction<'_>,
    game_id: &GameId,
    kind: AddonKind,
) -> AppResult<()> {
    if kind == AddonKind::OptiScaler {
        return Err(AppError::invalid_input(
            "OptiScaler lifecycle state must be removed through its aggregate repository",
        ));
    }
    let existing_kind: Option<String> = transaction
        .prepare_cached(EXISTING_KIND_SQL)
        .map_err(storage_error)?
        .query_row(named_params! { ":game_id": game_id.as_str() }, |row| {
            row.get("kind")
        })
        .optional()
        .map_err(storage_error)?;
    let expected_kind = kind.as_str();
    if let Some(existing_kind) = &existing_kind
        && existing_kind != expected_kind
    {
        return Err(AppError::invalid_input(format!(
            "refusing to delete a '{existing_kind}' install record as a \
             '{expected_kind}' one for {game_id}; uninstall it with the correct kind"
        )));
    }

    // Engine.ini ownership is released through its explicit CAS lifecycle
    // before the add-on row may disappear.  A direct metadata delete would
    // otherwise strand the on-disk receipt (or an in-flight transition) with
    // no durable owner capable of recovering it.
    let has_engine_config_journal: bool = transaction
        .query_row(
            "SELECT engine_config_journal_json IS NOT NULL
               FROM installed_addons WHERE game_id = :game_id",
            named_params! { ":game_id": game_id.as_str() },
            |row| row.get(0),
        )
        .optional()
        .map_err(storage_error)?
        .unwrap_or(false);
    if has_engine_config_journal {
        return Err(AppError::invalid_input(
            "cannot delete an add-on while its Engine.ini journal is present; release it first",
        ));
    }

    transaction
        .prepare_cached(DELETE_SQL)
        .map_err(storage_error)?
        .execute(named_params! {
            ":game_id": game_id.as_str(),
        })
        .map_err(storage_error)?;
    Ok(())
}

fn observe_on_connection(
    connection: &rusqlite::Connection,
    game_id: &GameId,
) -> AppResult<RowObservation<InstalledAddon>> {
    let mut statement = connection.prepare_cached(GET_SQL).map_err(storage_error)?;
    observe_raw(
        statement
            .query_row(
                named_params! { ":game_id": game_id.as_str() },
                raw_installed_addon_from_row,
            )
            .optional(),
    )
}

fn observe_raw(
    result: rusqlite::Result<Option<RawInstalledAddon>>,
) -> AppResult<RowObservation<InstalledAddon>> {
    match result.map_err(storage_error)? {
        None => Ok(RowObservation::Missing),
        Some(raw) => Ok(match decode_installed_addon(raw) {
            Ok(addon) => RowObservation::Present(addon),
            Err(error) => RowObservation::Invalid(error),
        }),
    }
}

#[derive(Debug)]
struct RawInstalledAddon {
    game_id: String,
    kind: String,
    addon_file: String,
    addon_version: Option<String>,
    created_files_json: String,
    backed_up_files_json: String,
    managed_files_json: String,
    tracked_sources_json: String,
    host_kind: Option<String>,
    reshade_channel: Option<String>,
    registered_exe_path: Option<String>,
    renodx_config_receipt_json: Option<String>,
    engine_config_journal_json: Option<String>,
    created_at: i64,
    updated_at: i64,
}

fn raw_installed_addon_from_row(row: &Row<'_>) -> rusqlite::Result<RawInstalledAddon> {
    Ok(RawInstalledAddon {
        game_id: row.get("game_id")?,
        kind: row.get("kind")?,
        addon_file: row.get("addon_file")?,
        addon_version: row.get("addon_version")?,
        created_files_json: row.get("created_files_json")?,
        backed_up_files_json: row.get("backed_up_files_json")?,
        managed_files_json: row.get("managed_files_json")?,
        tracked_sources_json: row.get("tracked_sources_json")?,
        host_kind: row.get("host_kind")?,
        reshade_channel: row.get("reshade_channel")?,
        registered_exe_path: row.get("registered_exe_path")?,
        renodx_config_receipt_json: row.get("renodx_config_receipt_json")?,
        engine_config_journal_json: row.get("engine_config_journal_json")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

/// Decodes an extracted row into an [`InstalledAddon`]. All SQLite column
/// extraction happens in [`raw_installed_addon_from_row`], so errors here are
/// typed row corruption rather than operational query failures.
fn decode_installed_addon(raw: RawInstalledAddon) -> AppResult<InstalledAddon> {
    let game_id = GameId::new(raw.game_id).map_err(invalid_row)?;
    let kind: AddonKind = mapping::enum_from_text(&raw.kind)?;
    if kind == AddonKind::OptiScaler {
        return Err(invalid_row(
            "OptiScaler lifecycle state must not be stored in installed_addons",
        ));
    }
    let addon_file = PathRef::new(raw.addon_file).map_err(invalid_row)?;
    let created_files: Vec<PathRef> = mapping::deserialize_json(&raw.created_files_json)?;
    let backed_up_files: Vec<PathRef> = mapping::deserialize_json(&raw.backed_up_files_json)?;
    let managed_files: Vec<ManagedAddonFile> = mapping::deserialize_json(&raw.managed_files_json)?;
    let tracked_sources: Vec<TrackedSource> = mapping::deserialize_json(&raw.tracked_sources_json)?;
    let host_kind = raw
        .host_kind
        .map(|value| mapping::enum_from_text::<InstalledAddonHostKind>(&value))
        .transpose()?;
    let registered_exe_path: Option<PathRef> = raw
        .registered_exe_path
        .map(PathRef::new)
        .transpose()
        .map_err(invalid_row)?;
    let renodx_config_receipt = raw
        .renodx_config_receipt_json
        .as_deref()
        .map(mapping::deserialize_json)
        .transpose()?;
    let engine_config_journal = raw
        .engine_config_journal_json
        .as_deref()
        .map(|value| {
            let journal: EngineConfigJournal = mapping::deserialize_json(value)?;
            if journal.is_empty() {
                return Err(AppError::storage_failed(
                    "empty Engine.ini journal must be stored as SQL NULL",
                ));
            }
            if mapping::serialize_json(&journal)? != value {
                return Err(AppError::storage_failed(
                    "Engine.ini journal token is not canonical",
                ));
            }
            Ok(journal)
        })
        .transpose()?;

    let mut record = InstalledAddon::from_parts_with_managed(InstalledAddonParts {
        game_id,
        kind,
        addon_file,
        addon_version: raw.addon_version,
        created_files,
        backed_up_files,
        managed_files,
        tracked_sources,
        renodx_config_receipt,
        engine_config_journal,
    })
    .map_err(invalid_row)?
    .ok_or_else(|| invalid_row("created_files must contain addon_file"))?
    .with_timestamps(Some(raw.created_at), Some(raw.updated_at));

    if let Some(host_kind) = host_kind {
        record = record.with_host_kind(host_kind);
    }
    if let Some(channel) = raw.reshade_channel {
        record = record.with_reshade_channel(channel);
    }
    if let Some(path) = registered_exe_path {
        record = record.with_registered_exe_path(path);
    }

    Ok(record)
}

#[cfg(test)]
mod tests;
