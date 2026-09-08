use super::super::*;
use super::PeerTopologyDirection;

pub(super) fn peer_topology_direction(
    before: Option<&GameProxyTopology>,
    after: Option<&GameProxyTopology>,
) -> PeerTopologyDirection {
    let before_has_downstream = before.is_some_and(|topology| topology.downstream.is_some());
    let after_has_downstream = after.is_some_and(|topology| topology.downstream.is_some());
    match (before_has_downstream, after_has_downstream) {
        (true, false) => PeerTopologyDirection::OutOfOptiTopology,
        _ => PeerTopologyDirection::IntoOptiTopology,
    }
}

pub(in crate::repositories::game_mutations) fn expected_peer_relocation(
    before: Option<&GameProxyTopology>,
    after: Option<&GameProxyTopology>,
) -> AppResult<Option<ExpectedPeerRelocation>> {
    for topology in [before, after].into_iter().flatten() {
        topology.validate().map_err(crate::error::invalid_row)?;
        if topology.outer.implementation != ProxyImplementation::OptiScaler {
            return Err(renderpilot_application::AppError::invalid_input(
                "OptiScaler topology must have an OptiScaler outer",
            ));
        }
    }
    if let (Some(before), Some(after)) = (before, after)
        && before.id != after.id
    {
        return Err(renderpilot_application::AppError::invalid_input(
            "proxy topology subject cannot change during a filesystem transition",
        ));
    }

    let relocation = match (before, after) {
        (None, None) => None,
        (None, Some(after)) => {
            after_peer_relocation(after, "install", peer_topology_direction(None, Some(after)))?
        }
        (Some(before), None) => before_peer_relocation(
            before,
            "uninstall",
            peer_topology_direction(Some(before), after),
        )?,
        (Some(before), Some(after)) => match (&before.downstream, &after.downstream) {
            (None, None) => None,
            (None, Some(_)) => after_peer_relocation(
                after,
                "install",
                peer_topology_direction(Some(before), Some(after)),
            )?,
            (Some(_), None) => before_peer_relocation(
                before,
                "uninstall",
                peer_topology_direction(Some(before), Some(after)),
            )?,
            (Some(before_peer), Some(after_peer)) => {
                ensure_reshade_peer_link(before_peer, "before")?;
                ensure_reshade_peer_link(after_peer, "after")?;
                ensure_valid_peer_origin(before, "before")?;
                ensure_valid_peer_origin(after, "after")?;
                if normalized_path_key(before_peer.path.as_str())
                    == normalized_path_key(after_peer.path.as_str())
                {
                    if before_peer.receipt.digest() != after_peer.receipt.digest() {
                        return Err(renderpilot_application::AppError::invalid_input(
                            "OptiScaler topology changed a ReShade peer digest without relocation",
                        ));
                    }
                    None
                } else {
                    if before_peer.receipt.digest() != after_peer.receipt.digest() {
                        return Err(renderpilot_application::AppError::invalid_input(
                            "OptiScaler peer relocation changed the ReShade digest",
                        ));
                    }
                    if before_peer.receipt.ownership() != after_peer.receipt.ownership() {
                        return Err(renderpilot_application::AppError::invalid_input(
                            "OptiScaler peer relocation changed the ReShade custody",
                        ));
                    }
                    // `downstream_origin` is the return target for each
                    // topology, not the historical source of a peer move. The
                    // physical source and destination are therefore derived
                    // from the before/after downstream links themselves.
                    Some(ExpectedPeerRelocation {
                        source: before_peer.path.clone(),
                        destination: after_peer.path.clone(),
                        sha256: before_peer.receipt.digest().clone(),
                        ownership: after_peer.receipt.ownership(),
                        direction: peer_topology_direction(Some(before), Some(after)),
                    })
                }
            }
        },
    };
    Ok(relocation)
}

pub(in crate::repositories::game_mutations) fn ensure_reshade_peer_link(
    link: &renderpilot_domain::ProxyLink,
    side: &str,
) -> AppResult<()> {
    if link.implementation != ProxyImplementation::ReShade {
        return Err(renderpilot_application::AppError::invalid_input(format!(
            "OptiScaler {side} topology peer must be a ReShade downstream host",
        )));
    }
    Ok(())
}

pub(in crate::repositories::game_mutations) fn ensure_valid_peer_origin(
    topology: &GameProxyTopology,
    side: &str,
) -> AppResult<()> {
    let Some(downstream) = topology.downstream.as_ref() else {
        return Err(renderpilot_application::AppError::invalid_input(format!(
            "OptiScaler {side} topology peer is missing",
        )));
    };
    let Some(origin) = topology.downstream_origin.as_ref() else {
        return Err(renderpilot_application::AppError::invalid_input(format!(
            "OptiScaler {side} topology peer origin is missing",
        )));
    };
    let origin_key = normalized_path_key(origin.as_str());
    let root_key = normalized_path_key(topology.root_slot.as_str());
    if origin_key == normalized_path_key(downstream.path.as_str()) {
        return Err(renderpilot_application::AppError::invalid_input(format!(
            "OptiScaler {side} topology peer origin equals its active downstream path",
        )));
    }
    match topology.root_prestate {
        ProxyRootPrestate::RelocatedDownstream if origin_key != root_key => {
            return Err(renderpilot_application::AppError::invalid_input(format!(
                "OptiScaler {side} relocated root pre-state must originate at the root slot",
            )));
        }
        _ => {}
    }
    Ok(())
}

pub(in crate::repositories::game_mutations) fn after_peer_relocation(
    topology: &GameProxyTopology,
    operation: &str,
    direction: PeerTopologyDirection,
) -> AppResult<Option<ExpectedPeerRelocation>> {
    let Some(peer) = topology.downstream.as_ref() else {
        return Ok(None);
    };
    ensure_reshade_peer_link(peer, operation)?;
    ensure_valid_peer_origin(topology, operation)?;
    let origin = topology.downstream_origin.as_ref().ok_or_else(|| {
        renderpilot_application::AppError::invalid_input(format!(
            "OptiScaler {operation} topology peer origin is missing",
        ))
    })?;
    Ok(Some(ExpectedPeerRelocation {
        source: origin.clone(),
        destination: peer.path.clone(),
        sha256: peer.receipt.digest().clone(),
        ownership: peer.receipt.ownership(),
        direction,
    }))
}

pub(in crate::repositories::game_mutations) fn before_peer_relocation(
    topology: &GameProxyTopology,
    operation: &str,
    direction: PeerTopologyDirection,
) -> AppResult<Option<ExpectedPeerRelocation>> {
    let Some(peer) = topology.downstream.as_ref() else {
        return Ok(None);
    };
    ensure_reshade_peer_link(peer, operation)?;
    ensure_valid_peer_origin(topology, operation)?;
    let origin = topology.downstream_origin.as_ref().ok_or_else(|| {
        renderpilot_application::AppError::invalid_input(format!(
            "OptiScaler {operation} topology peer origin is missing",
        ))
    })?;
    Ok(Some(ExpectedPeerRelocation {
        source: peer.path.clone(),
        destination: origin.clone(),
        sha256: peer.receipt.digest().clone(),
        ownership: peer.receipt.ownership(),
        direction,
    }))
}
