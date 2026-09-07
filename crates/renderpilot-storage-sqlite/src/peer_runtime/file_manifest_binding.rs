//! Exact binding of a file mutation manifest to its sealed peer program.
//!
//! This module owns the native outer-manifest projection used by both commit
//! preparation and recovery.  It does not parse JSON; callers pass the single
//! already-parsed `Value` from the storage boundary.

use renderpilot_application::{AppError, AppResult};
use renderpilot_domain::{CapabilityToken, PeerEndpointOperation};
use serde_json::Value;

use super::ParsedPeerProgram;
use super::ancestor_binding;
use super::recovery::PeerRecoveryAncestor;

/// Exact outer-manifest projection shared by commit preparation and recovery.
///
/// The recovery boundary consumes this projection instead of parsing native
/// snapshot and ancestor arrays a second time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ValidatedFilePeerManifest {
    pub(super) format: u64,
    pub(super) roots: Vec<String>,
    pub(super) transaction_dir: Option<String>,
    pub(super) snapshot_paths: Vec<Option<String>>,
    pub(super) ancestors: Vec<PeerRecoveryAncestor>,
}

/// Binds the parsed file manifest to the exact ordered peer program.
pub(super) fn bind(
    manifest: &Value,
    program: &ParsedPeerProgram,
) -> AppResult<ValidatedFilePeerManifest> {
    let object = manifest
        .as_object()
        .ok_or_else(|| AppError::storage_failed("file peer manifest must be an object"))?;
    let format = object
        .get("format_version")
        .and_then(Value::as_u64)
        .ok_or_else(|| AppError::storage_failed("file peer manifest format_version is missing"))?;
    let expected_format = match program.execution_class() {
        "ordinary" => 1,
        "retryable" => 2,
        _ => {
            return Err(AppError::storage_failed(
                "file peer manifest has an unsupported execution class",
            ));
        }
    };
    if format != expected_format {
        return Err(AppError::storage_failed(
            "file peer manifest format does not match peer program execution class",
        ));
    }
    let roots = required_string_array(object, "roots", "file peer manifest")?;
    if roots != program.roots() {
        return Err(AppError::storage_failed(
            "file peer manifest roots differ from peer program roots",
        ));
    }
    if !program.stage().is_empty() {
        return Err(AppError::storage_failed(
            "file peer program cannot carry stage capabilities",
        ));
    }
    let ancestors = ancestor_binding::bind(object, program, &roots)?;

    let snapshot_paths = match format {
        1 => bind_ordinary_manifest(object, program, &roots)?,
        2 => bind_retryable_manifest(object, program, &roots)?,
        _ => {
            return Err(AppError::storage_failed(
                "file peer manifest format_version is unsupported",
            ));
        }
    };
    let transaction_dir = object
        .get("transaction_dir")
        .map(|value| {
            value
                .as_str()
                .filter(|value| !value.trim().is_empty() && !value.contains('\0'))
                .map(str::to_owned)
                .ok_or_else(|| {
                    AppError::storage_failed(
                        "file peer manifest transaction_dir must be a non-empty string",
                    )
                })
        })
        .transpose()?;
    Ok(ValidatedFilePeerManifest {
        format,
        roots,
        transaction_dir,
        snapshot_paths,
        ancestors,
    })
}

fn bind_ordinary_manifest(
    manifest: &serde_json::Map<String, Value>,
    program: &ParsedPeerProgram,
    roots: &[String],
) -> AppResult<Vec<Option<String>>> {
    let snapshots = required_array(manifest, "snapshots", "ordinary file peer manifest")?;
    if snapshots.len() != program.intents().len() {
        return Err(AppError::storage_failed(
            "ordinary file snapshot cardinality differs from peer endpoints",
        ));
    }
    let mut expected_custody = Vec::new();
    let mut snapshot_paths = Vec::with_capacity(snapshots.len());
    for (index, snapshot) in snapshots.iter().enumerate() {
        let snapshot = snapshot.as_object().ok_or_else(|| {
            AppError::storage_failed("ordinary file manifest snapshot must be an object")
        })?;
        let path = required_string(snapshot, "path", "ordinary file peer manifest")?;
        let intent = &program.intents()[index];
        if path != intent.path().as_str() {
            return Err(AppError::storage_failed(
                "ordinary file snapshot order or path differs from peer endpoints",
            ));
        }
        let snapshot_value = snapshot.get("snapshot").ok_or_else(|| {
            AppError::storage_failed("ordinary file snapshot must declare its snapshot path")
        })?;
        let snapshot_path = optional_snapshot_path(snapshot_value, "ordinary file")?;
        if snapshot_path.is_some() != program.before()[index].is_some() {
            return Err(AppError::storage_failed(
                "ordinary file snapshot presence differs from peer O1",
            ));
        }
        if program.before()[index].is_some() {
            expected_custody.push(CapabilityToken::from_path(intent.path(), roots).map_err(
                |error| {
                    AppError::storage_failed(format!(
                        "file peer manifest custody capability: {error}"
                    ))
                },
            )?);
        }
        reject_byte_identical_replace(program, index)?;
        snapshot_paths.push(snapshot_path);
    }
    compare_auxiliary_capabilities(program, expected_custody)?;
    Ok(snapshot_paths)
}

fn bind_retryable_manifest(
    manifest: &serde_json::Map<String, Value>,
    program: &ParsedPeerProgram,
    roots: &[String],
) -> AppResult<Vec<Option<String>>> {
    let operations = required_array(manifest, "operations", "retryable file peer manifest")?;
    let snapshots = required_array(manifest, "snapshots", "retryable file peer manifest")?;
    if operations.len() != program.intents().len() || snapshots.len() != operations.len() {
        return Err(AppError::storage_failed(
            "retryable file operation/snapshot cardinality differs from peer endpoints",
        ));
    }
    let mut expected_custody = Vec::new();
    let mut snapshot_paths = Vec::with_capacity(snapshots.len());
    for (index, (operation, snapshot)) in operations.iter().zip(snapshots).enumerate() {
        let operation = operation.as_object().ok_or_else(|| {
            AppError::storage_failed("retryable file operation must be an object")
        })?;
        let snapshot = snapshot
            .as_object()
            .ok_or_else(|| AppError::storage_failed("retryable file snapshot must be an object"))?;
        let intent = &program.intents()[index];
        let operation_path = required_string(operation, "path", "retryable file peer manifest")?;
        let snapshot_path = required_string(snapshot, "path", "retryable file peer manifest")?;
        if operation_path != intent.path().as_str() || snapshot_path != operation_path {
            return Err(AppError::storage_failed(
                "retryable file operation/snapshot paths differ from peer endpoints",
            ));
        }
        let expected_kind = match intent.operation() {
            PeerEndpointOperation::Create | PeerEndpointOperation::Replace => "write",
            PeerEndpointOperation::Remove => "delete",
        };
        if operation.get("kind").and_then(Value::as_str) != Some(expected_kind) {
            return Err(AppError::storage_failed(
                "retryable file operation kind differs from peer intent",
            ));
        }
        let expected_before = expected_observation(program.before()[index].as_ref());
        if operation.get("expected") != Some(&expected_before)
            || snapshot.get("before") != Some(&expected_before)
        {
            return Err(AppError::storage_failed(
                "retryable file expected preimage differs from peer O1",
            ));
        }
        match intent.operation() {
            PeerEndpointOperation::Create | PeerEndpointOperation::Replace => {
                if operation.get("post_digest").and_then(Value::as_str)
                    != intent
                        .planned_sha256()
                        .map(renderpilot_domain::Sha256Hash::as_str)
                {
                    return Err(AppError::storage_failed(
                        "retryable file postimage digest differs from peer intent",
                    ));
                }
            }
            PeerEndpointOperation::Remove => {
                if operation.get("post_digest").is_some() {
                    return Err(AppError::storage_failed(
                        "retryable delete operation carries a postimage digest",
                    ));
                }
            }
        }
        let snapshot_value = snapshot.get("snapshot").ok_or_else(|| {
            AppError::storage_failed("retryable file snapshot must declare its snapshot path")
        })?;
        let snapshot_path = optional_snapshot_path(snapshot_value, "retryable file")?;
        if snapshot_path.is_some() != program.before()[index].is_some() {
            return Err(AppError::storage_failed(
                "retryable file snapshot presence differs from peer O1",
            ));
        }
        if program.before()[index].is_some() {
            expected_custody.push(CapabilityToken::from_path(intent.path(), roots).map_err(
                |error| {
                    AppError::storage_failed(format!(
                        "file peer manifest custody capability: {error}"
                    ))
                },
            )?);
        }
        reject_byte_identical_replace(program, index)?;
        snapshot_paths.push(snapshot_path);
    }
    compare_auxiliary_capabilities(program, expected_custody)?;
    Ok(snapshot_paths)
}

fn optional_snapshot_path(value: &Value, context: &str) -> AppResult<Option<String>> {
    if value.is_null() {
        return Ok(None);
    }
    value
        .as_str()
        .filter(|value| !value.trim().is_empty() && !value.contains('\0'))
        .map(str::to_owned)
        .map(Some)
        .ok_or_else(|| {
            AppError::storage_failed(format!(
                "{context} snapshot path must be a non-empty string or null"
            ))
        })
}

fn reject_byte_identical_replace(program: &ParsedPeerProgram, index: usize) -> AppResult<()> {
    let intent = &program.intents()[index];
    if intent.operation() != PeerEndpointOperation::Replace {
        return Ok(());
    }
    let before = program.before()[index]
        .as_ref()
        .ok_or_else(|| AppError::storage_failed("replace endpoint is missing its O1 image"))?;
    if Some(before.sha256()) == intent.planned_sha256()
        && Some(before.length()) == intent.planned_length()
    {
        return Err(AppError::storage_failed(
            "replace endpoint has byte-identical before and after images",
        ));
    }
    Ok(())
}

fn compare_auxiliary_capabilities(
    program: &ParsedPeerProgram,
    mut expected_custody: Vec<CapabilityToken>,
) -> AppResult<()> {
    expected_custody.sort();
    let actual_custody = program.custody().to_vec();
    if actual_custody != expected_custody {
        return Err(AppError::storage_failed(
            "file peer custody capabilities differ from O1 file endpoints",
        ));
    }
    for (index, intent) in program.intents().iter().enumerate() {
        let expected =
            CapabilityToken::from_path(intent.path(), program.roots()).map_err(|error| {
                AppError::storage_failed(format!(
                    "file peer manifest read-guard capability: {error}"
                ))
            })?;
        let actual = program.read_guards().get(index).ok_or_else(|| {
            AppError::storage_failed("file peer read guard cardinality differs from endpoints")
        })?;
        if actual.len() != 1 || actual[0] != expected {
            return Err(AppError::storage_failed(
                "file peer read guard is not the endpoint root-relative capability",
            ));
        }
    }
    Ok(())
}

fn required_array<'a>(
    object: &'a serde_json::Map<String, Value>,
    key: &str,
    context: &str,
) -> AppResult<&'a Vec<Value>> {
    object
        .get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| AppError::storage_failed(format!("{context} {key} must be an array")))
}

fn required_string_array(
    object: &serde_json::Map<String, Value>,
    key: &str,
    context: &str,
) -> AppResult<Vec<String>> {
    required_array(object, key, context)?
        .iter()
        .map(|value| {
            value
                .as_str()
                .filter(|value| !value.trim().is_empty())
                .map(str::to_owned)
                .ok_or_else(|| {
                    AppError::storage_failed(format!("{context} {key} must contain strings"))
                })
        })
        .collect()
}

fn required_string<'a>(
    object: &'a serde_json::Map<String, Value>,
    key: &str,
    context: &str,
) -> AppResult<&'a str> {
    object
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty() && !value.contains('\0'))
        .ok_or_else(|| AppError::storage_failed(format!("{context} {key} must be a string")))
}

fn expected_observation(before: Option<&renderpilot_domain::PeerFileImage>) -> Value {
    match before {
        None => serde_json::json!({ "kind": "absent" }),
        Some(file) => serde_json::json!({
            "kind": "regular",
            "digest": file.sha256().as_str(),
        }),
    }
}
