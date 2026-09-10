use renderpilot_domain::{PathRef, managed_sidecar_path, normalized_path_key};

use crate::{
    addons::luma::peer::active_payload::ValidatedActivePayloadTarget,
    peer_mutation_executor::PeerPathSnapshot,
};

use super::super::model::LumaActiveUpdateClaimDelta;

#[derive(Debug)]
pub(super) struct FreshTarget {
    pub(super) live: PathRef,
    pub(super) sidecar: PathRef,
    pub(super) bytes: Vec<u8>,
}

impl FreshTarget {
    pub(super) fn from_validated(target: ValidatedActivePayloadTarget) -> Self {
        let (live, sidecar, bytes) = target.into_parts();
        Self {
            live,
            sidecar,
            bytes,
        }
    }
}

#[derive(Debug)]
pub(super) enum Candidate {
    Fresh {
        fresh_live: PathRef,
        sidecar: PathRef,
        bytes: Vec<u8>,
        retained_live: Option<PathRef>,
        backed: bool,
        backed_path: Option<PathRef>,
    },
    Removed {
        live: PathRef,
        sidecar: PathRef,
        backed: bool,
        backed_path: Option<PathRef>,
    },
}

impl Candidate {
    pub(super) fn live(&self) -> &PathRef {
        match self {
            Self::Fresh {
                fresh_live,
                retained_live,
                ..
            } => retained_live.as_ref().unwrap_or(fresh_live),
            Self::Removed { live, .. } => live,
        }
    }

    pub(super) fn sidecar(&self) -> &PathRef {
        match self {
            Self::Fresh { sidecar, .. } | Self::Removed { sidecar, .. } => sidecar,
        }
    }

    pub(super) fn bytes(&self) -> Option<&[u8]> {
        match self {
            Self::Fresh { bytes, .. } => Some(bytes),
            Self::Removed { .. } => None,
        }
    }

    pub(super) fn key(&self) -> String {
        normalized_path_key(self.live().as_str())
    }

    pub(super) fn is_new(&self) -> bool {
        matches!(
            self,
            Self::Fresh {
                retained_live: None,
                ..
            }
        )
    }

    pub(super) fn backed_path(&self) -> Option<&PathRef> {
        match self {
            Self::Fresh { backed_path, .. } | Self::Removed { backed_path, .. } => {
                backed_path.as_ref()
            }
        }
    }

    pub(super) fn is_main(&self, main_addon: &PathRef) -> bool {
        matches!(self, Self::Fresh { .. })
            && normalized_path_key(self.live().as_str()) == normalized_path_key(main_addon.as_str())
    }

    pub(super) fn expected_sidecar_for(
        live: &PathRef,
    ) -> Result<PathRef, renderpilot_domain::PeerTransitionError> {
        managed_sidecar_path(live)
    }
}

#[derive(Debug)]
pub(super) struct CandidatePlan {
    pub(super) candidates: Vec<Candidate>,
}

#[derive(Debug)]
pub(super) struct ObservedCandidate {
    pub(super) live: PeerPathSnapshot,
    pub(super) sidecar: PeerPathSnapshot,
}

#[derive(Debug, Clone, Copy)]
pub(super) enum DecisionKind {
    Unchanged,
    Create,
    AcquireForeign,
    ReplaceOwned,
    RemoveCreated,
    ReleaseBacked,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct PlannedDecision {
    pub(super) candidate: usize,
    pub(super) kind: DecisionKind,
}

#[derive(Debug)]
pub(super) struct ClassifiedPayload {
    pub(super) decisions: Vec<PlannedDecision>,
    pub(super) delta: LumaActiveUpdateClaimDelta,
    pub(super) main_changed_path: Option<PathRef>,
}
