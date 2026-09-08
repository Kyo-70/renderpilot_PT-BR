use renderpilot_application::{AppError, AppResult};
use renderpilot_domain::GameId;
use rusqlite::Row;

use crate::error::storage_error;

use super::super::model::{
    BeginFileMutationPreparation, PendingFileMutationRow, PendingFileMutationState,
};

pub(super) fn validate_begin_preparation(begin: &BeginFileMutationPreparation) -> AppResult<()> {
    for (field, value) in [
        ("id", begin.id.as_str()),
        ("feature", begin.feature.as_str()),
    ] {
        if value.trim().is_empty() || value.contains('\0') {
            return Err(AppError::storage_failed(format!(
                "pending file mutation {field} is invalid"
            )));
        }
    }
    if begin
        .subject_id
        .as_deref()
        .is_some_and(|value| value.trim().is_empty() || value.contains('\0'))
    {
        return Err(AppError::storage_failed(
            "pending file mutation subject id is invalid",
        ));
    }
    if is_optiscaler_feature(&begin.feature) {
        return super::super::commit::validate_optiscaler_journal_for_begin(
            &begin.initial_manifest_json,
        );
    }
    let manifest: serde_json::Value =
        serde_json::from_str(&begin.initial_manifest_json).map_err(|error| {
            AppError::storage_failed(format!("invalid initial file mutation manifest: {error}"))
        })?;
    if !manifest.is_object() {
        return Err(AppError::storage_failed(
            "initial file mutation manifest must be a JSON object",
        ));
    }
    Ok(())
}

pub(super) fn row_to_pending_mutation(row: &Row<'_>) -> AppResult<PendingFileMutationRow> {
    Ok(PendingFileMutationRow {
        id: row.get("id").map_err(storage_error)?,
        game_id: GameId::new(row.get::<_, String>("game_id").map_err(storage_error)?)
            .map_err(|error| AppError::storage_failed(error.to_string()))?,
        feature: row.get("feature").map_err(storage_error)?,
        subject_id: row.get("subject_id").map_err(storage_error)?,
        state: row
            .get::<_, String>("state")
            .map_err(storage_error)?
            .parse::<PendingFileMutationState>()?,
        manifest_json: row.get("manifest_json").map_err(storage_error)?,
    })
}

/// Storage validates only the durable boundary shape. Orchestration remains the
/// authority for roots and snapshot locations, but malformed JSON must never
/// transition a row to Prepared or invalidate a catalog.
pub(super) fn validate_prepared_manifest_for_feature(
    feature: &str,
    manifest_json: &str,
) -> AppResult<()> {
    if matches!(
        feature,
        renderpilot_domain::mutation_features::OPTISCALER_INSTALL
            | renderpilot_domain::mutation_features::OPTISCALER_UPDATE
            | renderpilot_domain::mutation_features::OPTISCALER_RELOCATE
            | renderpilot_domain::mutation_features::OPTISCALER_UNINSTALL
    ) {
        return super::super::commit::validate_optiscaler_journal_for_prepared(manifest_json);
    }
    let manifest: serde_json::Value = serde_json::from_str(manifest_json).map_err(|error| {
        AppError::storage_failed(format!("invalid pending file mutation manifest: {error}"))
    })?;
    let snapshots = manifest
        .as_object()
        .and_then(|object| object.get("snapshots"))
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| {
            AppError::storage_failed(
                "pending file mutation manifest must be an object with snapshots array",
            )
        })?;
    for snapshot in snapshots {
        let path = snapshot
            .as_object()
            .and_then(|object| object.get("path"))
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| {
                AppError::storage_failed(
                    "pending file mutation manifest snapshots require a string path",
                )
            })?;
        if path.trim().is_empty() || path.contains('\0') {
            return Err(AppError::storage_failed(
                "pending file mutation manifest contains an invalid target path",
            ));
        }
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
