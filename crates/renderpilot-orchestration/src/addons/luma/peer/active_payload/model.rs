use std::{error::Error, fmt};

use renderpilot_domain::{PathRef, PathRefError, PeerTransitionError};

use crate::ServiceError;

/// The generic-file projection that the record builder can consume after the
/// peer transaction commits. A collision is represented by the same live path
/// in both lists: the live file is written by Luma and its captured baseline is
/// restored by the generic peer uninstall path from the managed `.bak`.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ActivePayloadProjection {
    pub(super) main_addon: PathRef,
    pub(super) created_files: Vec<PathRef>,
    pub(super) backed_up_files: Vec<PathRef>,
    pub(super) dlss_bytes: Option<Vec<u8>>,
}

/// Structurally and authority-validated payload, before any live-path
/// observation.  The update adapter consumes this boundary so no caller can
/// accidentally observe a partially validated payload.
#[derive(Debug)]
pub(crate) struct ValidatedActivePayload {
    main_addon: PathRef,
    targets: Vec<ValidatedActivePayloadTarget>,
    dlss_bytes: Option<Vec<u8>>,
}

impl ValidatedActivePayload {
    pub(crate) fn new(
        main_addon: PathRef,
        targets: Vec<ValidatedActivePayloadTarget>,
        dlss_bytes: Option<Vec<u8>>,
    ) -> Self {
        Self {
            main_addon,
            targets,
            dlss_bytes,
        }
    }

    pub(crate) fn into_parts(
        self,
    ) -> (PathRef, Vec<ValidatedActivePayloadTarget>, Option<Vec<u8>>) {
        (self.main_addon, self.targets, self.dlss_bytes)
    }
}

/// One validated generic payload endpoint.  `live` retains the canonical
/// effective-root spelling produced by the validator; update retention may
/// deliberately replace it with the persisted record spelling.
#[derive(Debug)]
pub(crate) struct ValidatedActivePayloadTarget {
    live: PathRef,
    sidecar: PathRef,
    bytes: Vec<u8>,
}

impl ValidatedActivePayloadTarget {
    pub(crate) fn new(live: PathRef, sidecar: PathRef, bytes: Vec<u8>) -> Self {
        Self {
            live,
            sidecar,
            bytes,
        }
    }

    pub(crate) fn into_parts(self) -> (PathRef, PathRef, Vec<u8>) {
        (self.live, self.sidecar, self.bytes)
    }
}

impl ActivePayloadProjection {
    pub(crate) fn main_addon(&self) -> &PathRef {
        &self.main_addon
    }

    pub(crate) fn created_files(&self) -> &[PathRef] {
        &self.created_files
    }

    pub(crate) fn backed_up_files(&self) -> &[PathRef] {
        &self.backed_up_files
    }

    #[cfg(test)]
    pub(crate) fn dlss_bytes(&self) -> Option<&[u8]> {
        self.dlss_bytes.as_deref()
    }

    pub(crate) fn take_dlss_bytes(&mut self) -> Option<Vec<u8>> {
        self.dlss_bytes.take()
    }
}

/// Structural failures are returned separately from authority, observation,
/// snapshot, and effect failures. The distinction lets callers reject shape
/// before a single filesystem observation occurs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ActivePayloadStructuralError {
    InvalidRelativePath { field: &'static str, value: String },
    InvalidMainAddon { value: String },
    MissingMainAddon { value: String },
    MultipleMainAddons { first: PathRef, second: PathRef },
    DuplicateTarget(PathRef),
    DuplicateDlss(PathRef),
    OverlappingTargets { first: PathRef, second: PathRef },
    ReservedTarget(PathRef),
    PathConversion(PathRefError),
    SidecarPath(PeerTransitionError),
}

impl fmt::Display for ActivePayloadStructuralError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRelativePath { field, value } => {
                write!(formatter, "invalid {field} relative payload path: {value}")
            }
            Self::InvalidMainAddon { value } => {
                write!(formatter, "invalid Luma main add-on path: {value}")
            }
            Self::MissingMainAddon { value } => {
                write!(formatter, "Luma payload has no main add-on at {value}")
            }
            Self::MultipleMainAddons { first, second } => write!(
                formatter,
                "Luma payload has multiple root main add-ons: {first} and {second}"
            ),
            Self::DuplicateTarget(path) => {
                write!(formatter, "Luma payload target is duplicated: {path}")
            }
            Self::DuplicateDlss(path) => {
                write!(
                    formatter,
                    "Luma payload contains duplicate DLSS target: {path}"
                )
            }
            Self::OverlappingTargets { first, second } => write!(
                formatter,
                "Luma payload targets overlap: {first} and {second}"
            ),
            Self::ReservedTarget(path) => {
                write!(formatter, "Luma payload target is reserved: {path}")
            }
            Self::PathConversion(error) => write!(
                formatter,
                "Luma payload path cannot be represented: {error}"
            ),
            Self::SidecarPath(error) => {
                write!(formatter, "Luma payload sidecar cannot be derived: {error}")
            }
        }
    }
}

impl Error for ActivePayloadStructuralError {}

/// Failure while rechecking a payload endpoint against the sealed roots.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ActivePayloadAuthorityError {
    pub(crate) path: PathRef,
    pub(crate) error: ServiceError,
}

impl fmt::Display for ActivePayloadAuthorityError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "Luma payload path is outside the sealed authority: {}: {}",
            self.path, self.error
        )
    }
}

impl Error for ActivePayloadAuthorityError {}

/// Failure from the one retained no-follow observation for an endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ActivePayloadObservationError {
    pub(crate) path: PathRef,
    pub(crate) error: ServiceError,
}

impl fmt::Display for ActivePayloadObservationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "failed to observe Luma payload endpoint {}: {}",
            self.path, self.error
        )
    }
}

impl Error for ActivePayloadObservationError {}

/// The complete typed error surface of the active generic payload adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ActivePayloadError {
    Structural(ActivePayloadStructuralError),
    Authority(ActivePayloadAuthorityError),
    Observation(ActivePayloadObservationError),
    Snapshot(super::super::snapshot_input::LumaSnapshotInputError),
    Effects(super::super::effects::LumaPeerEffectError),
}

impl fmt::Display for ActivePayloadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Structural(error) => error.fmt(formatter),
            Self::Authority(error) => error.fmt(formatter),
            Self::Observation(error) => error.fmt(formatter),
            Self::Snapshot(error) => error.fmt(formatter),
            Self::Effects(error) => error.fmt(formatter),
        }
    }
}

impl Error for ActivePayloadError {}

impl From<ActivePayloadStructuralError> for ActivePayloadError {
    fn from(error: ActivePayloadStructuralError) -> Self {
        Self::Structural(error)
    }
}

impl From<super::super::snapshot_input::LumaSnapshotInputError> for ActivePayloadError {
    fn from(error: super::super::snapshot_input::LumaSnapshotInputError) -> Self {
        Self::Snapshot(error)
    }
}

impl From<super::super::effects::LumaPeerEffectError> for ActivePayloadError {
    fn from(error: super::super::effects::LumaPeerEffectError) -> Self {
        Self::Effects(error)
    }
}

#[derive(Debug)]
pub(super) struct GenericTarget {
    pub(super) live: PathRef,
    pub(super) sidecar: PathRef,
    pub(super) bytes: Vec<u8>,
}

#[derive(Debug)]
pub(super) struct ValidatedPayloadFile {
    pub(super) relative: std::path::PathBuf,
    pub(super) key: String,
    pub(super) bytes: Vec<u8>,
}

#[derive(Debug)]
pub(super) struct ObservedGenericTarget {
    pub(super) target: GenericTarget,
    pub(super) live_snapshot: crate::peer_mutation_executor::PeerPathSnapshot,
    pub(super) sidecar_snapshot: crate::peer_mutation_executor::PeerPathSnapshot,
}
