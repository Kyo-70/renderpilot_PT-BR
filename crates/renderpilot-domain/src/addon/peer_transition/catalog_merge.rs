//! Merge catalog-derived endpoint constraints with peer derivation.

use crate::normalized_path_key;

use super::EndpointGuard;
use super::catalog_physical::PeerCatalogPhysicalContract;
use super::model::{PeerEndpointIntent, PeerTransitionError};
use super::reconcile::DerivedEndpoint;

pub(super) fn merge(
    mut peer: Vec<DerivedEndpoint>,
    catalog: &PeerCatalogPhysicalContract,
    before_peer: Option<&crate::InstalledAddon>,
    after_peer: Option<&crate::InstalledAddon>,
) -> Result<Vec<DerivedEndpoint>, PeerTransitionError> {
    for endpoint in catalog.endpoints() {
        let key = normalized_path_key(endpoint.intent.path().as_str());
        if let Some(existing) = peer
            .iter_mut()
            .find(|candidate| normalized_path_key(candidate.intent.path().as_str()) == key)
        {
            merge_endpoint(existing, endpoint)?;
        } else if catalog.requires_peer_endpoint(endpoint.intent.path()) {
            return Err(PeerTransitionError::CatalogBaselineSatisfactionMismatch(
                endpoint.intent.path().clone(),
            ));
        } else {
            peer.push(endpoint.clone());
        }
    }

    for satisfaction in catalog.satisfactions() {
        let path_key = normalized_path_key(satisfaction.live_path.as_str());
        let Some(index) = peer
            .iter()
            .position(|endpoint| normalized_path_key(endpoint.intent.path().as_str()) == path_key)
        else {
            if catalog.requires_peer_endpoint(&satisfaction.live_path) {
                return Err(PeerTransitionError::CatalogBaselineSatisfactionMismatch(
                    satisfaction.live_path.clone(),
                ));
            }
            continue;
        };
        let path = peer[index].intent.path().clone();
        let endpoint = &peer[index];
        if endpoint.intent.role() == super::model::PeerEndpointRole::TopologyDownstream {
            return Err(PeerTransitionError::CatalogBaselineSatisfactionMismatch(
                path,
            ));
        }
        if catalog.can_discharge_peer_release(&path, before_peer, after_peer, endpoint) {
            peer.remove(index);
        }
    }
    Ok(peer)
}

fn merge_endpoint(
    current: &mut DerivedEndpoint,
    incoming: &DerivedEndpoint,
) -> Result<(), PeerTransitionError> {
    if current.intent.operation() != incoming.intent.operation()
        || current.intent.role() != incoming.intent.role()
    {
        return Err(PeerTransitionError::PhysicalProgramMismatch(
            incoming.intent.path().clone(),
        ));
    }
    let digest = merge_option(
        current.intent.planned_sha256(),
        incoming.intent.planned_sha256(),
        incoming.intent.path(),
    )?;
    let length = merge_option(
        current.intent.planned_length().as_ref(),
        incoming.intent.planned_length().as_ref(),
        incoming.intent.path(),
    )?;
    current.intent = PeerEndpointIntent::new(
        current.intent.path().clone(),
        current.intent.role(),
        current.intent.operation(),
        digest,
        length,
    )?;
    current.guard = merge_guards(&current.guard, &incoming.guard, incoming.intent.path())?;
    Ok(())
}

fn merge_option<T: Clone + PartialEq>(
    current: Option<&T>,
    incoming: Option<&T>,
    path: &crate::PathRef,
) -> Result<Option<T>, PeerTransitionError> {
    match (current, incoming) {
        (Some(left), Some(right)) if left != right => {
            Err(PeerTransitionError::PhysicalProgramMismatch(path.clone()))
        }
        (Some(value), _) | (_, Some(value)) => Ok(Some(value.clone())),
        (None, None) => Ok(None),
    }
}

fn merge_guards(
    current: &EndpointGuard,
    incoming: &EndpointGuard,
    path: &crate::PathRef,
) -> Result<EndpointGuard, PeerTransitionError> {
    Ok(EndpointGuard {
        before_sha256: merge_option(
            current.before_sha256.as_ref(),
            incoming.before_sha256.as_ref(),
            path,
        )?,
        before_length: merge_option(
            current.before_length.as_ref(),
            incoming.before_length.as_ref(),
            path,
        )?,
        after_sha256: merge_option(
            current.after_sha256.as_ref(),
            incoming.after_sha256.as_ref(),
            path,
        )?,
        after_length: merge_option(
            current.after_length.as_ref(),
            incoming.after_length.as_ref(),
            path,
        )?,
    })
}

pub(super) fn bind(
    contract: &PeerCatalogPhysicalContract,
    before_peer: Option<&crate::InstalledAddon>,
    after_peer: Option<&crate::InstalledAddon>,
    physical_program: &[PeerEndpointIntent],
) -> Result<(), PeerTransitionError> {
    if !contract.matches_inputs(before_peer, after_peer, physical_program) {
        return Err(PeerTransitionError::InvalidPeerSnapshot(
            "catalog physical contract was derived from different inputs",
        ));
    }
    Ok(())
}
