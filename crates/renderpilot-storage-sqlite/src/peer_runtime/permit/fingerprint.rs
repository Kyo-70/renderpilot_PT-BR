//! Durable row fingerprints and transaction-local projections.

use renderpilot_application::{AppError, AppResult};
use renderpilot_domain::{GameId, InstalledAddon};
use rusqlite::{OptionalExtension, Transaction};

use super::super::manifest::sha256_bytes;
use super::types::RowFingerprint;
use crate::error::storage_error;
use crate::repositories::{installed_addons, proxy_topologies};

pub(crate) fn ensure_runtime(
    expected: &std::sync::Arc<super::types::RuntimeInstance>,
    actual: &std::sync::Arc<super::types::RuntimeInstance>,
) -> AppResult<()> {
    if !std::sync::Arc::ptr_eq(expected, actual) {
        return Err(AppError::storage_failed(
            "peer commit permit belongs to another storage runtime",
        ));
    }
    Ok(())
}

pub(crate) fn ensure_identity_stable(
    previous: RowFingerprint,
    current: RowFingerprint,
) -> AppResult<()> {
    if previous.rowid != current.rowid || previous.created_at != current.created_at {
        return Err(AppError::storage_failed(
            "peer preparation row identity changed during transition",
        ));
    }
    Ok(())
}

pub(crate) fn invalidate_catalog_for_prepared(
    transaction: &Transaction<'_>,
    game_id: &GameId,
    mutation_id: &str,
) -> AppResult<()> {
    let exists: bool = transaction
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM games WHERE id = ?1)",
            [game_id.as_str()],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    if exists {
        crate::repositories::observations::invalidate_game_authority_within_transaction(
            transaction,
            game_id,
            "prepared_peer_mutation",
            Some(mutation_id),
        )?;
    }
    Ok(())
}

pub(crate) fn read_file_fingerprint(
    transaction: &Transaction<'_>,
    id: &str,
) -> AppResult<Option<RowFingerprint>> {
    transaction
        .query_row(
            "SELECT rowid, created_at, updated_at, manifest_json
             FROM pending_file_mutations WHERE id = ?1",
            [id],
            |row| {
                let manifest: String = row.get(3)?;
                Ok(RowFingerprint {
                    rowid: row.get(0)?,
                    created_at: row.get(1)?,
                    updated_at: row.get(2)?,
                    manifest_sha256: sha256_bytes(manifest.as_bytes()),
                    root_capabilities_sha256: None,
                })
            },
        )
        .optional()
        .map_err(storage_error)
}

pub(crate) fn read_shared_fingerprint(
    transaction: &Transaction<'_>,
    id: &str,
) -> AppResult<Option<RowFingerprint>> {
    transaction
        .query_row(
            "SELECT rowid, created_at, updated_at, manifest_json,
                    root_capabilities_json
             FROM pending_shared_vulkan_mutations
             WHERE resource_key = ?1 AND id = ?2",
            rusqlite::params![
                crate::repositories::pending_shared_vulkan_mutations::RESOURCE_KEY,
                id,
            ],
            |row| {
                let manifest: String = row.get(3)?;
                let root_capabilities: String = row.get(4)?;
                Ok(RowFingerprint {
                    rowid: row.get(0)?,
                    created_at: row.get(1)?,
                    updated_at: row.get(2)?,
                    manifest_sha256: sha256_bytes(manifest.as_bytes()),
                    root_capabilities_sha256: Some(sha256_bytes(root_capabilities.as_bytes())),
                })
            },
        )
        .optional()
        .map_err(storage_error)
}

pub(crate) fn read_shared_root_capabilities(
    transaction: &Transaction<'_>,
    id: &str,
) -> AppResult<String> {
    transaction
        .query_row(
            "SELECT root_capabilities_json
             FROM pending_shared_vulkan_mutations
             WHERE resource_key = ?1 AND id = ?2",
            rusqlite::params![
                crate::repositories::pending_shared_vulkan_mutations::RESOURCE_KEY,
                id,
            ],
            |row| row.get(0),
        )
        .map_err(storage_error)
}

pub(crate) fn read_file_manifest(transaction: &Transaction<'_>, id: &str) -> AppResult<String> {
    transaction
        .query_row(
            "SELECT manifest_json FROM pending_file_mutations WHERE id = ?1",
            [id],
            |row| row.get(0),
        )
        .map_err(storage_error)
}

pub(crate) fn read_shared_manifest(transaction: &Transaction<'_>, id: &str) -> AppResult<String> {
    transaction
        .query_row(
            "SELECT manifest_json
             FROM pending_shared_vulkan_mutations
             WHERE resource_key = ?1 AND id = ?2",
            rusqlite::params![
                crate::repositories::pending_shared_vulkan_mutations::RESOURCE_KEY,
                id,
            ],
            |row| row.get(0),
        )
        .map_err(storage_error)
}

pub(crate) fn apply_peer_snapshots(
    transaction: &Transaction<'_>,
    game_id: &GameId,
    after_peer: Option<&InstalledAddon>,
    after_topology: Option<&renderpilot_domain::GameProxyTopology>,
) -> AppResult<()> {
    if let Some(topology) = after_topology {
        proxy_topologies::upsert_within_transaction(transaction, topology)?;
    } else {
        proxy_topologies::delete_within_transaction(transaction, game_id)?;
    }
    if let Some(peer) = after_peer {
        installed_addons::upsert_within_transaction(transaction, peer)
    } else {
        if let Some(existing) = installed_addons::get_within_transaction(transaction, game_id)? {
            installed_addons::delete_within_transaction(transaction, game_id, existing.kind())?;
        }
        Ok(())
    }
}

pub(crate) fn domain_error(error: impl std::fmt::Display) -> AppError {
    AppError::invalid_input(format!("invalid peer transition: {error}"))
}
