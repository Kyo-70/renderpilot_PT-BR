use renderpilot_application::{AppError, AppResult};
use renderpilot_domain::{PathRef, normalized_path_key};
use rusqlite::Transaction;
use serde_json::Value;

use super::read_guards::{is_strict_descendant, strict_absolute_path};
use crate::error::storage_error;

const VERSION: u64 = 1;
const SHARED_ID: &str = "shared";

/// Storage-owned interpretation of one shared-Vulkan root capability envelope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct BoundSharedPeerRoots {
    canonical_game_root: Option<PathRef>,
}

impl BoundSharedPeerRoots {
    pub(super) fn canonical_game_root(&self) -> Option<&PathRef> {
        self.canonical_game_root.as_ref()
    }
}

pub(super) fn bind_preparing_row(
    transaction: &Transaction<'_>,
    mutation_id: &str,
    program_roots: &[String],
) -> AppResult<BoundSharedPeerRoots> {
    let root_capabilities_json: String = transaction
        .query_row(
            "SELECT root_capabilities_json
             FROM pending_shared_vulkan_mutations
             WHERE resource_key = ?1 AND id = ?2",
            rusqlite::params![
                crate::repositories::pending_shared_vulkan_mutations::RESOURCE_KEY,
                mutation_id,
            ],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    bind_persisted_json(&root_capabilities_json, program_roots)
}

pub(super) fn bind_persisted_json(
    root_capabilities_json: &str,
    program_roots: &[String],
) -> AppResult<BoundSharedPeerRoots> {
    let value: Value = serde_json::from_str(root_capabilities_json)
        .map_err(|error| AppError::storage_failed(format!("invalid shared root seal: {error}")))?;
    let object = value
        .as_object()
        .ok_or_else(|| AppError::storage_failed("shared root seal must be an object"))?;
    require_exact_keys(object, &["version", "roots"])?;
    if object.get("version").and_then(Value::as_u64) != Some(VERSION) {
        return Err(AppError::storage_failed(
            "unsupported shared root capability version",
        ));
    }
    let roots = object
        .get("roots")
        .and_then(Value::as_array)
        .ok_or_else(|| AppError::storage_failed("shared root seal roots must be an array"))?;
    if roots.is_empty() {
        return Err(AppError::storage_failed(
            "shared root seal must contain the shared root",
        ));
    }

    let mut canonical_paths = Vec::with_capacity(roots.len());
    let mut game_count = 0usize;
    let mut shared_count = 0usize;
    for (index, root) in roots.iter().enumerate() {
        let root = root
            .as_object()
            .ok_or_else(|| AppError::storage_failed("shared root entry must be an object"))?;
        require_exact_keys(root, &["id", "kind", "canonical_path"])?;
        let id = root
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| AppError::storage_failed("shared root entry id is invalid"))?;
        let kind = root
            .get("kind")
            .and_then(Value::as_str)
            .ok_or_else(|| AppError::storage_failed("shared root entry kind is invalid"))?;
        let canonical_path = root
            .get("canonical_path")
            .and_then(Value::as_str)
            .ok_or_else(|| AppError::storage_failed("shared root entry path is invalid"))?;
        let path = strict_absolute_path(canonical_path, "shared root path")?;
        match kind {
            "game" => {
                if id != format!("game-{game_count}") {
                    return Err(AppError::storage_failed(
                        "shared game roots must use contiguous ordered ids",
                    ));
                }
                game_count += 1;
            }
            "shared_vulkan" => {
                shared_count += 1;
                if id != SHARED_ID || index + 1 != roots.len() {
                    return Err(AppError::storage_failed(
                        "shared Vulkan root must be unique and last",
                    ));
                }
            }
            _ => {
                return Err(AppError::storage_failed(
                    "shared root entry kind is invalid",
                ));
            }
        }
        canonical_paths.push(path);
    }
    if shared_count != 1
        || canonical_paths
            .iter()
            .map(PathRef::as_str)
            .ne(program_roots.iter().map(String::as_str))
    {
        return Err(AppError::storage_failed(
            "shared root seal paths differ from the peer program roots",
        ));
    }
    for (index, path) in canonical_paths.iter().enumerate() {
        if canonical_paths.iter().skip(index + 1).any(|other| {
            normalized_path_key(path.as_str()) == normalized_path_key(other.as_str())
                || is_strict_descendant(path.as_str(), other.as_str())
                || is_strict_descendant(other.as_str(), path.as_str())
        }) {
            return Err(AppError::storage_failed("shared root capabilities overlap"));
        }
    }

    Ok(BoundSharedPeerRoots {
        canonical_game_root: (game_count > 0).then(|| canonical_paths[0].clone()),
    })
}

fn require_exact_keys(object: &serde_json::Map<String, Value>, expected: &[&str]) -> AppResult<()> {
    if object.len() != expected.len() || expected.iter().any(|key| !object.contains_key(*key)) {
        return Err(AppError::storage_failed(
            "shared root seal contains unknown or missing keys",
        ));
    }
    Ok(())
}
