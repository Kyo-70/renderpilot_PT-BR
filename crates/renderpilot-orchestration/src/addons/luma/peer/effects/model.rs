use renderpilot_domain::{PathRef, PeerEndpointRole};

use crate::peer_mutation_executor::{EndpointExpectation, EndpointPostcondition};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LumaPeerOperationOrder {
    InstallOrUpdate,
    Uninstall,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LumaPeerEffectGroup {
    Generic,
    DgVoodoo,
    DlssCascade,
    Host,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct EndpointSpec {
    pub(super) path: PathRef,
    pub(super) role: PeerEndpointRole,
    pub(super) before: EndpointExpectation,
    pub(super) after: EndpointPostcondition,
    pub(super) payload: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum EndpointBundle {
    Single {
        group: LumaPeerEffectGroup,
        endpoint: EndpointSpec,
    },
    Acquisition {
        group: LumaPeerEffectGroup,
        sidecar: EndpointSpec,
        live: EndpointSpec,
    },
    Release {
        group: LumaPeerEffectGroup,
        live: EndpointSpec,
        sidecar: EndpointSpec,
    },
    /// Restore a missing live endpoint from its exact managed sidecar, then
    /// release the sidecar.  This is intentionally separate from `Release`:
    /// the live endpoint is acquired from `Absent`, not replaced in place.
    RestoreAbsent {
        group: LumaPeerEffectGroup,
        live: EndpointSpec,
        sidecar: EndpointSpec,
    },
}

impl EndpointBundle {
    pub(super) fn group(&self) -> LumaPeerEffectGroup {
        match self {
            Self::Single { group, .. }
            | Self::Acquisition { group, .. }
            | Self::Release { group, .. }
            | Self::RestoreAbsent { group, .. } => *group,
        }
    }

    pub(super) fn logical_path(&self) -> &PathRef {
        match self {
            Self::Single { endpoint, .. } => &endpoint.path,
            Self::Acquisition { live, .. }
            | Self::Release { live, .. }
            | Self::RestoreAbsent { live, .. } => &live.path,
        }
    }

    pub(super) fn len(&self) -> usize {
        match self {
            Self::Single { .. } => 1,
            Self::Acquisition { .. } | Self::Release { .. } | Self::RestoreAbsent { .. } => 2,
        }
    }

    pub(super) fn visit(&self, mut visitor: impl FnMut(&EndpointSpec)) {
        match self {
            Self::Single { endpoint, .. } => visitor(endpoint),
            Self::Acquisition { sidecar, live, .. } => {
                visitor(sidecar);
                visitor(live);
            }
            Self::Release { live, sidecar, .. } | Self::RestoreAbsent { live, sidecar, .. } => {
                visitor(live);
                visitor(sidecar);
            }
        }
    }

    pub(super) fn visit_owned(self, mut visitor: impl FnMut(EndpointSpec)) {
        match self {
            Self::Single { endpoint, .. } => visitor(endpoint),
            Self::Acquisition { sidecar, live, .. } => {
                visitor(sidecar);
                visitor(live);
            }
            Self::Release { live, sidecar, .. } | Self::RestoreAbsent { live, sidecar, .. } => {
                visitor(live);
                visitor(sidecar);
            }
        }
    }

    pub(super) fn live_role(&self) -> PeerEndpointRole {
        match self {
            Self::Single { endpoint, .. } => endpoint.role,
            Self::Acquisition { live, .. }
            | Self::Release { live, .. }
            | Self::RestoreAbsent { live, .. } => live.role,
        }
    }
}
