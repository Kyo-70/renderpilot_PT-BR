use renderpilot_domain::{
    ManagedAddonFile, PathRef, PeerTransitionError, PlannedGameProxyTopology,
};

/// Host classification for one active RenoDX install.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RenoDxHostClassification {
    /// The exact existing downstream is suitable and remains untouched.
    Reused {
        binding: ManagedAddonFile,
        planned_topology: PlannedGameProxyTopology,
    },
    /// RenoDX acquires or repairs the exact downstream host.
    Owned(RenoDxOwnedHostPlan),
}

impl RenoDxHostClassification {
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RenoDxOwnedHostPlan {
    pub(super) target: PathRef,
    pub(super) binding: ManagedAddonFile,
    pub(super) prepared_bytes: Vec<u8>,
    pub(super) transition: RenoDxHostTransition,
    pub(super) planned_topology: PlannedGameProxyTopology,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum RenoDxHostTransition {
    Create,
    Replace {
        live_digest: renderpilot_domain::Sha256Hash,
    },
}

impl RenoDxOwnedHostPlan {
    pub(crate) fn binding(&self) -> &ManagedAddonFile {
        &self.binding
    }

    pub(crate) fn planned_topology(&self) -> &PlannedGameProxyTopology {
        &self.planned_topology
    }

    pub(crate) fn target(&self) -> &PathRef {
        &self.target
    }

    pub(crate) fn prepared_bytes(self) -> Vec<u8> {
        self.prepared_bytes
    }

    pub(crate) fn sidecar_path(&self) -> Result<PathRef, PeerTransitionError> {
        match self.transition {
            RenoDxHostTransition::Replace { .. } => {
                renderpilot_domain::managed_sidecar_path(&self.target)
            }
            RenoDxHostTransition::Create => {
                Err(PeerTransitionError::InvalidRenoDxReshadeIniTransition(
                    "a fresh RenoDX host has no managed sidecar",
                ))
            }
        }
    }

    pub(crate) fn is_create(&self) -> bool {
        matches!(self.transition, RenoDxHostTransition::Create)
    }
}
