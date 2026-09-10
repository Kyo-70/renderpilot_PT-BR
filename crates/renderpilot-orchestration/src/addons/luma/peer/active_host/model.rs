use std::{error::Error, fmt};

use renderpilot_domain::{
    ManagedAddonFile, PathRef, PeerTransitionError, PlannedGameProxyTopology, managed_sidecar_path,
};

use crate::addons::luma::peer::{
    effects::LumaPeerEffectError, host::LumaHostLoweringError,
    snapshot_input::LumaSnapshotInputError,
};

/// A safe initial active-host result. Reused hosts expose only their managed
/// binding and unchanged topology; they cannot request a sidecar or become a
/// mutation target accidentally.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ActiveHostClassification {
    Reused {
        binding: ManagedAddonFile,
        planned_topology: PlannedGameProxyTopology,
    },
    Owned(ActiveHostOwnedPlan),
}

impl ActiveHostClassification {
    pub(crate) fn binding(&self) -> &ManagedAddonFile {
        match self {
            Self::Reused { binding, .. } => binding,
            Self::Owned(plan) => plan.binding(),
        }
    }

    pub(crate) fn planned_topology(&self) -> &PlannedGameProxyTopology {
        match self {
            Self::Reused {
                planned_topology, ..
            } => planned_topology,
            Self::Owned(plan) => plan.planned_topology(),
        }
    }

    pub(crate) fn owned(&self) -> Option<&ActiveHostOwnedPlan> {
        match self {
            Self::Reused { .. } => None,
            Self::Owned(plan) => Some(plan),
        }
    }

    pub(crate) fn into_owned(self) -> Option<ActiveHostOwnedPlan> {
        match self {
            Self::Reused { .. } => None,
            Self::Owned(plan) => Some(plan),
        }
    }
}

/// A complete owned active-host transition. The sidecar path is only
/// applicable to the F4 replacement variant, where the lowerer validates its
/// retained absent snapshot immediately before emitting the transition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ActiveHostOwnedPlan {
    pub(super) target: PathRef,
    pub(super) binding: ManagedAddonFile,
    pub(super) prepared_bytes: Vec<u8>,
    pub(super) transition: ActiveHostTransition,
    pub(super) planned_topology: PlannedGameProxyTopology,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ActiveHostTransition {
    Create,
    Replace {
        live_digest: renderpilot_domain::Sha256Hash,
    },
}

impl ActiveHostOwnedPlan {
    pub(crate) fn binding(&self) -> &ManagedAddonFile {
        &self.binding
    }

    pub(crate) fn planned_topology(&self) -> &PlannedGameProxyTopology {
        &self.planned_topology
    }

    /// Returns a sidecar request only for the owned F4 replacement. A fresh
    /// create has no sidecar endpoint and must not observe a foreign `.bak`.
    pub(crate) fn sidecar_path(&self) -> Result<PathRef, ActiveHostLoweringError> {
        match self.transition {
            ActiveHostTransition::Replace { .. } => {
                managed_sidecar_path(&self.target).map_err(ActiveHostLoweringError::Sidecar)
            }
            ActiveHostTransition::Create => Err(ActiveHostLoweringError::SidecarNotApplicable),
        }
    }

    pub(crate) fn is_create(&self) -> bool {
        matches!(self.transition, ActiveHostTransition::Create)
    }
}

/// Fail-closed errors raised while correlating topology, assessment, and the
/// retained exact host snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ActiveHostClassificationError {
    Topology(&'static str),
    TopologyValidation(String),
    Path {
        expected: PathRef,
        observed: PathRef,
    },
    Assessment(&'static str),
    Evidence {
        path: PathRef,
        detail: &'static str,
    },
    Prepared(&'static str),
    PreparedDetail(String),
}

impl fmt::Display for ActiveHostClassificationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Topology(reason) => {
                write!(formatter, "invalid active Luma topology: {reason}")
            }
            Self::TopologyValidation(reason) => {
                write!(formatter, "invalid active Luma topology: {reason}")
            }
            Self::Path { expected, observed } => write!(
                formatter,
                "active Luma host path {observed} is not the topology ReShade path {expected}"
            ),
            Self::Assessment(reason) => {
                write!(formatter, "invalid active host assessment: {reason}")
            }
            Self::Evidence { path, detail } => {
                write!(
                    formatter,
                    "active host evidence mismatch at {path}: {detail}"
                )
            }
            Self::Prepared(reason) => {
                write!(formatter, "invalid prepared Luma ReShade host: {reason}")
            }
            Self::PreparedDetail(reason) => {
                write!(formatter, "invalid prepared Luma ReShade host: {reason}")
            }
        }
    }
}

impl Error for ActiveHostClassificationError {}

/// Failures after an owned transition has been selected, before an effect is
/// appended. The accumulator remains unchanged on every error path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ActiveHostLoweringError {
    Snapshot(LumaSnapshotInputError),
    SnapshotImageMismatch(PathRef),
    Sidecar(PeerTransitionError),
    SidecarNotApplicable,
    BindingMismatch(PathRef),
    Effects(LumaPeerEffectError),
}

impl fmt::Display for ActiveHostLoweringError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Snapshot(error) => error.fmt(formatter),
            Self::SnapshotImageMismatch(path) => {
                write!(
                    formatter,
                    "retained active host snapshot is internally inconsistent at {path}"
                )
            }
            Self::Sidecar(error) => error.fmt(formatter),
            Self::SidecarNotApplicable => {
                formatter.write_str("a fresh active host create has no managed sidecar")
            }
            Self::BindingMismatch(path) => {
                write!(formatter, "active host owned binding does not match {path}")
            }
            Self::Effects(error) => error.fmt(formatter),
        }
    }
}

impl Error for ActiveHostLoweringError {}

impl From<LumaSnapshotInputError> for ActiveHostLoweringError {
    fn from(error: LumaSnapshotInputError) -> Self {
        Self::Snapshot(error)
    }
}

impl From<PeerTransitionError> for ActiveHostLoweringError {
    fn from(error: PeerTransitionError) -> Self {
        Self::Sidecar(error)
    }
}

impl From<LumaPeerEffectError> for ActiveHostLoweringError {
    fn from(error: LumaPeerEffectError) -> Self {
        Self::Effects(error)
    }
}

impl From<LumaHostLoweringError> for ActiveHostLoweringError {
    fn from(error: LumaHostLoweringError) -> Self {
        match error {
            LumaHostLoweringError::Snapshot(error) => Self::Snapshot(error),
            LumaHostLoweringError::Effects(error) => Self::Effects(error),
        }
    }
}
