use std::{error::Error, fmt};

use renderpilot_domain::{
    ManagedAddonFile, PathRef, PeerTransitionError, Sha256Hash, managed_sidecar_path,
};

use crate::addons::luma::peer::effects::LumaPeerEffectError;

/// The result of active-install DLSS classification.
///
/// `NoPayload` and `Reused` contain no sidecar path or sidecar evidence. The
/// only value which can be lowered into a physical owned transition is
/// `Owned`.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ActiveDlssClassification {
    NoPayload,
    Reused { binding: ManagedAddonFile },
    Owned(ActiveDlssOwnedPlan),
}

impl ActiveDlssClassification {
    pub(crate) fn binding(&self) -> Option<&ManagedAddonFile> {
        match self {
            Self::NoPayload => None,
            Self::Reused { binding } => Some(binding),
            Self::Owned(plan) => Some(plan.binding()),
        }
    }

    #[cfg(test)]
    pub(crate) fn owned(&self) -> Option<&ActiveDlssOwnedPlan> {
        match self {
            Self::Owned(plan) => Some(plan),
            Self::NoPayload | Self::Reused { .. } => None,
        }
    }

    pub(crate) fn into_owned(self) -> Option<ActiveDlssOwnedPlan> {
        match self {
            Self::Owned(plan) => Some(plan),
            Self::NoPayload | Self::Reused { .. } => None,
        }
    }
}

/// A complete owned DLSS transition. The sidecar path is intentionally not
/// stored here: it can only be derived by the owned lowerer, immediately
/// before the exact sidecar snapshot is validated.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ActiveDlssOwnedPlan {
    pub(super) target: PathRef,
    pub(super) binding: ManagedAddonFile,
    pub(super) bundled_bytes: Vec<u8>,
    pub(super) transition: ActiveDlssOwnedTransition,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum ActiveDlssOwnedTransition {
    Create,
    Replace { live_digest: Sha256Hash },
}

impl ActiveDlssOwnedPlan {
    pub(crate) fn binding(&self) -> &ManagedAddonFile {
        &self.binding
    }

    /// Returns the only sidecar observation request permitted by this plan.
    pub(crate) fn sidecar_path(&self) -> Result<PathRef, ActiveDlssLoweringError> {
        managed_sidecar_path(&self.target).map_err(ActiveDlssLoweringError::Sidecar)
    }
}

/// Fail-closed errors from active DLSS classification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ActiveDlssClassificationError {
    InvalidTarget(PathRef),
    InvalidBundled(String),
    InvalidLive {
        path: PathRef,
        detail: String,
    },
    IncompatibleLive {
        path: PathRef,
        live_version: String,
        bundled_version: String,
    },
    CatalogMissing(PathRef),
    CatalogDrift {
        path: PathRef,
        expected: Vec<Sha256Hash>,
        observed: Sha256Hash,
    },
    CatalogReplacementForbidden {
        path: PathRef,
        reason: &'static str,
    },
    SnapshotImageMismatch(PathRef),
    SnapshotMissingBytes(PathRef),
}

impl fmt::Display for ActiveDlssClassificationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidTarget(path) => write!(formatter, "invalid active DLSS target: {path}"),
            Self::InvalidBundled(detail) => {
                write!(formatter, "bundled nvngx_dlss.dll is invalid: {detail}")
            }
            Self::InvalidLive { path, detail } => {
                write!(
                    formatter,
                    "live nvngx_dlss.dll is invalid at {path}: {detail}"
                )
            }
            Self::IncompatibleLive {
                path,
                live_version,
                bundled_version,
            } => write!(
                formatter,
                "live nvngx_dlss.dll at {path} is incompatible ({live_version} vs bundled {bundled_version})"
            ),
            Self::CatalogMissing(path) => {
                write!(
                    formatter,
                    "catalog-claimed nvngx_dlss.dll is missing at {path}"
                )
            }
            Self::CatalogDrift {
                path,
                expected,
                observed,
            } => write!(
                formatter,
                "catalog-claimed nvngx_dlss.dll drifted at {path}: observed {observed}, expected {expected:?}"
            ),
            Self::CatalogReplacementForbidden { path, reason } => write!(
                formatter,
                "catalog-owned nvngx_dlss.dll cannot be replaced at {path}: {reason}"
            ),
            Self::SnapshotImageMismatch(path) => {
                write!(
                    formatter,
                    "retained DLSS snapshot is internally inconsistent at {path}"
                )
            }
            Self::SnapshotMissingBytes(path) => {
                write!(formatter, "retained DLSS snapshot has no bytes at {path}")
            }
        }
    }
}

impl Error for ActiveDlssClassificationError {}

/// Failures after an owned classification has been selected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ActiveDlssLoweringError {
    SnapshotExpectedAbsent(PathRef),
    SnapshotExpectedFile(PathRef),
    SnapshotImageMismatch(PathRef),
    SnapshotMissingBytes(PathRef),
    Sidecar(PeerTransitionError),
    BindingMismatch(PathRef),
    Effects(LumaPeerEffectError),
}

impl fmt::Display for ActiveDlssLoweringError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SnapshotExpectedAbsent(path) => {
                write!(formatter, "expected an absent DLSS endpoint: {path}")
            }
            Self::SnapshotExpectedFile(path) => {
                write!(formatter, "expected a present DLSS endpoint: {path}")
            }
            Self::SnapshotImageMismatch(path) => {
                write!(
                    formatter,
                    "retained DLSS snapshot is internally inconsistent at {path}"
                )
            }
            Self::SnapshotMissingBytes(path) => {
                write!(formatter, "retained DLSS snapshot has no bytes at {path}")
            }
            Self::Sidecar(error) => error.fmt(formatter),
            Self::BindingMismatch(path) => {
                write!(formatter, "active DLSS owned binding does not match {path}")
            }
            Self::Effects(error) => error.fmt(formatter),
        }
    }
}

impl Error for ActiveDlssLoweringError {}

impl From<LumaPeerEffectError> for ActiveDlssLoweringError {
    fn from(error: LumaPeerEffectError) -> Self {
        Self::Effects(error)
    }
}
