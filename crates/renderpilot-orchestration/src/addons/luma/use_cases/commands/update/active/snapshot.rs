//! Locked, local-only active Luma phase-one snapshot.

use std::path::{Path, PathBuf};

use renderpilot_domain::{
    AddonKind, FileOwnership, GameId, GameProxyTopology, InstalledAddon, PathRef,
    ProxyImplementation, normalized_path_key,
};

use crate::ServiceError;
use crate::addons::engine;
use crate::addons::luma::dgvoodoo;
use crate::addons::luma::errors;
use crate::addons::luma::tracking;
use crate::addons::luma::types::{LumaExternalRequirement, LumaManifest};
use crate::addons::luma::use_cases::update_target::ResolvedUpdateTarget;
use crate::addons::reshade::host_policy;
use crate::peer_mutation_executor::{PeerPathSnapshot, observe_peer_path_snapshot};

use super::model::{ActiveUpdatePhase1, DgVoodooLocalDecision};

/// Builds the complete active phase-one snapshot from already-resolved local
/// inputs. The caller must hold the per-game mutation guard for the entire
/// call; this function itself performs no lock acquisition or mutation.
pub(crate) fn snapshot_active_update(
    manifest: &LumaManifest,
    game_id: &GameId,
    record: InstalledAddon,
    target: ResolvedUpdateTarget,
    topology: GameProxyTopology,
    stored_game_install_path: PathRef,
) -> Result<ActiveUpdatePhase1, ServiceError> {
    topology.validate().map_err(|error| {
        errors::invalid(format!("active Luma proxy topology is invalid: {error}"))
    })?;

    let canonical_game_root = crate::paths::canonical_candidate(&target.game_dir)
        .map_err(|error| errors::failed(format!("Luma game root is not reachable: {error}")))?;
    validate_topology(game_id, &canonical_game_root, &topology)?;

    let downstream_path = topology
        .downstream
        .as_ref()
        .map(|link| link.path.clone())
        .ok_or_else(|| {
            errors::invalid("active Luma topology has no ReShade downstream".to_owned())
        })?;
    let canonical_root_ref = path_ref(&canonical_game_root)?;
    let downstream_snapshot = observe_peer_path_snapshot(&downstream_path, &canonical_root_ref)?;
    validate_downstream_snapshot(&topology, &downstream_snapshot)?;

    let minimum_reshade_version = manifest.min_reshade_version_parsed()?;
    let host_replacement_required = host_policy::probe_topology_host_download(
        &canonical_game_root,
        &downstream_path,
        &downstream_snapshot,
        Some(&minimum_reshade_version),
    )?;
    let dependency_paths = dependency_paths(&target, &record);
    let dgvoodoo = dgvoodoo_decision(&target, &record, &dependency_paths);
    let had_torn_marker = engine::is_install_torn(&canonical_game_root, AddonKind::Luma);
    let payload_disk_intact = tracking::payload_disk_intact(&record);

    Ok(ActiveUpdatePhase1 {
        record,
        topology,
        target,
        stored_game_install_path,
        canonical_game_root,
        downstream_path,
        downstream_snapshot,
        minimum_reshade_version,
        had_torn_marker,
        payload_disk_intact,
        dependency_paths,
        dgvoodoo,
        host_replacement_required,
    })
}

fn validate_topology(
    game_id: &GameId,
    canonical_game_root: &Path,
    topology: &GameProxyTopology,
) -> Result<(), ServiceError> {
    if &topology.game_id != game_id
        || topology.outer.implementation != ProxyImplementation::OptiScaler
    {
        return Err(errors::invalid(
            "active Luma proxy topology does not belong to this OptiScaler game",
        ));
    }

    let Some(root_parent) = Path::new(topology.root_slot.as_str()).parent() else {
        return Err(errors::invalid(
            "active Luma proxy topology root slot has no parent",
        ));
    };
    if normalized_path_key(&root_parent.to_string_lossy())
        != normalized_path_key(&canonical_game_root.to_string_lossy())
    {
        return Err(errors::invalid(
            "active Luma proxy topology root slot is not directly under the game root",
        ));
    }

    let Some(downstream) = topology.downstream.as_ref() else {
        return Err(errors::invalid(
            "active Luma topology requires a ReShade downstream",
        ));
    };
    if downstream.implementation != ProxyImplementation::ReShade
        || !matches!(
            downstream.receipt.ownership(),
            FileOwnership::Owned | FileOwnership::Reused
        )
    {
        return Err(errors::invalid(
            "active Luma topology downstream must be a managed or reused ReShade host",
        ));
    }
    let expected_downstream = canonical_game_root.join("ReShade64.dll");
    if normalized_path_key(downstream.path.as_str())
        != normalized_path_key(&expected_downstream.to_string_lossy())
    {
        return Err(errors::invalid(
            "active Luma topology downstream must be the direct ReShade64.dll child of the game root",
        ));
    }
    Ok(())
}

fn validate_downstream_snapshot(
    topology: &GameProxyTopology,
    snapshot: &PeerPathSnapshot,
) -> Result<(), ServiceError> {
    let downstream = topology.downstream.as_ref().ok_or_else(|| {
        errors::invalid("active Luma topology requires a ReShade downstream".to_owned())
    })?;
    let Some(file) = snapshot.file() else {
        return Err(errors::invalid(
            "active Luma topology downstream ReShade64.dll is absent",
        ));
    };
    if downstream.receipt.digest() != file.digest()
        || downstream.receipt.identity() != file.identity()
    {
        return Err(errors::state_changed_retry_update());
    }
    Ok(())
}

fn dependency_paths(target: &ResolvedUpdateTarget, record: &InstalledAddon) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(requirement) = dgvoodoo::requirement(target.external_requirement.as_ref()) {
        for name in dgvoodoo::game_file_names(requirement) {
            push_unique_path(&mut paths, target.game_dir.join(name));
        }
    }
    for historical in tracking::owned_dependency_paths(record) {
        push_unique_path(&mut paths, historical);
    }
    paths.sort_by_key(|path| crate::paths::normalized_key(path));
    paths
}

fn dgvoodoo_decision(
    target: &ResolvedUpdateTarget,
    record: &InstalledAddon,
    dependency_paths: &[PathBuf],
) -> DgVoodooLocalDecision {
    let Some(requirement) = dgvoodoo::requirement(target.external_requirement.as_ref()) else {
        return if dependency_paths
            .iter()
            .any(|path| tracking::owns_path(record, path))
        {
            DgVoodooLocalDecision::Remove
        } else {
            DgVoodooLocalDecision::Preserve {
                config_owned: false,
            }
        };
    };

    let config_owned = tracking::owns_path(
        record,
        &target.game_dir.join(match requirement {
            LumaExternalRequirement::Dgvoodoo2 { config_file, .. } => config_file,
        }),
    );
    if !dgvoodoo::record_can_manage_runtime(record, &target.game_dir, requirement) {
        return DgVoodooLocalDecision::Preserve { config_owned };
    }

    managed_dgvoodoo_decision(
        dgvoodoo::owned_status(&target.game_dir, requirement),
        config_owned,
    )
}

fn managed_dgvoodoo_decision(
    status: dgvoodoo::OwnedDgVoodooStatus,
    config_owned: bool,
) -> DgVoodooLocalDecision {
    match status {
        dgvoodoo::OwnedDgVoodooStatus::Outdated | dgvoodoo::OwnedDgVoodooStatus::Incomplete => {
            DgVoodooLocalDecision::Replace { config_owned }
        }
        dgvoodoo::OwnedDgVoodooStatus::Current => DgVoodooLocalDecision::Preserve { config_owned },
        dgvoodoo::OwnedDgVoodooStatus::Unknown => {
            DgVoodooLocalDecision::ReplaceOnFull { config_owned }
        }
    }
}

fn push_unique_path(paths: &mut Vec<PathBuf>, candidate: PathBuf) {
    if !paths
        .iter()
        .any(|existing| crate::paths::same_path(existing, &candidate))
    {
        paths.push(candidate);
    }
}

fn path_ref(path: &Path) -> Result<PathRef, ServiceError> {
    PathRef::new(path.to_string_lossy().into_owned())
        .map_err(|error| errors::invalid(format!("invalid active Luma path: {error}")))
}

#[cfg(test)]
mod tests;
