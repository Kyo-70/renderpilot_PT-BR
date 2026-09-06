//! Snapshot-to-intent derivation for peer transitions.

use crate::{GameProxyTopology, InstalledAddon};

use super::model::{
    PeerEndpointIntent, PeerTransitionError, PlannedGameProxyTopology, ProxyPeerRoute,
};
use super::reconcile::reconcile_physical_program;
use super::validation;
use super::{
    ExactOptiConfigProjection, PeerTransitionAuthorities, PeerTransitionContext,
    PeerTransitionContract, claims::PeerSnapshot,
};

mod generic;
mod helpers;
mod host_release;
mod managed;
mod renodx_reshade_ini;
mod reused;
mod route;
mod topology;

pub(super) fn derive_physical(
    before_peer: Option<&InstalledAddon>,
    after_peer: Option<&InstalledAddon>,
    before_topology: Option<&GameProxyTopology>,
    planned_after_topology: Option<&PlannedGameProxyTopology>,
    route: ProxyPeerRoute,
    physical_program: Vec<PeerEndpointIntent>,
) -> Result<PeerTransitionContract, PeerTransitionError> {
    derive_physical_scoped(
        PeerTransitionContext::new(
            before_peer,
            after_peer,
            before_topology,
            planned_after_topology,
            route,
        ),
        physical_program,
        None,
        None,
        None,
    )
}

pub(super) fn derive_physical_with_catalog(
    before_peer: Option<&InstalledAddon>,
    after_peer: Option<&InstalledAddon>,
    before_topology: Option<&GameProxyTopology>,
    planned_after_topology: Option<&PlannedGameProxyTopology>,
    route: ProxyPeerRoute,
    physical_program: Vec<PeerEndpointIntent>,
    catalog: Option<&super::catalog_physical::PeerCatalogPhysicalContract>,
) -> Result<PeerTransitionContract, PeerTransitionError> {
    derive_physical_scoped(
        PeerTransitionContext::new(
            before_peer,
            after_peer,
            before_topology,
            planned_after_topology,
            route,
        ),
        physical_program,
        catalog,
        None,
        None,
    )
}

pub(super) fn derive_physical_scoped(
    context: PeerTransitionContext<'_>,
    physical_program: Vec<PeerEndpointIntent>,
    catalog: Option<&super::catalog_physical::PeerCatalogPhysicalContract>,
    renodx_reshade_ini: Option<super::renodx_reshade_ini::RenoDxReshadeIniAuthority>,
    optiscaler_config: Option<ExactOptiConfigProjection>,
) -> Result<PeerTransitionContract, PeerTransitionError> {
    let PeerTransitionContext {
        before_peer,
        after_peer,
        before_topology,
        planned_after_topology,
        route,
    } = context;
    let mut authorities = PeerTransitionAuthorities {
        topology_downstream: None,
        renodx_reshade_ini: renodx_reshade_ini.clone(),
        optiscaler_config: optiscaler_config.clone(),
        dlss_fix: None,
    };
    // The downstream authority is derived from the sealed topology below.
    // Validating it before that derivation would reject every ordinary
    // coordinated transition as if it were an untrusted typed endpoint.
    validation::validate_intents_with_scoped_authority(
        route,
        &physical_program,
        authorities.renodx_reshade_ini.as_ref(),
        authorities.optiscaler_config.as_ref(),
        None,
    )?;
    let planned_shape =
        super::planning::topology_shape(before_topology, planned_after_topology, route)?;
    super::planning::validate_shape(before_topology, &planned_shape)?;
    let after_topology = planned_shape.exact();
    super::claims::validate_snapshots(before_peer, after_peer, before_topology, after_topology)?;
    let before = PeerSnapshot::from_peer(before_peer)?;
    let after = PeerSnapshot::from_peer(after_peer)?;
    authorities.topology_downstream =
        helpers::coordinated_path(route, before_topology, &planned_shape)?;
    let physical_index = generic::index_physical_program(&physical_program);
    let mut endpoints = Vec::new();
    generic::derive(
        &before,
        &after,
        &physical_program,
        &physical_index,
        &mut endpoints,
    )?;
    managed::derive(
        &before,
        &after,
        authorities.topology_downstream.as_ref(),
        &mut endpoints,
    )?;
    topology::derive(
        PeerTransitionContext::new(
            before_peer,
            after_peer,
            before_topology,
            planned_after_topology,
            route,
        ),
        &planned_shape,
        authorities.topology_downstream.as_ref(),
        &physical_program,
        &mut endpoints,
    )?;
    host_release::validate(route, before_peer, before_topology, &physical_program)?;
    reused::validate_acquisition_order(&before, &after, &physical_program)?;

    if let Some(catalog) = catalog {
        super::catalog_merge::bind(catalog, before_peer, after_peer, &physical_program)?;
        endpoints = super::catalog_merge::merge(endpoints, catalog, before_peer, after_peer)?;
    }

    if let Some(authority) = authorities.renodx_reshade_ini.as_ref() {
        renodx_reshade_ini::bind(
            authority,
            before_peer,
            after_peer,
            &before,
            &after,
            &physical_program,
            &mut endpoints,
        )?;
    }

    if let Some(projection) = optiscaler_config.as_ref() {
        super::optiscaler_config::bind(projection, &physical_program, &mut endpoints)?;
    }

    let (intents, guards) =
        reconcile_physical_program(route, physical_program, endpoints, &authorities)?;
    route::validate(
        route,
        before_topology,
        &planned_shape,
        &intents,
        authorities.renodx_reshade_ini.is_some(),
        authorities.optiscaler_config.is_some(),
    )?;
    Ok(PeerTransitionContract {
        route,
        intents,
        guards,
        topology_downstream: authorities.topology_downstream,
        renodx_reshade_ini,
        optiscaler_config,
        dlss_projection: None,
    })
}
