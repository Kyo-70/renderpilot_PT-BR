//! Receipt-free route and planned topology shape reconciliation.

use crate::{
    FileOwnership, GameId, GameProxyTopology, PathRef, ProxyImplementation, ProxyLink,
    ProxyRootPrestate, Sha256Hash, normalized_path_key,
};

use super::model::{
    CoordinatedPeerOperation, PeerTransitionError, PlannedGameProxyTopology, ProxyPeerRoute,
};

#[derive(Debug, Clone)]
pub(super) enum PlannedTopologyShape {
    Absent,
    Exact(GameProxyTopology),
    Observed(ObservedTopology),
}

#[derive(Debug, Clone)]
pub(super) struct ObservedTopology {
    pub(super) id: String,
    pub(super) game_id: GameId,
    pub(super) root_slot: PathRef,
    pub(super) outer: ProxyLink,
    pub(super) downstream: ObservedDownstream,
    pub(super) downstream_origin: PathRef,
    pub(super) root_prestate: ProxyRootPrestate,
}

#[derive(Debug, Clone)]
pub(super) struct ObservedDownstream {
    pub(super) implementation: ProxyImplementation,
    pub(super) path: PathRef,
    pub(super) planned_sha256: Sha256Hash,
    pub(super) planned_length: u64,
}

#[derive(Debug, Clone, Copy)]
pub(super) enum PlannedDownstream<'a> {
    Exact(&'a ProxyLink),
    Observed(&'a ObservedDownstream),
}

impl<'a> PlannedDownstream<'a> {
    pub(super) fn path(self) -> &'a PathRef {
        match self {
            Self::Exact(link) => &link.path,
            Self::Observed(link) => &link.path,
        }
    }

    pub(super) fn digest(self) -> &'a Sha256Hash {
        match self {
            Self::Exact(link) => link.receipt.digest(),
            Self::Observed(link) => &link.planned_sha256,
        }
    }

    pub(super) fn implementation(self) -> ProxyImplementation {
        match self {
            Self::Exact(link) => link.implementation,
            Self::Observed(link) => link.implementation,
        }
    }

    pub(super) fn ownership(self) -> FileOwnership {
        match self {
            Self::Exact(link) => link.receipt.ownership(),
            Self::Observed(_) => FileOwnership::Owned,
        }
    }

    pub(super) fn length(self) -> Option<u64> {
        match self {
            Self::Exact(_) => None,
            Self::Observed(link) => Some(link.planned_length),
        }
    }
}

impl PlannedTopologyShape {
    pub(super) fn downstream(&self) -> Option<PlannedDownstream<'_>> {
        match self {
            Self::Absent => None,
            Self::Exact(topology) => topology.downstream.as_ref().map(PlannedDownstream::Exact),
            Self::Observed(topology) => Some(PlannedDownstream::Observed(&topology.downstream)),
        }
    }

    pub(super) fn root_slot(&self) -> Option<&PathRef> {
        match self {
            Self::Absent => None,
            Self::Exact(topology) => Some(&topology.root_slot),
            Self::Observed(topology) => Some(&topology.root_slot),
        }
    }

    pub(super) fn downstream_origin(&self) -> Option<&PathRef> {
        match self {
            Self::Absent => None,
            Self::Exact(topology) => topology.downstream_origin.as_ref(),
            Self::Observed(topology) => Some(&topology.downstream_origin),
        }
    }

    pub(super) fn root_prestate(&self) -> Option<ProxyRootPrestate> {
        match self {
            Self::Absent => None,
            Self::Exact(topology) => Some(topology.root_prestate),
            Self::Observed(topology) => Some(topology.root_prestate),
        }
    }

    pub(super) fn exact(&self) -> Option<&GameProxyTopology> {
        match self {
            Self::Exact(topology) => Some(topology),
            Self::Absent | Self::Observed(_) => None,
        }
    }

    pub(super) fn participant_paths(&self) -> Vec<&PathRef> {
        let mut paths = Vec::new();
        if let Some(root) = self.root_slot() {
            paths.push(root);
        }
        if let Some(downstream) = self.downstream() {
            paths.push(downstream.path());
        }
        if let Some(origin) = self.downstream_origin() {
            paths.push(origin);
        }
        paths
    }
}

/// Builds a topology shape without constructing a synthetic file receipt.  The
/// observed downstream digest/length remain planning assertions; its native
/// identity is created only by the evidence materializer.
pub(super) fn topology_shape(
    before: Option<&GameProxyTopology>,
    planned: Option<&PlannedGameProxyTopology>,
    route: ProxyPeerRoute,
) -> Result<PlannedTopologyShape, PeerTransitionError> {
    match route {
        ProxyPeerRoute::DurableDisjoint => match (before, planned) {
            (None, None) => Ok(PlannedTopologyShape::Absent),
            (Some(before), Some(PlannedGameProxyTopology::Exact(after))) if before == after => {
                Ok(PlannedTopologyShape::Exact(after.clone()))
            }
            (Some(_), Some(PlannedGameProxyTopology::ObservedOwnedDownstream { .. })) => {
                Err(PeerTransitionError::InvalidPlannedTopology(
                    "disjoint route cannot observe a new downstream",
                ))
            }
            (None, Some(_)) | (Some(_), None) => Err(PeerTransitionError::InvalidPlannedTopology(
                "topology presence must be represented on both sides",
            )),
            (Some(_), Some(PlannedGameProxyTopology::Exact(_))) => {
                Err(PeerTransitionError::InvalidPlannedTopology(
                    "exact topology changed on a disjoint route",
                ))
            }
        },
        ProxyPeerRoute::Coordinated(operation) => {
            if before.is_none() {
                return Err(PeerTransitionError::MissingTopologyImage(
                    "coordinated route requires before topology",
                ));
            }
            let Some(planned) = planned else {
                return Err(PeerTransitionError::MissingTopologyImage(
                    "coordinated route requires planned after topology",
                ));
            };
            match (operation, planned) {
                (
                    CoordinatedPeerOperation::Create | CoordinatedPeerOperation::ReplaceSamePath,
                    PlannedGameProxyTopology::ObservedOwnedDownstream {
                        id,
                        game_id,
                        root_slot,
                        outer,
                        implementation,
                        downstream_path,
                        downstream_origin,
                        root_prestate,
                        planned_sha256,
                        planned_length,
                    },
                ) => {
                    if *implementation != ProxyImplementation::ReShade {
                        return Err(PeerTransitionError::InvalidPlannedTopology(
                            "planned downstream is not the peer host implementation",
                        ));
                    }
                    Ok(PlannedTopologyShape::Observed(ObservedTopology {
                        id: id.clone(),
                        game_id: game_id.clone(),
                        root_slot: root_slot.clone(),
                        outer: outer.clone(),
                        downstream: ObservedDownstream {
                            implementation: *implementation,
                            path: downstream_path.clone(),
                            planned_sha256: planned_sha256.clone(),
                            planned_length: *planned_length,
                        },
                        downstream_origin: downstream_origin.clone(),
                        root_prestate: *root_prestate,
                    }))
                }
                (CoordinatedPeerOperation::Remove, PlannedGameProxyTopology::Exact(after)) => {
                    if after.downstream.is_some() {
                        return Err(PeerTransitionError::InvalidPlannedTopology(
                            "remove planned topology must omit downstream",
                        ));
                    }
                    Ok(PlannedTopologyShape::Exact(after.clone()))
                }
                (
                    CoordinatedPeerOperation::Create | CoordinatedPeerOperation::ReplaceSamePath,
                    PlannedGameProxyTopology::Exact(_),
                ) => Err(PeerTransitionError::InvalidPlannedTopology(
                    "coordinated create or replace requires observed downstream",
                )),
                (
                    CoordinatedPeerOperation::Remove,
                    PlannedGameProxyTopology::ObservedOwnedDownstream { .. },
                ) => Err(PeerTransitionError::InvalidPlannedTopology(
                    "coordinated remove requires exact topology",
                )),
            }
        }
    }
}

pub(super) fn validate_shape(
    before: Option<&GameProxyTopology>,
    shape: &PlannedTopologyShape,
) -> Result<(), PeerTransitionError> {
    match shape {
        PlannedTopologyShape::Absent => Ok(()),
        PlannedTopologyShape::Exact(topology) => topology
            .validate()
            .map_err(|error| PeerTransitionError::InvalidTopologySnapshot(error.to_string())),
        PlannedTopologyShape::Observed(topology) => {
            topology.outer.receipt.validate().map_err(|_| {
                PeerTransitionError::InvalidTopologySnapshot(
                    "planned outer receipt is invalid".to_owned(),
                )
            })?;
            if topology.id.trim().is_empty()
                || normalized_path_key(topology.root_slot.as_str())
                    != normalized_path_key(topology.outer.path.as_str())
                || normalized_path_key(topology.downstream.path.as_str())
                    == normalized_path_key(topology.root_slot.as_str())
                || normalized_path_key(topology.downstream.path.as_str())
                    == normalized_path_key(topology.downstream_origin.as_str())
                || normalized_path_key(topology.downstream_origin.as_str())
                    != normalized_path_key(topology.root_slot.as_str())
            {
                return Err(PeerTransitionError::InvalidPlannedTopology(
                    "planned topology paths are inconsistent",
                ));
            }
            if let Some(before) = before
                && (before.id != topology.id
                    || before.game_id != topology.game_id
                    || before.root_slot != topology.root_slot
                    || before.outer != topology.outer
                    || before.root_prestate != topology.root_prestate)
            {
                return Err(PeerTransitionError::ImmutableTopology);
            }
            Ok(())
        }
    }
}
