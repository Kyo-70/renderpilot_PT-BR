//! Shared validation of immutable peer snapshots and managed sidecars.

use std::{error::Error, fmt};

use renderpilot_domain::{PathRef, PeerTransitionError, managed_sidecar_path, normalized_path_key};

use crate::peer_mutation_executor::{PeerPathSnapshot, VerifiedPeerFile};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LumaSnapshotInputError {
    ExpectedAbsent(PathRef),
    ExpectedFile(PathRef),
    SidecarPathMismatch {
        expected: PathRef,
        supplied: PathRef,
    },
    Domain(PeerTransitionError),
}

impl fmt::Display for LumaSnapshotInputError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ExpectedAbsent(path) => {
                write!(formatter, "expected an absent peer endpoint: {path}")
            }
            Self::ExpectedFile(path) => write!(formatter, "expected a present peer file: {path}"),
            Self::SidecarPathMismatch { expected, supplied } => write!(
                formatter,
                "peer sidecar path {supplied} does not match managed path {expected}"
            ),
            Self::Domain(error) => error.fmt(formatter),
        }
    }
}

impl Error for LumaSnapshotInputError {}

impl From<PeerTransitionError> for LumaSnapshotInputError {
    fn from(error: PeerTransitionError) -> Self {
        Self::Domain(error)
    }
}

pub(super) fn require_absent(
    path: &PathRef,
    snapshot: &PeerPathSnapshot,
) -> Result<(), LumaSnapshotInputError> {
    if matches!(snapshot, PeerPathSnapshot::Absent) {
        Ok(())
    } else {
        Err(LumaSnapshotInputError::ExpectedAbsent(path.clone()))
    }
}

pub(super) fn require_file<'a>(
    path: &PathRef,
    snapshot: &'a PeerPathSnapshot,
) -> Result<&'a VerifiedPeerFile, LumaSnapshotInputError> {
    snapshot
        .file()
        .ok_or_else(|| LumaSnapshotInputError::ExpectedFile(path.clone()))
}

pub(super) fn snapshot_bytes(
    path: &PathRef,
    snapshot: &PeerPathSnapshot,
) -> Result<Vec<u8>, LumaSnapshotInputError> {
    snapshot
        .bytes()
        .map(ToOwned::to_owned)
        .ok_or_else(|| LumaSnapshotInputError::ExpectedFile(path.clone()))
}

pub(super) fn require_managed_sidecar(
    live_path: &PathRef,
    supplied: &PathRef,
) -> Result<PathRef, LumaSnapshotInputError> {
    let expected = managed_sidecar_path(live_path)?;
    if normalized_path_key(expected.as_str()) != normalized_path_key(supplied.as_str()) {
        return Err(LumaSnapshotInputError::SidecarPathMismatch {
            expected,
            supplied: supplied.clone(),
        });
    }
    Ok(expected)
}
