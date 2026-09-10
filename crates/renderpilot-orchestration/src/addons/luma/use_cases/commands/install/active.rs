//! Active OptiScaler-topology Luma install.
//!
//! The three phases keep all network work outside the mutation lock, then
//! submit one peer package to the durable executor. No engine sentinel
//! mutation or independent metadata commit is used on this route.

mod model;
mod phase;
mod validation;

#[cfg(test)]
mod tests;

use std::path::Path;

use renderpilot_domain::{AddonKind, GameId, GameProxyTopology, InstalledAddon};

use crate::ServiceError;
use crate::addons::exclusivity;
use crate::addons::luma::fetch::prepare::prepare_install;
use crate::addons::luma::install::PreparedInstall;
use crate::addons::luma::peer::{
    LumaActiveInstallInput, LumaPeerRootAuthority, compose_active_install,
};
use crate::addons::progress::emit_tool_finalizing;
use crate::addons::reshade::host_policy;
use crate::coordinated_files::catalog_path_claim;
use crate::net::ProgressObserver;
use crate::{Context, GameSafetyPermit};

use super::InstallRequest;
use model::ActiveInstallResolution;

pub(super) async fn install(
    request: InstallRequest<'_>,
    topology: GameProxyTopology,
) -> Result<renderpilot_domain::InstalledAddon, ServiceError> {
    let InstallRequest {
        context,
        manifest,
        reshade_sources,
        game_id,
        safety,
        progress,
    } = request;
    let phase1 = {
        let _guard =
            crate::mutation_boundary::enter_game_mutation_boundary_async(context, game_id).await?;
        phase::resolve_phase1(context, manifest, game_id, &topology)?
    };

    let dgvoodoo = phase::preparation_for_plan(&phase1.plan, phase1.snapshot.dgvoodoo_kind)?;
    let prepared = prepare_install(
        &phase1.plan,
        reshade_sources,
        game_id.clone(),
        phase1.snapshot.writes_host,
        dgvoodoo,
        progress,
    )
    .await?;

    commit_prepared(
        context, manifest, game_id, &safety, progress, &phase1, prepared,
    )
    .await
}

/// Completes an active install from an immutable, already-prepared payload.
///
/// This is the single phase-3 boundary for both the network route and local
/// acceptance coverage: the phase-1 snapshot is revalidated while holding the
/// mutation guard, then the sealed root, peer composer, durable executor, and
/// final mtime update run as one sequence.
async fn commit_prepared(
    context: &Context,
    manifest: &crate::addons::luma::types::LumaManifest,
    game_id: &GameId,
    safety: &GameSafetyPermit,
    progress: Option<&ProgressObserver<'_>>,
    phase1: &ActiveInstallResolution,
    prepared: PreparedInstall,
) -> Result<InstalledAddon, ServiceError> {
    let minimum_host_version = manifest.min_reshade_version_parsed()?;
    let guard =
        crate::mutation_boundary::enter_game_mutation_boundary_async(context, game_id).await?;
    let phase3 = phase::resolve_phase3(context, manifest, game_id, phase1)?;
    let authority = LumaPeerRootAuthority::resolve(
        &phase3.snapshot.game_root,
        Path::new(phase3.snapshot.host_path.as_str()),
    )?;
    let roots = sealed_scan_roots(&authority);
    exclusivity::ensure_not_blocked(context, game_id, AddonKind::Luma, Some(roots.as_slice()))?;

    let assessment = host_policy::assess_topology_downstream_from_snapshot(
        &phase3.snapshot.game_root,
        &phase3.snapshot.host_path,
        &phase3.snapshot.host_snapshot,
        authority.content(),
        "Luma",
        Some(&minimum_host_version),
    )?;
    let dlss_path = authority.effective_dlss_target()?;
    let catalog_claim =
        catalog_path_claim(context.storage(), game_id, Path::new(dlss_path.as_str()))?;
    let composition = compose_active_install(LumaActiveInstallInput {
        prepared,
        topology: &phase3.snapshot.topology,
        authority: &authority,
        assessment: &assessment,
        host_path: &phase3.snapshot.host_path,
        host_snapshot: &phase3.snapshot.host_snapshot,
        catalog_claim: &catalog_claim,
        minimum_host_version: &minimum_host_version,
    })?;
    let source_last_modified = composition.record().addon_dated().map(str::to_owned);
    let (record, program, payloads, planned_topology) = composition.into_parts();

    emit_tool_finalizing(progress, AddonKind::Luma);
    let addon_path = Path::new(record.addon_file().as_str()).to_path_buf();
    crate::FileSafetyAuthority::new().authorize_game_commit(
        context,
        crate::addons::mutation_features::LUMA_INSTALL,
        &guard,
        safety,
        || {
            let package = crate::addons::peer_lifecycle::package::PeerMutationPackage::plan_active(
                crate::addons::peer_lifecycle::package::PeerMutationRequest {
                    peer_kind: AddonKind::Luma,
                    before_peer: None,
                    after_peer: Some(&record),
                    before_topology: &phase3.snapshot.topology,
                    planned_after_topology: &planned_topology,
                    program,
                    payloads,
                    game_root: authority.canonical_game_root().to_path_buf(),
                    payload_root: authority.external_capability_root().map(Path::to_path_buf),
                    component_set: None,
                    baseline_mutations: &[],
                    catalog_claim: None,
                },
            )?;
            let prepared_peer = context
                .peer_mutation_executor()
                .prepare_ordinary_file_peer(
                    context,
                    &guard,
                    crate::addons::mutation_features::LUMA_INSTALL,
                    Some(game_id.as_str()),
                    package,
                )?;
            let mut changes = crate::addons::engine::InstallChanges::default();
            let applied = prepared_peer.apply(&mut changes)?;
            changes.sync_touched_dirs();
            applied.commit()?;
            crate::fs::stamp_mtime_best_effort(&addon_path, source_last_modified.as_deref(), None);
            Ok(record)
        },
    )
}

fn sealed_scan_roots(authority: &LumaPeerRootAuthority) -> Vec<&Path> {
    let mut roots = vec![authority.canonical_game_root()];
    if let Some(payload_root) = authority.external_capability_root() {
        roots.push(payload_root);
    }
    roots
}
