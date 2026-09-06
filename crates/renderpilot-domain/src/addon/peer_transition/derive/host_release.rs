//! Ordered release rules for a coordinated managed host.

use crate::{
    GameProxyTopology, InstalledAddon, ManagedFileBaseline, ManagedFileMode,
    NormalizedPathRelation, normalized_path_key, normalized_path_relation,
};

use super::super::claims::managed_sidecar_path;
use super::super::model::{
    CoordinatedPeerOperation, PeerEndpointIntent, PeerEndpointOperation, PeerEndpointRole,
    PeerTransitionError, ProxyPeerRoute,
};

/// Validates the host release pair before physical reconciliation.
pub(super) fn validate(
    route: ProxyPeerRoute,
    before_peer: Option<&InstalledAddon>,
    before_topology: Option<&GameProxyTopology>,
    physical_program: &[PeerEndpointIntent],
) -> Result<(), PeerTransitionError> {
    if route != ProxyPeerRoute::Coordinated(CoordinatedPeerOperation::Remove) {
        return Ok(());
    }
    let Some(topology) = before_topology else {
        return Ok(());
    };
    let Some(downstream) = topology.downstream.as_ref() else {
        return Ok(());
    };
    let Some(peer) = before_peer else {
        return Ok(());
    };
    let Some(claim) = peer.managed_files().iter().find(|file| {
        normalized_path_key(file.path().as_str()) == normalized_path_key(downstream.path.as_str())
    }) else {
        return Ok(());
    };
    if claim.mode() != ManagedFileMode::Owned {
        return Ok(());
    }

    let topology_index = physical_program.iter().position(|intent| {
        intent.role() == PeerEndpointRole::TopologyDownstream
            && matches!(
                normalized_path_relation(intent.path().as_str(), downstream.path.as_str()),
                NormalizedPathRelation::Equal
            )
    });
    let Some(topology_index) = topology_index else {
        return Err(PeerTransitionError::PhysicalProgramMismatch(
            downstream.path.clone(),
        ));
    };
    let topology_intent = &physical_program[topology_index];
    let sidecar = managed_sidecar_path(claim.path())?;

    match claim.baseline() {
        ManagedFileBaseline::Absent => {
            if topology_intent.operation() != PeerEndpointOperation::Remove
                || physical_program.iter().any(|intent| {
                    matches!(
                        normalized_path_relation(intent.path().as_str(), sidecar.as_str()),
                        NormalizedPathRelation::Equal
                    )
                })
            {
                return Err(PeerTransitionError::PhysicalProgramMismatch(
                    downstream.path.clone(),
                ));
            }
        }
        ManagedFileBaseline::Present { .. } => {
            let Some(sidecar_index) = physical_program.iter().position(|intent| {
                intent.role() == PeerEndpointRole::Disjoint
                    && intent.operation() == PeerEndpointOperation::Remove
                    && matches!(
                        normalized_path_relation(intent.path().as_str(), sidecar.as_str()),
                        NormalizedPathRelation::Equal
                    )
            }) else {
                return Err(PeerTransitionError::PhysicalProgramMismatch(sidecar));
            };
            if topology_intent.operation() != PeerEndpointOperation::Replace
                || sidecar_index != topology_index + 1
            {
                return Err(PeerTransitionError::PhysicalProgramMismatch(
                    downstream.path.clone(),
                ));
            }
        }
    }
    Ok(())
}
