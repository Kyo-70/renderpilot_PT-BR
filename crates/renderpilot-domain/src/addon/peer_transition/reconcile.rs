//! Reconciles the derived closed endpoint set with the ordered physical program.

use std::collections::BTreeMap;

use crate::normalized_path_key;

use super::model::{
    PeerEndpointIntent, PeerEndpointOperation, PeerTransitionError, ProxyPeerRoute,
};
use super::{EndpointGuard, PeerTransitionAuthorities, validation};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DerivedEndpoint {
    pub(super) intent: PeerEndpointIntent,
    pub(super) guard: EndpointGuard,
}

pub(super) fn reconcile_physical_program(
    route: ProxyPeerRoute,
    physical_program: Vec<PeerEndpointIntent>,
    mut derived: Vec<DerivedEndpoint>,
    authorities: &PeerTransitionAuthorities,
) -> Result<(Vec<PeerEndpointIntent>, Vec<EndpointGuard>), PeerTransitionError> {
    validation::validate_intents_with_authorities(route, &physical_program, authorities)?;
    let mut expected = BTreeMap::new();
    for endpoint in derived.drain(..) {
        let key = normalized_path_key(endpoint.intent.path().as_str());
        let path = endpoint.intent.path().clone();
        if expected.insert(key, endpoint).is_some() {
            return Err(PeerTransitionError::DuplicateEndpoint(path));
        }
    }
    let mut ordered = Vec::with_capacity(physical_program.len());
    let mut guards = Vec::with_capacity(physical_program.len());
    for physical in physical_program {
        let key = normalized_path_key(physical.path().as_str());
        let Some(endpoint) = expected.remove(&key) else {
            return Err(PeerTransitionError::UnclaimedPhysicalEndpoint(
                physical.path().clone(),
            ));
        };
        if endpoint.intent.operation() != physical.operation()
            || endpoint.intent.role() != physical.role()
        {
            return Err(PeerTransitionError::PhysicalProgramMismatch(
                physical.path().clone(),
            ));
        }
        if let Some(expected) = endpoint.guard.after_sha256.as_ref()
            && physical.planned_sha256() != Some(expected)
        {
            return Err(PeerTransitionError::DigestMismatch(physical.path().clone()));
        }
        if let Some(expected) = endpoint.guard.after_length
            && physical.planned_length() != Some(expected)
        {
            return Err(PeerTransitionError::LengthMismatch(physical.path().clone()));
        }
        if matches!(
            physical.operation(),
            PeerEndpointOperation::Create | PeerEndpointOperation::Replace
        ) && (physical.planned_sha256().is_none() || physical.planned_length().is_none())
        {
            return Err(PeerTransitionError::PhysicalProgramMismatch(
                physical.path().clone(),
            ));
        }
        ordered.push(physical);
        guards.push(endpoint.guard);
    }
    if let Some(endpoint) = expected.values().next() {
        return Err(PeerTransitionError::PhysicalProgramMismatch(
            endpoint.intent.path().clone(),
        ));
    }
    Ok((ordered, guards))
}
