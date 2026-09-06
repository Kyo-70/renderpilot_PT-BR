//! Shared path, claim, and endpoint helpers for derivation.

use crate::{GameProxyTopology, InstalledAddon, PathRef, normalized_path_key};

use super::super::EndpointGuard;
use super::super::claims::ManagedClaim;
use super::super::model::{
    CoordinatedPeerOperation, PeerEndpointIntent, PeerTransitionError, ProxyPeerRoute,
};
use super::super::planning::PlannedTopologyShape;
use super::super::reconcile::DerivedEndpoint;

pub(super) fn coordinated_path(
    route: ProxyPeerRoute,
    before: Option<&GameProxyTopology>,
    after: &PlannedTopologyShape,
) -> Result<Option<PathRef>, PeerTransitionError> {
    let ProxyPeerRoute::Coordinated(operation) = route else {
        return Ok(None);
    };
    match operation {
        CoordinatedPeerOperation::Create | CoordinatedPeerOperation::ReplaceSamePath => after
            .downstream()
            .map(|link| link.path().clone())
            .or_else(|| {
                before
                    .and_then(|topology| topology.downstream.as_ref())
                    .map(|link| link.path.clone())
            })
            .map(Some)
            .ok_or(PeerTransitionError::MissingTopologyImage("downstream")),
        CoordinatedPeerOperation::Remove => before
            .and_then(|topology| topology.downstream.as_ref())
            .map(|link| Some(link.path.clone()))
            .ok_or(PeerTransitionError::MissingTopologyImage("downstream")),
    }
}

pub(super) fn managed_claim(peer: Option<&InstalledAddon>, path: &PathRef) -> Option<ManagedClaim> {
    let key = normalized_path_key(path.as_str());
    peer?.managed_files().iter().find_map(|file| {
        (normalized_path_key(file.path().as_str()) == key).then(|| ManagedClaim::from_file(file))
    })
}

pub(super) fn add_endpoint(
    endpoints: &mut Vec<DerivedEndpoint>,
    intent: PeerEndpointIntent,
    guard: EndpointGuard,
) {
    endpoints.push(DerivedEndpoint { intent, guard });
}
