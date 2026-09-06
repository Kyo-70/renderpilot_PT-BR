//! Validation of route-wide endpoint claims after reconciliation.

use crate::{GameProxyTopology, NormalizedPathRelation, normalized_path_relation};

use super::super::model::{
    PeerEndpointIntent, PeerEndpointRole, PeerTransitionError, ProxyPeerRoute,
};
use super::super::planning::PlannedTopologyShape;

pub(super) fn validate(
    route: ProxyPeerRoute,
    before: Option<&GameProxyTopology>,
    after: &PlannedTopologyShape,
    intents: &[PeerEndpointIntent],
    allow_renodx_reshade_ini: bool,
    allow_optiscaler_config: bool,
) -> Result<(), PeerTransitionError> {
    let mut protected = before
        .into_iter()
        .flat_map(GameProxyTopology::participant_paths)
        .collect::<Vec<_>>();
    protected.extend(after.participant_paths());
    let downstream_path = after
        .downstream()
        .map(super::super::planning::PlannedDownstream::path)
        .or_else(|| {
            before.and_then(|topology| topology.downstream.as_ref().map(|link| &link.path))
        });
    for intent in intents {
        if intent.role() == PeerEndpointRole::TopologyDownstream {
            continue;
        }
        if intent.role() == PeerEndpointRole::RenoDxReshadeIni && !allow_renodx_reshade_ini {
            return Err(PeerTransitionError::InvalidEndpointRole(
                intent.path().clone(),
            ));
        }
        if intent.role() == PeerEndpointRole::OptiScalerConfig && !allow_optiscaler_config {
            return Err(PeerTransitionError::InvalidEndpointRole(
                intent.path().clone(),
            ));
        }
        if protected
            .iter()
            .any(|path| normalized_path_relation(path.as_str(), intent.path().as_str()).overlaps())
        {
            return Err(PeerTransitionError::GenericTopologyOverlap(
                intent.path().clone(),
            ));
        }
        if downstream_path.is_some_and(|path| {
            matches!(
                normalized_path_relation(path.as_str(), intent.path().as_str()),
                NormalizedPathRelation::Equal
            )
        }) {
            return Err(PeerTransitionError::GenericTopologyOverlap(
                intent.path().clone(),
            ));
        }
    }
    let unchanged = before == after.exact();
    if matches!(route, ProxyPeerRoute::DurableDisjoint)
        && (!unchanged
            || intents.iter().any(|intent| {
                intent.role() != PeerEndpointRole::Disjoint
                    && !(allow_renodx_reshade_ini
                        && intent.role() == PeerEndpointRole::RenoDxReshadeIni)
                    && !(allow_optiscaler_config
                        && intent.role() == PeerEndpointRole::OptiScalerConfig)
            }))
    {
        return Err(PeerTransitionError::ImmutableTopology);
    }
    Ok(())
}
