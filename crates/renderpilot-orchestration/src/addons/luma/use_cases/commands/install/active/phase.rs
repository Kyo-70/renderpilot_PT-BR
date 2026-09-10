use std::path::Path;

use renderpilot_application::ProxyTopologyRepository;
use renderpilot_domain::{AddonKind, GameId, GameProxyTopology, PathRef};

use crate::addons::exclusivity;
use crate::addons::game_analysis::install_target_dir;
use crate::addons::luma::dgvoodoo;
use crate::addons::luma::errors;
use crate::addons::luma::game_context::{analyze_and_resolve, executable_override, require_game};
use crate::addons::luma::matcher::ResolvedLumaInstall;
use crate::addons::luma::types::LumaManifest;
use crate::addons::records;
use crate::addons::reshade::host_policy;
use crate::paths::same_path;
use crate::peer_mutation_executor::observe_peer_path_snapshot;
use crate::{Context, ServiceError};

use super::model::{ActiveInstallResolution, ActiveInstallSnapshot, DgVoodooPrepKind};
use super::validation;

pub(super) fn resolve_phase1(
    context: &Context,
    manifest: &LumaManifest,
    game_id: &GameId,
    topology: &GameProxyTopology,
) -> Result<ActiveInstallResolution, ServiceError> {
    let current = context
        .storage()
        .get_proxy_topology(game_id)?
        .ok_or_else(errors::state_changed_retry_install)?;
    if current != *topology {
        return Err(errors::state_changed_retry_install());
    }
    let resolved = resolve_common(context, manifest, game_id, current)?;
    let game_root = resolved.snapshot.game_root.clone();
    exclusivity::ensure_not_blocked(
        context,
        game_id,
        AddonKind::Luma,
        Some(&[game_root.as_path()]),
    )?;
    if crate::addons::engine::is_install_torn(&game_root, AddonKind::Luma) {
        return Err(ServiceError::invalid_input(
            "an incomplete earlier Luma install is present; recover it before retrying",
        ));
    }
    Ok(resolved)
}

pub(super) fn resolve_phase3(
    context: &Context,
    manifest: &LumaManifest,
    game_id: &GameId,
    phase1: &ActiveInstallResolution,
) -> Result<ActiveInstallResolution, ServiceError> {
    let topology = context
        .storage()
        .get_proxy_topology(game_id)?
        .ok_or_else(errors::state_changed_retry_install)?;
    if topology != phase1.snapshot.topology {
        return Err(errors::state_changed_retry_install());
    }
    let current = resolve_common(context, manifest, game_id, topology)?;
    ensure_snapshot_matches(&phase1.snapshot, &current.snapshot)?;
    if crate::addons::engine::is_install_torn(&current.snapshot.game_root, AddonKind::Luma) {
        return Err(errors::state_changed_retry_install());
    }
    Ok(current)
}

fn ensure_snapshot_matches(
    before: &ActiveInstallSnapshot,
    current: &ActiveInstallSnapshot,
) -> Result<(), ServiceError> {
    if !same_path(&before.game_root, &current.game_root)
        || !same_path(&before.target_dir, &current.target_dir)
        || before.asset != current.asset
        || before.addon_file != current.addon_file
        || before.arch != current.arch
        || before.proxy_dll_name != current.proxy_dll_name
        || before.external_requirement != current.external_requirement
        || before.writes_host != current.writes_host
        || before.dgvoodoo_kind != current.dgvoodoo_kind
        || before.topology != current.topology
        || before.host_path != current.host_path
        || before.host_snapshot != current.host_snapshot
    {
        return Err(errors::state_changed_retry_install());
    }
    Ok(())
}

fn resolve_common(
    context: &Context,
    manifest: &LumaManifest,
    game_id: &GameId,
    topology: GameProxyTopology,
) -> Result<ActiveInstallResolution, ServiceError> {
    records::ensure_no_record(
        context,
        game_id,
        AddonKind::Luma,
        "Luma is already installed for this game; uninstall before reinstalling",
    )?;
    let game = require_game(context, game_id)?;
    let override_path = executable_override(context, game_id);
    let (analysis, resolution) = analyze_and_resolve(&game, manifest, override_path.as_deref());
    let plan = validation::install_plan(resolution)?;
    if plan.arch != renderpilot_domain::Architecture::X64 {
        return Err(ServiceError::invalid_input(
            "active OptiScaler Luma installs require the x64 add-on build",
        ));
    }
    let target_dir = install_target_dir(&analysis)?;
    let game_root = crate::paths::canonical_candidate(&target_dir)
        .map_err(|error| errors::failed(format!("Luma game root is not reachable: {error}")))?;
    let host_path = path_ref(&game_root.join("ReShade64.dll"))?;
    topology.validate().map_err(|error| {
        errors::invalid(format!("active Luma proxy topology is invalid: {error}"))
    })?;
    validation::validate_topology(game_id, &game_root, &topology, &host_path)?;
    let game_root_ref = path_ref(&game_root)?;
    let host_snapshot = observe_peer_path_snapshot(&host_path, &game_root_ref)?;
    validation::validate_host_snapshot(&topology, &host_snapshot)?;
    let minimum_host_version = manifest.min_reshade_version_parsed()?;
    let writes_host = host_policy::probe_topology_host_download(
        &game_root,
        &host_path,
        &host_snapshot,
        Some(&minimum_host_version),
    )?;
    let dgvoodoo_kind = validation::assess_dgvoodoo_kind(&plan, &target_dir)?;
    validation::validate_preparation_kind(&plan, dgvoodoo_kind)?;
    Ok(ActiveInstallResolution {
        snapshot: ActiveInstallSnapshot {
            game_root,
            target_dir,
            asset: plan.asset.clone(),
            addon_file: plan.addon_file.clone(),
            arch: plan.arch,
            proxy_dll_name: plan.proxy_dll_name.clone(),
            external_requirement: plan.external_requirement.clone(),
            writes_host,
            dgvoodoo_kind,
            topology,
            host_path,
            host_snapshot,
        },
        plan,
    })
}

pub(super) fn preparation_for_plan(
    plan: &ResolvedLumaInstall,
    kind: DgVoodooPrepKind,
) -> Result<Option<dgvoodoo::DgVoodooPreparation<'_>>, ServiceError> {
    validation::validate_preparation_kind(plan, kind)?;
    let Some(requirement) = dgvoodoo::requirement(plan.external_requirement.as_ref()) else {
        return Ok(None);
    };
    match kind {
        DgVoodooPrepKind::None => Ok(None),
        DgVoodooPrepKind::Managed => Ok(Some(dgvoodoo::DgVoodooPreparation::Managed(requirement))),
        DgVoodooPrepKind::Reused => Ok(Some(dgvoodoo::DgVoodooPreparation::Reused(
            dgvoodoo::reused_config(requirement),
        ))),
    }
}

pub(super) fn path_ref(path: &Path) -> Result<PathRef, ServiceError> {
    PathRef::new(path.to_string_lossy().into_owned())
        .map_err(|_| errors::failed(format!("invalid Luma active path: {}", path.display())))
}

#[cfg(test)]
#[path = "phase_tests.rs"]
mod tests;
