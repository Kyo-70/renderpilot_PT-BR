//! Exact downstream ReShade-slot observation for persisted proxy topologies.
//!
//! This module deliberately does not enumerate a game directory. It reads one
//! caller-validated path, rejects links/non-files, and derives the digest and
//! PE facts from the same retained byte buffer.

use std::fs::{self, File};
use std::io::Read;
use std::path::Path;

use renderpilot_detection::{PeInspection, inspect_pe_bytes};
use renderpilot_domain::Sha256Hash;
use sha2::{Digest, Sha256};

use crate::ServiceError;

/// Result of the one retained read used by the topology-aware exact-slot
/// assessor. Unlike [`super::scan_reshade_hosts`], this never enumerates
/// candidates.
#[derive(Debug)]
pub(crate) enum ExactHostObservation {
    Absent,
    Present {
        digest: Sha256Hash,
        length: u64,
        inspection: Box<PeInspection>,
    },
}

/// Reads one exact host path without accepting links, directories, or a file
/// that changed while it was being read. The PE parser and digest consume the
/// same retained byte buffer, so no field triggers a second file read.
pub(crate) fn observe_exact_host_file(path: &Path) -> Result<ExactHostObservation, ServiceError> {
    let before = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(ExactHostObservation::Absent);
        }
        Err(error) => {
            return Err(ServiceError::invalid_input(format!(
                "failed to inspect exact ReShade downstream host `{}`: {error}",
                path.display()
            )));
        }
    };
    if !is_safe_regular_file(&before) {
        return Err(ServiceError::invalid_input(format!(
            "exact ReShade downstream host `{}` is not a regular non-link file",
            path.display()
        )));
    }

    let mut file = File::open(path).map_err(|error| {
        ServiceError::invalid_input(format!(
            "failed to read exact ReShade downstream host `{}`: {error}",
            path.display()
        ))
    })?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).map_err(|error| {
        ServiceError::invalid_input(format!(
            "failed to read exact ReShade downstream host `{}`: {error}",
            path.display()
        ))
    })?;

    let after = fs::symlink_metadata(path).map_err(|error| {
        ServiceError::invalid_input(format!(
            "exact ReShade downstream host `{}` changed while being read: {error}",
            path.display()
        ))
    })?;
    if !is_safe_regular_file(&after) || !same_file_observation(&before, &after) {
        return Err(ServiceError::invalid_input(format!(
            "exact ReShade downstream host `{}` changed while being read",
            path.display()
        )));
    }

    let length = u64::try_from(bytes.len()).map_err(|error| {
        ServiceError::invalid_input(format!(
            "exact ReShade downstream host `{}` is too large: {error}",
            path.display()
        ))
    })?;
    let digest = Sha256Hash::new(hex::encode(Sha256::digest(&bytes))).map_err(|error| {
        ServiceError::invalid_input(format!(
            "failed to hash exact ReShade downstream host `{}`: {error}",
            path.display()
        ))
    })?;
    Ok(ExactHostObservation::Present {
        digest,
        length,
        inspection: Box::new(inspect_pe_bytes(&bytes)),
    })
}

fn is_safe_regular_file(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_file() && !metadata.file_type().is_symlink() && !is_reparse(metadata)
}

fn same_file_observation(before: &fs::Metadata, after: &fs::Metadata) -> bool {
    before.len() == after.len()
        && same_file_identity(before, after)
        && matches!(
            (before.modified().ok(), after.modified().ok()),
            (Some(before), Some(after)) if before == after
        )
}

#[cfg(unix)]
fn same_file_identity(before: &fs::Metadata, after: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    before.dev() == after.dev() && before.ino() == after.ino()
}

// Stable Rust does not expose Windows file-index identity through
// `std::fs::Metadata`; the retained read is still fenced by no-reparse
// metadata, size, and write-time checks below. The native authority used by
// the later mutation phase supplies the stronger handle-bound CAS.
#[cfg(windows)]
fn same_file_identity(_before: &fs::Metadata, _after: &fs::Metadata) -> bool {
    true
}

#[cfg(not(any(unix, windows)))]
fn same_file_identity(_before: &fs::Metadata, _after: &fs::Metadata) -> bool {
    true
}

#[cfg(windows)]
fn is_reparse(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn is_reparse(_metadata: &fs::Metadata) -> bool {
    false
}
