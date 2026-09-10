//! Composition boundary for one active-topology Luma update.
//!
//! This module only joins the already validated projections from the update
//! phases. It does not observe the filesystem, resolve configuration, call
//! storage, or choose an application command. The returned route is the
//! complete immutable decision consumed by those later boundaries.

use renderpilot_domain::{
    GameProxyTopology, InstalledAddon, PeerReusedClaimMembershipContract, PlannedGameProxyTopology,
};

use super::super::effects::{LumaPeerEffectAccumulator, LumaPeerOperationOrder};
use crate::addons::peer_lifecycle::package::validate_metadata_only;

use crate::peer_mutation_executor::ExactEndpoint;

use super::{
    dgvoodoo::project_dgvoodoo,
    dlss::project_dlss,
    error::LumaActiveUpdateError,
    host::project_host,
    model::{
        LumaActiveUpdateAggregateMembership, LumaActiveUpdateComposition, LumaActiveUpdateInput,
        LumaActiveUpdateMetadata, LumaActiveUpdatePhysical,
    },
    payload::project_payload,
    record::rebuild_record,
};

/// Composes one active Luma update from its immutable phase-3 input.
///
/// The projections are consumed in dependency order and the effect
/// accumulator is finalized exactly once. A physical program takes priority
/// over metadata or aggregate membership changes; endpoint-free changes must
/// pass one of the two typed, zero-write routes or are rejected.
pub(crate) fn compose_active_update(
    input: LumaActiveUpdateInput<'_>,
) -> Result<LumaActiveUpdateComposition<'_>, LumaActiveUpdateError> {
    let (
        before,
        topology,
        authority,
        host_observation,
        minimum_host_version,
        catalog_claim,
        cascade,
        prepared,
    ) = input.into_parts();
    let (
        payload_input,
        host_input,
        dgvoodoo_input,
        dependency_paths,
        tracked_sources,
        addon_version,
    ) = prepared.into_parts();

    let mut accumulator = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);
    let payload = project_payload(
        before,
        authority,
        payload_input,
        &dependency_paths,
        &tracked_sources,
        &mut accumulator,
    )?;
    let (payload_record, dlss_input, mtime) = payload.into_parts();

    let dlss = project_dlss(
        before,
        authority,
        dlss_input,
        catalog_claim,
        cascade,
        &mut accumulator,
    )?;
    let dgvoodoo = project_dgvoodoo(
        before,
        authority,
        dgvoodoo_input,
        &dependency_paths,
        &tracked_sources,
        &mut accumulator,
    )?;
    let host = project_host(
        before,
        topology,
        authority,
        host_input,
        host_observation,
        minimum_host_version,
        &mut accumulator,
    )?;
    let (host_binding, planned_topology) = host.into_parts();

    let after = rebuild_record(
        before,
        payload_record,
        dgvoodoo,
        &host_binding,
        dlss,
        tracked_sources,
        addon_version,
    )?;
    let effects = accumulator.finalize()?;

    match effects {
        Some(effects) => {
            let (program, payloads) = effects.into_parts();
            authority
                .validate(
                    Some(before),
                    Some(&after),
                    cascade.catalog_claim(),
                    program.endpoints().iter().map(ExactEndpoint::path),
                )
                .map_err(LumaActiveUpdateError::authority)?;
            Ok(LumaActiveUpdateComposition::Physical(Box::new(
                LumaActiveUpdatePhysical::new(
                    after,
                    program,
                    payloads,
                    planned_topology,
                    cascade,
                    mtime,
                ),
            )))
        }
        None => compose_endpoint_free(
            before,
            topology,
            authority,
            after,
            &planned_topology,
            cascade,
            mtime.as_ref(),
        ),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EndpointFreeRoute {
    Noop,
    Metadata,
    AggregateMembership,
}

fn compose_endpoint_free<'a>(
    before: &'a InstalledAddon,
    topology: &'a GameProxyTopology,
    authority: &'a super::super::root_authority::LumaPeerRootAuthority,
    after: InstalledAddon,
    planned_topology: &PlannedGameProxyTopology,
    cascade: &'a crate::catalog::cascade::CascadeResult,
    mtime: Option<&super::model::LumaActiveUpdateMtime>,
) -> Result<LumaActiveUpdateComposition<'a>, LumaActiveUpdateError> {
    let route =
        select_endpoint_free_route(before, topology, &after, planned_topology, cascade, mtime)?;

    authority
        .validate(Some(before), Some(&after), None, &[])
        .map_err(LumaActiveUpdateError::authority)?;

    match route {
        EndpointFreeRoute::Noop => Ok(LumaActiveUpdateComposition::Noop),
        EndpointFreeRoute::Metadata => Ok(LumaActiveUpdateComposition::Metadata(
            LumaActiveUpdateMetadata::new(after),
        )),
        EndpointFreeRoute::AggregateMembership => {
            Ok(LumaActiveUpdateComposition::AggregateMembership(
                LumaActiveUpdateAggregateMembership::new(after),
            ))
        }
    }
}

fn select_endpoint_free_route(
    before: &InstalledAddon,
    topology: &GameProxyTopology,
    after: &InstalledAddon,
    planned_topology: &PlannedGameProxyTopology,
    cascade: &crate::catalog::cascade::CascadeResult,
    mtime: Option<&super::model::LumaActiveUpdateMtime>,
) -> Result<EndpointFreeRoute, LumaActiveUpdateError> {
    if !matches!(planned_topology, PlannedGameProxyTopology::Exact(planned) if planned == topology)
    {
        return Err(LumaActiveUpdateError::endpoint_free_topology());
    }
    if mtime.is_some() {
        return Err(LumaActiveUpdateError::endpoint_free_mtime());
    }
    if cascade.catalog_claim().is_some() {
        return Err(LumaActiveUpdateError::endpoint_free_cascade(
            "catalog rollback claim has no physical endpoint program",
        ));
    }
    if !cascade.rollback_specs.is_empty() {
        return Err(LumaActiveUpdateError::endpoint_free_cascade(
            "catalog rollback specs have no physical endpoint program",
        ));
    }
    if !cascade.mutation_paths.is_empty() {
        return Err(LumaActiveUpdateError::endpoint_free_cascade(
            "catalog mutation paths have no physical endpoint program",
        ));
    }

    if after == before {
        return Ok(EndpointFreeRoute::Noop);
    }

    if after.managed_files() == before.managed_files() {
        validate_metadata_only(Some(before), Some(after), Some(topology), Some(topology))
            .map_err(LumaActiveUpdateError::metadata)?;
        return Ok(EndpointFreeRoute::Metadata);
    }

    PeerReusedClaimMembershipContract::derive(before, after, topology)?;
    Ok(EndpointFreeRoute::AggregateMembership)
}

#[cfg(test)]
mod tests;
