//! Derivation of coordinated proxy-topology transitions.

use crate::{
    FileOwnership, InstalledAddon, ManagedFileBaseline, ManagedFileMode, NormalizedPathRelation,
    PathRef, ProxyImplementation, ProxyLink, ProxyRootPrestate, normalized_path_relation,
};

use super::super::EndpointGuard;
use super::super::claims::ManagedClaim;
use super::super::model::{
    CoordinatedPeerOperation, PeerEndpointIntent, PeerEndpointOperation, PeerEndpointRole,
    PeerTransitionContext, PeerTransitionError, ProxyPeerRoute,
};
use super::super::planning::{PlannedDownstream, PlannedTopologyShape};
use super::super::reconcile::DerivedEndpoint;
use super::helpers::{add_endpoint, managed_claim};
use super::reused;

#[derive(Clone, Copy)]
enum CoordinatedDownstream<'before, 'after> {
    Create {
        after: PlannedDownstream<'after>,
    },
    Replace {
        before: &'before ProxyLink,
        after: PlannedDownstream<'after>,
    },
    Remove {
        before: &'before ProxyLink,
        endpoint_operation: PeerEndpointOperation,
    },
}

impl<'before, 'after> CoordinatedDownstream<'before, 'after> {
    fn endpoint_operation(self) -> PeerEndpointOperation {
        match self {
            Self::Create { .. } => PeerEndpointOperation::Create,
            Self::Replace { .. } => PeerEndpointOperation::Replace,
            Self::Remove {
                endpoint_operation, ..
            } => endpoint_operation,
        }
    }

    fn before(self) -> Option<&'before ProxyLink> {
        match self {
            Self::Create { .. } => None,
            Self::Replace { before, .. } | Self::Remove { before, .. } => Some(before),
        }
    }

    fn after(self) -> Option<PlannedDownstream<'after>> {
        match self {
            Self::Create { after } | Self::Replace { after, .. } => Some(after),
            Self::Remove { .. } => None,
        }
    }
}

pub(super) fn derive(
    context: PeerTransitionContext<'_>,
    after: &PlannedTopologyShape,
    topology_path: Option<&PathRef>,
    physical_program: &[PeerEndpointIntent],
    endpoints: &mut Vec<DerivedEndpoint>,
) -> Result<(), PeerTransitionError> {
    let route = context.route;
    let before_peer = context.before_peer;
    let after_peer = context.after_peer;
    let before = context.before_topology;
    let ProxyPeerRoute::Coordinated(operation) = route else {
        if before != after.exact() {
            return Err(PeerTransitionError::ImmutableTopology);
        }
        return Ok(());
    };
    let Some(before_topology) = before else {
        return Err(PeerTransitionError::MissingTopologyImage(
            "coordinated route requires before and after topology",
        ));
    };
    let path = topology_path.ok_or(PeerTransitionError::MissingTopologyImage(
        "coordinated route has no downstream path",
    ))?;
    let before_downstream = before_topology.downstream.as_ref();
    let after_downstream = after.downstream();
    if !matches!(operation, CoordinatedPeerOperation::Remove)
        && Some(before_topology.root_prestate) != after.root_prestate()
    {
        return Err(PeerTransitionError::ImmutableTopology);
    }
    let coordinated = match operation {
        CoordinatedPeerOperation::Create => {
            let Some(after) = after_downstream else {
                return Err(PeerTransitionError::OperationMismatch(path.clone()));
            };
            if before_downstream.is_some() {
                return Err(PeerTransitionError::OperationMismatch(path.clone()));
            }
            CoordinatedDownstream::Create { after }
        }
        CoordinatedPeerOperation::ReplaceSamePath => {
            let (Some(before), Some(after)) = (before_downstream, after_downstream) else {
                return Err(PeerTransitionError::OperationMismatch(path.clone()));
            };
            if !matches!(
                normalized_path_relation(before.path.as_str(), after.path().as_str()),
                NormalizedPathRelation::Equal
            ) {
                return Err(PeerTransitionError::OperationMismatch(path.clone()));
            }
            CoordinatedDownstream::Replace { before, after }
        }
        CoordinatedPeerOperation::Remove => {
            let Some(before) = before_downstream else {
                return Err(PeerTransitionError::OperationMismatch(path.clone()));
            };
            if after_downstream.is_some()
                || after.root_prestate() != Some(ProxyRootPrestate::Absent)
                || after.downstream_origin().is_some()
            {
                return Err(PeerTransitionError::OperationMismatch(path.clone()));
            }
            let claim = managed_claim(before_peer, &before.path)
                .ok_or_else(|| PeerTransitionError::InvalidManagedDownstream(path.clone()))?;
            let endpoint_operation = match claim.baseline {
                ManagedFileBaseline::Absent => PeerEndpointOperation::Remove,
                ManagedFileBaseline::Present { .. } => PeerEndpointOperation::Replace,
            };
            CoordinatedDownstream::Remove {
                before,
                endpoint_operation,
            }
        }
    };
    let actual_operation = coordinated.endpoint_operation();
    let before_link = coordinated.before();
    let after_link = coordinated.after();
    if let Some(link) = before_link
        && !matches!(
            normalized_path_relation(link.path.as_str(), path.as_str()),
            NormalizedPathRelation::Equal
        )
    {
        return Err(PeerTransitionError::OperationMismatch(path.clone()));
    }
    if let Some(link) = after_link
        && !matches!(
            normalized_path_relation(link.path().as_str(), path.as_str()),
            NormalizedPathRelation::Equal
        )
    {
        return Err(PeerTransitionError::OperationMismatch(path.clone()));
    }
    match coordinated {
        CoordinatedDownstream::Create { after: after_link } => {
            require_owned_downstream(after_link, after_peer, true)?;
        }
        CoordinatedDownstream::Replace {
            before: before_link,
            after: after_link,
        } => {
            if before_topology.downstream_origin.as_ref() != after.downstream_origin() {
                return Err(PeerTransitionError::ImmutableTopology);
            }
            let after_claim = require_owned_downstream(after_link, after_peer, false)?;
            match before_link.receipt.ownership() {
                FileOwnership::Owned => {
                    let before_claim = require_owned_downstream(
                        PlannedDownstream::Exact(before_link),
                        before_peer,
                        false,
                    )?;
                    if before_claim.baseline != after_claim.baseline {
                        return Err(PeerTransitionError::InvalidManagedDownstream(path.clone()));
                    }
                }
                FileOwnership::Reused if before_peer.is_none() => {
                    if before_topology.outer.implementation != ProxyImplementation::OptiScaler
                        || before_link.implementation != ProxyImplementation::ReShade
                    {
                        return Err(PeerTransitionError::InvalidManagedDownstream(path.clone()));
                    }
                    reused::validate_initial_topology_acquisition(
                        before_link,
                        &after_claim,
                        physical_program,
                    )?;
                }
                FileOwnership::Reused => {
                    let before_claim =
                        managed_claim(before_peer, &before_link.path).ok_or_else(|| {
                            PeerTransitionError::InvalidManagedDownstream(path.clone())
                        })?;
                    if before_claim.mode != ManagedFileMode::Reused
                        || before_claim.installed != *before_link.receipt.digest()
                    {
                        return Err(PeerTransitionError::InvalidManagedDownstream(path.clone()));
                    }
                    if before_claim.baseline != after_claim.baseline {
                        return Err(PeerTransitionError::InvalidManagedDownstream(path.clone()));
                    }
                }
            }
        }
        CoordinatedDownstream::Remove {
            before: before_link,
            ..
        } => {
            require_owned_downstream(PlannedDownstream::Exact(before_link), before_peer, false)?;
            if managed_claim(after_peer, &before_link.path).is_some() {
                return Err(PeerTransitionError::InvalidManagedDownstream(path.clone()));
            }
        }
    }
    let planned_sha = after_link.map(|link| link.digest().clone());
    let before_sha = before_link.map(|link| link.receipt.digest().clone());
    let after_sha = match actual_operation {
        PeerEndpointOperation::Remove => None,
        PeerEndpointOperation::Replace if after_link.is_none() => {
            let claim = managed_claim(before_peer, path)
                .ok_or_else(|| PeerTransitionError::InvalidManagedDownstream(path.clone()))?;
            match claim.baseline {
                ManagedFileBaseline::Absent => None,
                ManagedFileBaseline::Present { sha256 } => Some(sha256),
            }
        }
        _ => planned_sha,
    };
    if after_link.is_some_and(|link| link.implementation() != ProxyImplementation::ReShade) {
        return Err(PeerTransitionError::InvalidPlannedTopology(
            "planned downstream is not the peer host implementation",
        ));
    }
    let after_length = after_link.and_then(PlannedDownstream::length);
    let intent = PeerEndpointIntent::new(
        path.clone(),
        PeerEndpointRole::TopologyDownstream,
        actual_operation,
        after_sha.clone(),
        None,
    )?;
    add_endpoint(
        endpoints,
        intent,
        EndpointGuard {
            before_sha256: before_sha,
            after_sha256: after_sha,
            after_length,
            ..EndpointGuard::default()
        },
    );
    Ok(())
}

fn require_owned_downstream(
    link: PlannedDownstream<'_>,
    peer: Option<&InstalledAddon>,
    require_absent_baseline: bool,
) -> Result<ManagedClaim, PeerTransitionError> {
    if link.ownership() != FileOwnership::Owned {
        return Err(PeerTransitionError::InvalidManagedDownstream(
            link.path().clone(),
        ));
    }
    let claim = managed_claim(peer, link.path())
        .ok_or_else(|| PeerTransitionError::InvalidManagedDownstream(link.path().clone()))?;
    if claim.mode != ManagedFileMode::Owned
        || (require_absent_baseline && !matches!(claim.baseline, ManagedFileBaseline::Absent))
        || claim.installed != *link.digest()
    {
        return Err(PeerTransitionError::InvalidManagedDownstream(
            link.path().clone(),
        ));
    }
    Ok(claim)
}
