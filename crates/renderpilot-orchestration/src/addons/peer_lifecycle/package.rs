use std::path::PathBuf;

use renderpilot_domain::{
    AddonKind, GameProxyTopology, InstalledAddon, LibraryComponent, PeerCatalogPhysicalContract,
    PeerCatalogRollbackClaim, PeerEndpointIntent, PeerEndpointOperation, PeerReadGuardRequirement,
    PeerTransitionContract, PlannedGameProxyTopology, ProxyPeerRoute,
    required_read_guards_with_catalog, validate_peer_metadata_only,
};
use renderpilot_storage_sqlite::ComponentBaselineMutation;

use super::roots::PeerRoots;
use super::route::classify_active_route;
use crate::ServiceError;
use crate::addons::engine::PeerMutationPlan;
use crate::addons::engine::apply::{PeerMutationPreflight, preflight_peer_mutation};
use crate::file_mutation::MutationScope;
use crate::peer_mutation_executor::{
    EndpointExpectation, EndpointPostcondition, ExactEndpointProgram, PeerAncestorPlan,
    render_peer_root,
};

/// All inputs needed to plan one active-topology peer transition.
///
/// Aggregate references are borrowed only while the caller builds the package;
/// the package clones the snapshots that storage preparation must retain. File
/// effects remain borrowed because their component identities are owned by the
/// surrounding catalog transaction.
pub(crate) struct PeerMutationRequest<'a> {
    pub(crate) peer_kind: AddonKind,
    pub(crate) before_peer: Option<&'a InstalledAddon>,
    pub(crate) after_peer: Option<&'a InstalledAddon>,
    pub(crate) before_topology: &'a GameProxyTopology,
    pub(crate) planned_after_topology: &'a PlannedGameProxyTopology,
    pub(crate) program: ExactEndpointProgram,
    pub(crate) payloads: Vec<Option<Vec<u8>>>,
    pub(crate) game_root: PathBuf,
    pub(crate) payload_root: Option<PathBuf>,
    pub(crate) component_set: Option<&'a [LibraryComponent]>,
    pub(crate) baseline_mutations: &'a [ComponentBaselineMutation<'a>],
    pub(crate) catalog_claim: Option<&'a PeerCatalogRollbackClaim>,
}

/// Immutable package crossing planning into durable reservation.
///
/// The package owns route, roots, O1 preflight, domain contract, endpoint
/// payload mapping, ancestor plan, and aggregate snapshots. Consumers must use
/// these projections instead of deriving any of them again. It is deliberately
/// move-only: cloning this boundary would duplicate retained O1 bytes and
/// payloads without adding any authority.
#[derive(Debug)]
pub(crate) struct PeerMutationPackage<'a> {
    plan: PeerMutationPlan,
    preflight: PeerMutationPreflight,
    ancestor_plan: PeerAncestorPlan,
    roots: PeerRoots,
    route: ProxyPeerRoute,
    contract: PeerTransitionContract,
    before_peer: Option<InstalledAddon>,
    after_peer: Option<InstalledAddon>,
    before_topology: GameProxyTopology,
    planned_after_topology: PlannedGameProxyTopology,
    component_set: Option<&'a [LibraryComponent]>,
    baseline_mutations: &'a [ComponentBaselineMutation<'a>],
    catalog_claim: Option<PeerCatalogRollbackClaim>,
    read_guards: Vec<PeerReadGuardRequirement>,
    canonical_game_root: String,
}

impl<'a> PeerMutationPackage<'a> {
    pub(crate) fn plan_active(request: PeerMutationRequest<'a>) -> Result<Self, ServiceError> {
        Self::plan_active_scoped(request)
    }

    fn plan_active_scoped(mut request: PeerMutationRequest<'a>) -> Result<Self, ServiceError> {
        let roots = PeerRoots::new(
            std::mem::take(&mut request.game_root),
            std::mem::take(&mut request.payload_root),
        )?;
        require_topology_game_root(
            &roots,
            request.before_topology,
            request.planned_after_topology,
        )?;
        validate_catalog_package_inputs(&request)?;
        let plan = PeerMutationPlan::new(request.program, std::mem::take(&mut request.payloads))
            .map_err(|error| crate::failed(error.to_string()))?;
        let preflight = preflight_peer_mutation(&plan, request.peer_kind)?;
        let preimages = crate::peer_mutation_executor::domain_preimages(&plan, &preflight)?;
        let route = classify_active_route(
            request.peer_kind,
            request.before_topology,
            request.planned_after_topology,
            plan.program(),
        )?;
        let physical_program = domain_program(plan.program(), plan.payloads())?;
        let catalog = request
            .catalog_claim
            .map(|claim| {
                PeerCatalogPhysicalContract::derive(
                    claim,
                    request.before_peer,
                    request.after_peer,
                    &physical_program,
                    &preimages,
                )
                .map_err(|error| {
                    crate::failed(format!("peer catalog physical contract rejected: {error}"))
                })
            })
            .transpose()?;
        let read_guards = required_read_guards_with_catalog(
            request.before_peer,
            request.after_peer,
            Some(request.before_topology),
            Some(request.planned_after_topology),
            route,
            &physical_program,
            catalog.as_ref(),
        )
        .map_err(|error| crate::failed(format!("peer read-guard derivation rejected: {error}")))?;
        for requirement in &read_guards {
            if requirement.is_topology_sourced() {
                roots
                    .require_game_path(requirement.path())
                    .map_err(|error| {
                        crate::failed(format!(
                            "peer topology read-guard is outside the explicit game authority: {error}"
                        ))
                    })?;
            } else {
                roots
                    .require_sealed_path(requirement.path())
                    .map_err(|error| {
                        crate::failed(format!(
                            "peer sealed read-guard is outside the explicit sealed authority: {error}"
                        ))
                    })?;
            }
        }
        let contract = PeerTransitionContract::derive_physical_with_catalog(
            request.before_peer,
            request.after_peer,
            Some(request.before_topology),
            Some(request.planned_after_topology),
            route,
            physical_program,
            catalog.as_ref(),
        )
        .map_err(|error| crate::failed(format!("peer transition contract rejected: {error}")))?;
        contract.validate_preimages(&preimages).map_err(|error| {
            crate::failed(format!("peer transition preimage rejected: {error}"))
        })?;
        let canonical_game_root = roots
            .roots()
            .first()
            .ok_or_else(|| crate::failed("peer mutation has no canonical game root"))
            .and_then(|root| {
                render_peer_root(root)
                    .map_err(|error| crate::failed(format!("invalid canonical game root: {error}")))
            })?;
        let ancestor_plan =
            PeerAncestorPlan::derive(plan.program(), preflight.before(), roots.roots())
                .map_err(|error| crate::failed(error.to_string()))?;

        assert_package_cardinality(&plan, &preflight, &ancestor_plan, &contract)?;

        Ok(Self {
            plan,
            preflight,
            ancestor_plan,
            roots,
            route,
            contract,
            before_peer: request.before_peer.cloned(),
            after_peer: request.after_peer.cloned(),
            before_topology: request.before_topology.clone(),
            planned_after_topology: request.planned_after_topology.clone(),
            component_set: request.component_set,
            baseline_mutations: request.baseline_mutations,
            catalog_claim: request.catalog_claim.cloned(),
            read_guards,
            canonical_game_root,
        })
    }

    pub(crate) fn plan(&self) -> &PeerMutationPlan {
        &self.plan
    }

    pub(crate) fn preflight(&self) -> &PeerMutationPreflight {
        &self.preflight
    }

    pub(crate) fn ancestor_plan(&self) -> &PeerAncestorPlan {
        &self.ancestor_plan
    }

    pub(crate) fn scope(&self) -> &MutationScope {
        self.roots.scope()
    }

    pub(crate) fn route(&self) -> ProxyPeerRoute {
        self.route
    }

    pub(crate) fn contract(&self) -> &PeerTransitionContract {
        &self.contract
    }

    pub(crate) fn before_peer(&self) -> Option<&InstalledAddon> {
        self.before_peer.as_ref()
    }

    pub(crate) fn after_peer(&self) -> Option<&InstalledAddon> {
        self.after_peer.as_ref()
    }

    pub(crate) fn before_topology(&self) -> &GameProxyTopology {
        &self.before_topology
    }

    pub(crate) fn planned_after_topology(&self) -> &PlannedGameProxyTopology {
        &self.planned_after_topology
    }

    pub(crate) fn component_set(&self) -> Option<&'a [LibraryComponent]> {
        self.component_set
    }

    pub(crate) fn baseline_mutations(&self) -> &'a [ComponentBaselineMutation<'a>] {
        self.baseline_mutations
    }

    pub(crate) fn catalog_claim(&self) -> Option<&PeerCatalogRollbackClaim> {
        self.catalog_claim.as_ref()
    }

    pub(crate) fn read_guards(&self) -> &[PeerReadGuardRequirement] {
        &self.read_guards
    }

    pub(crate) fn canonical_game_root(&self) -> &str {
        &self.canonical_game_root
    }
}

fn require_topology_game_root(
    roots: &PeerRoots,
    before: &GameProxyTopology,
    planned: &PlannedGameProxyTopology,
) -> Result<(), ServiceError> {
    roots.require_game_path(&before.root_slot)?;
    if let Some(downstream) = &before.downstream {
        roots.require_game_path(&downstream.path)?;
    }
    match planned {
        PlannedGameProxyTopology::Exact(topology) => {
            roots.require_game_path(&topology.root_slot)?;
            if let Some(downstream) = &topology.downstream {
                roots.require_game_path(&downstream.path)?;
            }
        }
        PlannedGameProxyTopology::ObservedOwnedDownstream {
            root_slot,
            downstream_path,
            ..
        } => {
            roots.require_game_path(root_slot)?;
            roots.require_game_path(downstream_path)?;
        }
    }
    Ok(())
}

/// Metadata-only callers must use the domain guard. Physical/topology changes
/// are rejected even when no filesystem roots are available.
pub(crate) fn validate_metadata_only(
    before_peer: Option<&InstalledAddon>,
    after_peer: Option<&InstalledAddon>,
    before_topology: Option<&GameProxyTopology>,
    after_topology: Option<&GameProxyTopology>,
) -> Result<(), ServiceError> {
    validate_peer_metadata_only(before_peer, after_peer, before_topology, after_topology)
        .map_err(|error| crate::failed(format!("metadata-only peer transition rejected: {error}")))
}

fn domain_program(
    program: &ExactEndpointProgram,
    payloads: &[Option<Vec<u8>>],
) -> Result<Vec<PeerEndpointIntent>, ServiceError> {
    if payloads.len() != program.endpoints().len() {
        return Err(crate::failed(
            "peer payload cardinality changed before domain validation",
        ));
    }
    program
        .endpoints()
        .iter()
        .zip(payloads)
        .map(|(endpoint, payload)| {
            let operation = match (endpoint.before(), endpoint.after()) {
                (EndpointExpectation::Absent, EndpointPostcondition::File(_)) => {
                    PeerEndpointOperation::Create
                }
                (EndpointExpectation::File(_), EndpointPostcondition::File(_)) => {
                    PeerEndpointOperation::Replace
                }
                (EndpointExpectation::File(_), EndpointPostcondition::Absent) => {
                    PeerEndpointOperation::Remove
                }
                _ => {
                    return Err(crate::failed(format!(
                        "invalid peer endpoint transition: {}",
                        endpoint.path().as_str()
                    )));
                }
            };
            let planned_sha256 = match endpoint.after() {
                EndpointPostcondition::File(digest) => Some(digest.clone()),
                EndpointPostcondition::Absent => None,
            };
            let planned_length = payload.as_ref().map(|bytes| bytes.len() as u64);
            let intent = PeerEndpointIntent::new(
                endpoint.path().clone(),
                endpoint.role(),
                operation,
                planned_sha256,
                planned_length,
            )
            .map_err(|error| crate::failed(error.to_string()))?;
            Ok(intent)
        })
        .collect()
}

fn assert_package_cardinality(
    plan: &PeerMutationPlan,
    preflight: &PeerMutationPreflight,
    ancestor_plan: &PeerAncestorPlan,
    contract: &PeerTransitionContract,
) -> Result<(), ServiceError> {
    let expected = plan.program().endpoints().len();
    if preflight.before().len() != expected
        || contract.intents().len() != expected
        || (0..expected).any(|ordinal| ancestor_plan.endpoint_chain(ordinal).is_none())
        || ancestor_plan.endpoint_chain(expected).is_some()
    {
        return Err(crate::failed(
            "peer package endpoint cardinality changed during planning",
        ));
    }
    Ok(())
}

fn validate_catalog_package_inputs(request: &PeerMutationRequest<'_>) -> Result<(), ServiceError> {
    let Some(claim) = request.catalog_claim else {
        return Ok(());
    };

    if claim.game_id() != &request.before_topology.game_id {
        return Err(crate::failed(
            "peer catalog claim belongs to a different game",
        ));
    }
    if request.component_set != Some(claim.after_components()) {
        return Err(crate::failed(
            "peer catalog claim does not match the component projection",
        ));
    }

    let expected = claim
        .deleted_baselines()
        .iter()
        .map(|entry| entry.component_id().clone())
        .collect::<std::collections::BTreeSet<_>>();
    let mut actual = std::collections::BTreeSet::new();
    for mutation in request.baseline_mutations {
        let ComponentBaselineMutation::Delete { component_id } = mutation else {
            return Err(crate::failed(
                "peer catalog claim permits only component-baseline deletes",
            ));
        };
        if !actual.insert((*component_id).clone()) {
            return Err(crate::failed(
                "peer catalog claim contains a duplicate component-baseline delete",
            ));
        }
    }
    if actual != expected {
        return Err(crate::failed(
            "peer catalog claim baseline-delete projection is incomplete or broadened",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
