use std::path::PathBuf;
use std::time::SystemTime;

use renderpilot_domain::{
    AddonKind, GameProxyTopology, InstalledAddon, InstalledAddonHostKind, ProxyImplementation,
};

use crate::{Context, ServiceError};

/// Complete sealed input for an ordinary active RenoDX install commit.
pub(super) struct ActiveGameCommit<'a> {
    pub(super) context: &'a Context,
    pub(super) game_id: &'a renderpilot_domain::GameId,
    pub(super) feature: &'static str,
    pub(super) safety: &'a crate::GameMutationSafetyPermits,
    pub(super) guard: crate::game_mutation_lock::GameMutationGuard,
    pub(super) record: InstalledAddon,
    pub(super) program: crate::peer_mutation_executor::ExactEndpointProgram,
    pub(super) payloads: Vec<Option<Vec<u8>>>,
    pub(super) before_topology: GameProxyTopology,
    pub(super) planned_topology: renderpilot_domain::PlannedGameProxyTopology,
    pub(super) game_root: PathBuf,
    pub(super) payload_root: Option<PathBuf>,
    pub(super) reshade_ini_authority: Option<renderpilot_domain::RenoDxReshadeIniAuthority>,
    pub(super) addon_path: PathBuf,
    pub(super) source_last_modified: Option<String>,
    pub(super) source_mtime: Option<SystemTime>,
}

pub(super) fn commit_game(request: ActiveGameCommit<'_>) -> Result<InstalledAddon, ServiceError> {
    let ActiveGameCommit {
        context,
        game_id,
        feature,
        safety,
        guard,
        record,
        program,
        payloads,
        before_topology,
        planned_topology,
        game_root,
        payload_root,
        reshade_ini_authority,
        addon_path,
        source_last_modified,
        source_mtime,
    } = request;
    crate::FileSafetyAuthority::new().authorize_game_commit(
        context,
        feature,
        &guard,
        safety.game(),
        || {
            let optiscaler_config = if record.host_kind() == Some(InstalledAddonHostKind::Proxy)
                && before_topology.outer.implementation == ProxyImplementation::OptiScaler
            {
                super::super::super::super::optiscaler_config::plan(
                    context, game_id, &game_root, true,
                )?
            } else {
                None
            };
            let (package, use_optiscaler_config) = if let Some(config) = optiscaler_config {
                let (companion, endpoint, payload) = config.into_parts();
                let program = program
                    .append(endpoint)
                    .map_err(|error| ServiceError::command_failed(error.to_string()))?;
                let mut payloads = payloads;
                payloads.push(Some(payload));
                let request = crate::addons::peer_lifecycle::package::PeerMutationRequest {
                    peer_kind: AddonKind::RenoDx,
                    before_peer: None,
                    after_peer: Some(&record),
                    before_topology: &before_topology,
                    planned_after_topology: &planned_topology,
                    program,
                    payloads,
                    game_root,
                    payload_root,
                    component_set: None,
                    baseline_mutations: &[],
                    catalog_claim: None,
                };
                (
                    crate::addons::peer_lifecycle::package::PeerMutationPackage::plan_active_with_renodx_optiscaler_config(
                        request,
                        reshade_ini_authority,
                        companion,
                    )?,
                    true,
                )
            } else {
                let request = crate::addons::peer_lifecycle::package::PeerMutationRequest {
                    peer_kind: AddonKind::RenoDx,
                    before_peer: None,
                    after_peer: Some(&record),
                    before_topology: &before_topology,
                    planned_after_topology: &planned_topology,
                    program,
                    payloads,
                    game_root,
                    payload_root,
                    component_set: None,
                    baseline_mutations: &[],
                    catalog_claim: None,
                };
                let package = match reshade_ini_authority {
                    Some(authority) => crate::addons::peer_lifecycle::package::PeerMutationPackage::plan_active_with_renodx_reshade_ini(
                        request,
                        authority,
                    )?,
                    None => crate::addons::peer_lifecycle::package::PeerMutationPackage::plan_active(
                        request,
                    )?,
                };
                (package, false)
            };
            let prepared_peer = if use_optiscaler_config {
                context
                    .peer_mutation_executor()
                    .prepare_ordinary_file_peer_with_renodx_optiscaler_config(
                        context,
                        &guard,
                        feature,
                        Some(game_id.as_str()),
                        package,
                    )?
            } else {
                context.peer_mutation_executor().prepare_ordinary_file_peer(
                    context,
                    &guard,
                    feature,
                    Some(game_id.as_str()),
                    package,
                )?
            };
            let mut changes = crate::addons::engine::InstallChanges::default();
            let applied = prepared_peer.apply(&mut changes)?;
            changes.sync_touched_dirs();
            applied.commit()?;
            crate::fs::stamp_mtime_best_effort(
                &addon_path,
                source_last_modified.as_deref(),
                source_mtime,
            );
            Ok(record)
        },
    )
}

#[cfg(test)]
#[path = "game/tests.rs"]
mod tests;
