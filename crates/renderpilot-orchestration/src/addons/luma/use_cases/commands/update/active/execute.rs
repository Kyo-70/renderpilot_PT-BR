//! Guarded execution of an already-composed active Luma update.
//!
//! Composition is deliberately completed before this module emits progress,
//! consumes safety authority, or reserves a durable file transaction.  Each
//! result variant has one corresponding persistence ceremony.

use std::path::Path;

use renderpilot_domain::{AddonKind, PeerCatalogRollbackClaim};
use renderpilot_storage_sqlite::ComponentBaselineMutation;

use crate::addons::engine::InstallChanges;
use crate::addons::luma::peer::LumaPeerRootAuthority;
use crate::addons::luma::peer::{
    LumaActiveUpdateAggregateMembership, LumaActiveUpdateComposition, LumaActiveUpdateEvidence,
    LumaActiveUpdateHostObservation, LumaActiveUpdateInput, LumaActiveUpdateMetadata,
    LumaActiveUpdatePhysical, LumaActiveUpdatePrepared, compose_active_update,
};
use crate::addons::luma::use_cases::commands::update::active::{
    ActiveUpdatePhase1, ActiveUpdatePhase3,
};
use crate::addons::peer_lifecycle::package::{PeerMutationPackage, PeerMutationRequest};
use crate::addons::peer_lifecycle::{
    PeerAggregateMembershipPackage, PeerAggregateMembershipRequest,
};
use crate::addons::progress::emit_tool_finalizing;
use crate::game_mutation_lock::GameMutationGuard;
use crate::net::ProgressObserver;
use crate::{Context, GameSafetyPermit, ServiceError};

/// Executes one active update result under the recovered game guard.
///
/// The caller supplies phase-three evidence and network preparation from the
/// preceding immutable phases.  This function performs no route discovery and
/// never falls back from a failed composition to an endpoint-free operation.
pub(super) fn commit_active_update(
    context: &Context,
    guard: &GameMutationGuard,
    safety: &GameSafetyPermit,
    phase1: &ActiveUpdatePhase1,
    phase3: ActiveUpdatePhase3,
    prepared: LumaActiveUpdatePrepared,
    progress: Option<&ProgressObserver<'_>>,
) -> Result<(), ServiceError> {
    ensure_guard_matches_phase1(guard, phase1)?;
    let (authority, host_assessment, catalog_claim, cascade) = phase3.into_parts();
    let host_observation = LumaActiveUpdateHostObservation::new(
        &host_assessment,
        phase1.downstream_path(),
        phase1.downstream_snapshot(),
    );
    let evidence = LumaActiveUpdateEvidence::new(
        phase1.record(),
        phase1.topology(),
        &authority,
        host_observation,
        phase1.minimum_reshade_version(),
        &catalog_claim,
        &cascade,
    );
    let input = LumaActiveUpdateInput::new(evidence, prepared);
    let composition = compose_active_update(input)
        .map_err(|error| ServiceError::command_failed(error.to_string()))?;

    match composition {
        LumaActiveUpdateComposition::Noop => Ok(()),
        LumaActiveUpdateComposition::Metadata(metadata) => {
            emit_tool_finalizing(progress, AddonKind::Luma);
            commit_metadata(context, guard, phase1, metadata)
        }
        LumaActiveUpdateComposition::AggregateMembership(membership) => {
            emit_tool_finalizing(progress, AddonKind::Luma);
            commit_aggregate_membership(context, guard, phase1, &authority, membership)
        }
        LumaActiveUpdateComposition::Physical(physical) => {
            emit_tool_finalizing(progress, AddonKind::Luma);
            commit_physical(context, guard, safety, phase1, &authority, *physical)
        }
    }
}

fn ensure_guard_matches_phase1(
    guard: &GameMutationGuard,
    phase1: &ActiveUpdatePhase1,
) -> Result<(), ServiceError> {
    ensure_guard_matches_record(guard, phase1.record())
}

fn ensure_guard_matches_record(
    guard: &GameMutationGuard,
    record: &renderpilot_domain::InstalledAddon,
) -> Result<(), ServiceError> {
    if guard.game_id() != record.game_id() {
        return Err(ServiceError::invalid_input(
            "active Luma update images do not belong to the guarded game",
        ));
    }
    Ok(())
}

fn commit_metadata(
    context: &Context,
    guard: &GameMutationGuard,
    phase1: &ActiveUpdatePhase1,
    metadata: LumaActiveUpdateMetadata,
) -> Result<(), ServiceError> {
    let after = metadata.into_record();
    crate::addons::luma::peer::commit_metadata(context, guard, phase1.record(), &after)
}

fn commit_aggregate_membership(
    context: &Context,
    guard: &GameMutationGuard,
    phase1: &ActiveUpdatePhase1,
    authority: &LumaPeerRootAuthority,
    membership: LumaActiveUpdateAggregateMembership,
) -> Result<(), ServiceError> {
    let after = membership.into_record();
    let package = PeerAggregateMembershipPackage::plan_active(PeerAggregateMembershipRequest {
        before_peer: phase1.record(),
        after_peer: &after,
        unchanged_topology: phase1.topology(),
        game_root: authority.canonical_game_root().to_path_buf(),
        payload_root: authority.external_capability_root().map(Path::to_path_buf),
    })?;
    context
        .peer_mutation_executor()
        .commit_reused_claim_membership(guard, package)
}

fn commit_physical(
    context: &Context,
    guard: &GameMutationGuard,
    safety: &GameSafetyPermit,
    phase1: &ActiveUpdatePhase1,
    authority: &LumaPeerRootAuthority,
    physical: LumaActiveUpdatePhysical<'_>,
) -> Result<(), ServiceError> {
    let (after, program, payloads, planned_topology, cascade, mtime) = physical.into_parts();
    let component_set = cascade
        .catalog_claim()
        .map(PeerCatalogRollbackClaim::after_components);
    let baseline_mutations = cascade
        .catalog_claim()
        .map(|claim| {
            claim
                .deleted_baselines()
                .iter()
                .map(|entry| ComponentBaselineMutation::Delete {
                    component_id: entry.component_id(),
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    crate::FileSafetyAuthority::new().authorize_game_commit(
        context,
        crate::addons::mutation_features::LUMA_UPDATE,
        guard,
        safety,
        || {
            let package = PeerMutationPackage::plan_active(PeerMutationRequest {
                peer_kind: AddonKind::Luma,
                before_peer: Some(phase1.record()),
                after_peer: Some(&after),
                before_topology: phase1.topology(),
                planned_after_topology: &planned_topology,
                program,
                payloads,
                game_root: authority.canonical_game_root().to_path_buf(),
                payload_root: authority.external_capability_root().map(Path::to_path_buf),
                component_set,
                baseline_mutations: &baseline_mutations,
                catalog_claim: cascade.catalog_claim(),
            })?;
            let prepared = context
                .peer_mutation_executor()
                .prepare_ordinary_file_peer(
                    context,
                    guard,
                    crate::addons::mutation_features::LUMA_UPDATE,
                    Some(guard.game_id().as_str()),
                    package,
                )?;
            let mut changes = InstallChanges::default();
            let applied = prepared.apply(&mut changes)?;
            changes.sync_touched_dirs();
            applied.commit()?;
            crate::catalog::cascade::record_cascade_rollback_journal(
                context.storage(),
                guard.game_id(),
                &cascade.rollback_specs,
            );
            if let Some((path, last_modified)) = mtime.as_ref() {
                crate::fs::stamp_mtime_best_effort(
                    Path::new(path.as_str()),
                    last_modified.as_deref(),
                    None,
                );
            }
            Ok(())
        },
    )
}

#[cfg(test)]
mod tests;
