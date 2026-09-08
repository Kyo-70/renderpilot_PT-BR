use renderpilot_application::{AppError, AppResult};
use renderpilot_domain::{
    AddonKind, GameProxyTopology, PeerReadGuardEvidence, PeerReusedClaimMembershipContract,
    PlannedGameProxyTopology, ProxyImplementation, validate_peer_metadata_only,
};
use rusqlite::OptionalExtension;

use super::super::aggregate::{AggregateAfter, AggregateBefore, GameAggregateMutation};
use super::super::read_guards;
use super::model::{MetadataAggregateTransition, PreparedMetadataRoute};

/// Result of validating the caller-selected route before a reservation is
/// opened.  The route owns every value that must survive until commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ValidatedPreparation {
    pub(super) after: AggregateAfter,
    pub(super) route: PreparedMetadataRoute,
}

pub(super) fn validate_preparation(
    mutation: &GameAggregateMutation,
    transition: MetadataAggregateTransition<'_>,
) -> AppResult<ValidatedPreparation> {
    let after = materialize_after(mutation)?;
    validate_unchanged_participants(mutation.before(), &after)?;
    let before_peer = require_peer(mutation.before().peer(), "metadata aggregate before")?;
    let after_peer = require_peer(after.peer(), "metadata aggregate after")?;
    reject_unpersistable_peer(after_peer.kind())?;
    if same_persisted_peer_columns(before_peer, after_peer) {
        return Err(AppError::invalid_input(
            "metadata aggregate transition cannot be a no-op",
        ));
    }

    let route = match transition {
        MetadataAggregateTransition::PeerMetadataRefresh => {
            validate_refresh(
                before_peer,
                after_peer,
                mutation.before().topology(),
                after.topology(),
            )?;
            PreparedMetadataRoute::PeerMetadataRefresh
        }
        MetadataAggregateTransition::ReusedClaimMembership {
            canonical_game_root,
            sealed_roots,
            initial_read_guards,
        } => validate_membership(
            before_peer,
            after_peer,
            mutation.before().topology(),
            after.topology(),
            canonical_game_root,
            sealed_roots,
            initial_read_guards,
        )?,
    };
    Ok(ValidatedPreparation { after, route })
}

/// Revalidates the route after it has been moved into an opaque permit.
pub(super) fn validate_owned_route(
    mutation: &GameAggregateMutation,
    route: &PreparedMetadataRoute,
) -> AppResult<AggregateAfter> {
    let after = materialize_after(mutation)?;
    validate_unchanged_participants(mutation.before(), &after)?;
    let before_peer = require_peer(mutation.before().peer(), "metadata aggregate before")?;
    let after_peer = require_peer(after.peer(), "metadata aggregate after")?;
    reject_unpersistable_peer(after_peer.kind())?;
    if same_persisted_peer_columns(before_peer, after_peer) {
        return Err(AppError::storage_failed(
            "metadata aggregate permit contains a no-op transition",
        ));
    }
    match route {
        PreparedMetadataRoute::PeerMetadataRefresh => {
            validate_refresh(
                before_peer,
                after_peer,
                mutation.before().topology(),
                after.topology(),
            )?;
        }
        PreparedMetadataRoute::ReusedClaimMembership {
            canonical_game_root,
            sealed_roots,
            contract,
            initial_read_guards,
        } => {
            let strings = sealed_roots
                .iter()
                .map(|root| root.as_str().to_owned())
                .collect::<Vec<_>>();
            let (bound_canonical, bound_roots) =
                read_guards::bind_roots_for_membership(canonical_game_root.as_str(), &strings)?;
            if bound_canonical != *canonical_game_root || bound_roots != *sealed_roots {
                return Err(AppError::storage_failed(
                    "metadata aggregate permit roots changed",
                ));
            }
            let topology = require_topology(mutation.before().topology(), "membership")?;
            for path in topology.participant_paths() {
                read_guards::validate_topology_participant(canonical_game_root, path)?;
            }
            let current_contract = derive_membership(before_peer, after_peer, topology)?;
            if &current_contract != contract {
                return Err(AppError::storage_failed(
                    "metadata aggregate permit membership contract changed",
                ));
            }
            validate_membership_evidence(sealed_roots, &current_contract, initial_read_guards)?;
        }
    }
    Ok(after)
}

pub(super) fn validate_final_guards(
    route: &PreparedMetadataRoute,
    final_read_guards: &[PeerReadGuardEvidence],
) -> AppResult<()> {
    match route {
        PreparedMetadataRoute::PeerMetadataRefresh => {
            if final_read_guards.is_empty() {
                Ok(())
            } else {
                Err(AppError::invalid_input(
                    "metadata refresh cannot carry read-guard evidence",
                ))
            }
        }
        PreparedMetadataRoute::ReusedClaimMembership {
            sealed_roots,
            contract,
            ..
        } => validate_membership_evidence(sealed_roots, contract, final_read_guards),
    }
}

pub(super) fn load_before(
    transaction: &rusqlite::Transaction<'_>,
    game_id: &renderpilot_domain::GameId,
) -> AppResult<AggregateBefore> {
    let state = crate::repositories::get_optiscaler_state_within_transaction(transaction, game_id)?;
    let topology =
        crate::repositories::proxy_topologies::get_within_transaction(transaction, game_id)?;
    let peer = crate::repositories::installed_addons::get_within_transaction(transaction, game_id)?;
    AggregateBefore::new(game_id.clone(), state, topology, peer)
}

pub(super) fn read_generation(
    transaction: &rusqlite::Transaction<'_>,
    game_id: &renderpilot_domain::GameId,
) -> AppResult<super::super::aggregate::AggregateGeneration> {
    let value: Option<i64> = transaction
        .query_row(
            "SELECT peer_aggregate_revision FROM games WHERE id = ?1",
            [game_id.as_str()],
            |row| row.get(0),
        )
        .optional()
        .map_err(crate::error::storage_error)?;
    value
        .ok_or_else(|| AppError::storage_failed(format!("game `{game_id}` is missing")))
        .and_then(super::super::aggregate::AggregateGeneration::from_persisted)
}

fn materialize_after(mutation: &GameAggregateMutation) -> AppResult<AggregateAfter> {
    let topology = match mutation.planned_after().topology() {
        None => None,
        Some(PlannedGameProxyTopology::Exact(topology)) => Some(topology.clone()),
        Some(PlannedGameProxyTopology::ObservedOwnedDownstream { .. }) => {
            return Err(AppError::invalid_input(
                "metadata aggregate cannot materialize an observed downstream topology",
            ));
        }
    };
    AggregateAfter::new(
        mutation.planned_after().game_id().clone(),
        mutation.planned_after().state().cloned(),
        topology,
        mutation.planned_after().peer().cloned(),
    )
}

fn validate_unchanged_participants(
    before: &AggregateBefore,
    after: &AggregateAfter,
) -> AppResult<()> {
    if before.state() != after.state() {
        return Err(AppError::invalid_input(
            "metadata aggregate cannot change OptiScaler state",
        ));
    }
    if before.topology() != after.topology() {
        return Err(AppError::invalid_input(
            "metadata aggregate cannot change proxy topology",
        ));
    }
    Ok(())
}

fn validate_refresh(
    before: &renderpilot_domain::InstalledAddon,
    after: &renderpilot_domain::InstalledAddon,
    before_topology: Option<&GameProxyTopology>,
    after_topology: Option<&GameProxyTopology>,
) -> AppResult<()> {
    validate_peer_metadata_only(Some(before), Some(after), before_topology, after_topology)
        .map_err(super::super::permit::domain_error)
}

fn validate_membership(
    before: &renderpilot_domain::InstalledAddon,
    after: &renderpilot_domain::InstalledAddon,
    before_topology: Option<&GameProxyTopology>,
    after_topology: Option<&GameProxyTopology>,
    canonical_game_root: &str,
    sealed_roots: &[String],
    initial_read_guards: &[PeerReadGuardEvidence],
) -> AppResult<PreparedMetadataRoute> {
    let before_topology = require_topology(before_topology, "membership")?;
    let after_topology = require_topology(after_topology, "membership")?;
    if before_topology != after_topology {
        return Err(AppError::invalid_input(
            "membership route requires an unchanged OptiScaler topology",
        ));
    }
    if before_topology.outer.implementation != ProxyImplementation::OptiScaler {
        return Err(AppError::invalid_input(
            "membership route requires an OptiScaler outer topology",
        ));
    }
    let contract = derive_membership(before, after, before_topology)?;
    let (canonical_game_root, sealed_roots) =
        read_guards::bind_roots_for_membership(canonical_game_root, sealed_roots)?;
    for path in before_topology.participant_paths() {
        read_guards::validate_topology_participant(&canonical_game_root, path)?;
    }
    validate_membership_evidence(&sealed_roots, &contract, initial_read_guards)?;
    Ok(PreparedMetadataRoute::ReusedClaimMembership {
        canonical_game_root,
        sealed_roots,
        contract,
        initial_read_guards: initial_read_guards.to_vec(),
    })
}

fn validate_membership_evidence(
    sealed_roots: &[renderpilot_domain::PathRef],
    contract: &PeerReusedClaimMembershipContract,
    evidence: &[PeerReadGuardEvidence],
) -> AppResult<()> {
    read_guards::validate_membership_evidence(sealed_roots, contract, evidence)
}

fn derive_membership(
    before: &renderpilot_domain::InstalledAddon,
    after: &renderpilot_domain::InstalledAddon,
    topology: &GameProxyTopology,
) -> AppResult<PeerReusedClaimMembershipContract> {
    PeerReusedClaimMembershipContract::derive(before, after, topology)
        .map_err(super::super::permit::domain_error)
}

fn require_peer<'a>(
    peer: Option<&'a renderpilot_domain::InstalledAddon>,
    side: &str,
) -> AppResult<&'a renderpilot_domain::InstalledAddon> {
    peer.ok_or_else(|| AppError::invalid_input(format!("{side} peer must be present")))
}

fn require_topology<'a>(
    topology: Option<&'a GameProxyTopology>,
    route: &str,
) -> AppResult<&'a GameProxyTopology> {
    topology.ok_or_else(|| {
        AppError::invalid_input(format!("{route} route requires an OptiScaler topology"))
    })
}

fn reject_unpersistable_peer(kind: AddonKind) -> AppResult<()> {
    if kind == AddonKind::OptiScaler {
        return Err(AppError::invalid_input(
            "metadata aggregate cannot persist an OptiScaler peer row",
        ));
    }
    Ok(())
}

/// Timestamps are storage-owned row metadata and are not an aggregate
/// participant mutation.  Compare exactly the columns the peer repository
/// persists so a freshly built after-image cannot turn a semantic no-op into a
/// generation advance merely by omitting those timestamps.
fn same_persisted_peer_columns(
    before: &renderpilot_domain::InstalledAddon,
    after: &renderpilot_domain::InstalledAddon,
) -> bool {
    before.game_id() == after.game_id()
        && before.kind() == after.kind()
        && before.addon_file() == after.addon_file()
        && before.addon_version() == after.addon_version()
        && before.created_files() == after.created_files()
        && before.backed_up_files() == after.backed_up_files()
        && before.managed_files() == after.managed_files()
        && before.tracked_sources() == after.tracked_sources()
        && before.host_kind() == after.host_kind()
        && before.reshade_channel() == after.reshade_channel()
        && before.registered_exe_path() == after.registered_exe_path()
}
