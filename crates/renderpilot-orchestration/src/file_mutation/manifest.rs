//! On-disk before-state manifest for a durable file transaction.

use std::path::Path;

use renderpilot_storage_sqlite::{PendingFileMutationRow, PreparedMutationResolutionFence};
use serde::{Deserialize, Serialize};

use crate::ServiceError;
use crate::peer_mutation_executor::{PeerAncestorManifestEntry, PeerProgramEnvelope};

pub(super) const MANIFEST_FORMAT_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct FileMutationManifest {
    pub(super) format_version: u32,
    pub(super) roots: Vec<String>,
    pub(super) transaction_dir: String,
    pub(super) snapshots: Vec<FileBeforeSnapshot>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) peer_ancestors: Vec<PeerAncestorManifestEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct FileBeforeSnapshot {
    pub(crate) path: String,
    pub(crate) snapshot: Option<String>,
}

pub(super) fn serialize_manifest(manifest: &FileMutationManifest) -> Result<String, ServiceError> {
    serialize_manifest_with_peer_program(manifest, None)
}

pub(super) fn serialize_manifest_with_peer_program(
    manifest: &FileMutationManifest,
    peer_program: Option<&PeerProgramEnvelope>,
) -> Result<String, ServiceError> {
    let mut value = serde_json::to_value(manifest).map_err(|error| {
        crate::failed(format!(
            "failed to serialize file transaction manifest: {error}"
        ))
    })?;
    if let Some(peer_program) = peer_program {
        let object = value.as_object_mut().ok_or_else(|| {
            crate::failed("file transaction manifest must serialize as an object")
        })?;
        object.insert(
            "peer_program".to_owned(),
            serde_json::to_value(peer_program).map_err(|error| {
                crate::failed(format!("failed to serialize peer program: {error}"))
            })?,
        );
    }
    serde_json::to_string(&value).map_err(|error| {
        crate::failed(format!(
            "failed to serialize file transaction manifest: {error}"
        ))
    })
}

pub(super) fn deserialize_manifest(
    row: &PendingFileMutationRow,
) -> Result<FileMutationManifest, ServiceError> {
    serde_json::from_str(&row.manifest_json).map_err(|error| {
        crate::failed(format!(
            "pending file mutation {} has an invalid manifest: {error}",
            row.id
        ))
    })
}

/// Restores only after storage minted a fence for this exact Prepared row.
pub(super) fn restore_manifest(
    manifest: &FileMutationManifest,
    _fence: &PreparedMutationResolutionFence,
) -> Result<(), ServiceError> {
    for before in manifest.snapshots.iter().rev() {
        let path = Path::new(&before.path);
        match &before.snapshot {
            Some(snapshot) => crate::fs::copy_file_atomically(Path::new(snapshot), path)?,
            None => crate::fs::remove_file_if_exists(path)?,
        }
    }
    Ok(())
}

/// Cleans a manifest created and retained in this process. Crash recovery must
/// instead carry the canonical directory returned by the exact-owner validator.
pub(super) fn cleanup_manifest(manifest: &FileMutationManifest) -> Result<(), ServiceError> {
    super::remove_dir_if_exists(Path::new(&manifest.transaction_dir))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn old_manifest_defaults_ancestors_and_empty_wire_omits_them() {
        let manifest: FileMutationManifest = serde_json::from_str(
            r#"{"format_version":1,"roots":["C:/game"],"transaction_dir":"C:/txn","snapshots":[]}"#,
        )
        .expect("old manifest");
        assert!(manifest.peer_ancestors.is_empty());
        let encoded = serialize_manifest(&manifest).expect("serialize");
        assert_eq!(
            encoded,
            r#"{"format_version":1,"roots":["C:/game"],"snapshots":[],"transaction_dir":"C:/txn"}"#
        );
    }
}
