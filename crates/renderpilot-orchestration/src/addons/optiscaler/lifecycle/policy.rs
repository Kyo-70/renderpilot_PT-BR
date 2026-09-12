use super::*;

pub(crate) fn managed_config_invariants(
    release: &OptiScalerRelease,
    modules: &HashSet<String>,
    target_dir: &Path,
    native_paths: &NativeModulePaths,
    chain_reshade: bool,
    compatibility_invariants: &[ManagedIniValue],
) -> Vec<ManagedIniValue> {
    let mut invariants = compatibility_invariants.to_vec();
    super::super::config::append_reshade_chain_invariant(&mut invariants, chain_reshade);
    if release_has_private_runtime(release) {
        invariants.push(ManagedIniValue {
            section: "Libraries".to_owned(),
            key: "OptiDllPath".to_owned(),
            value: ".\\OptiScaler".to_owned(),
        });
    }
    if modules.contains("nvidia_sr") {
        let target = module_library_path(
            release,
            "nvidia_sr",
            target_dir,
            native_paths.dlss_sr.as_deref(),
            "nvngx_dlss.dll",
        );
        invariants.push(ManagedIniValue {
            section: "Libraries".to_owned(),
            key: "NvngxDlssPath".to_owned(),
            value: ini_library_path(target_dir, &target),
        });
    }
    if modules.contains("optipatcher") {
        invariants.push(ManagedIniValue {
            section: "Plugins".to_owned(),
            key: "LoadAsiPlugins".to_owned(),
            value: "true".to_owned(),
        });
    }
    invariants
}

pub(in crate::addons::optiscaler) fn current_release(
    manifest: &OptiScalerManifest,
) -> Result<&OptiScalerRelease, ServiceError> {
    manifest
        .current_release()
        .ok_or_else(|| failed("OptiScaler stable catalogue has no current release"))
}

pub(in crate::addons::optiscaler) fn target_from_availability(
    availability: &EvaluatedAvailability,
) -> Result<ApplyTarget, ServiceError> {
    Ok(ApplyTarget {
        exe: availability
            .target_exe
            .clone()
            .ok_or_else(|| failed("no target executable"))?,
        dir: availability
            .target_dir
            .clone()
            .ok_or_else(|| failed("no target directory"))?,
        proxy: availability.proxy.clone(),
    })
}

pub(in crate::addons::optiscaler) fn ensure_apply_allowed(
    availability: &EvaluatedAvailability,
) -> Result<(), ServiceError> {
    ensure_availability_allowed(availability)
}

pub(in crate::addons::optiscaler) fn ensure_adoption_allowed(
    availability: &EvaluatedAvailability,
) -> Result<(), ServiceError> {
    ensure_availability_allowed(availability)
}

pub(in crate::addons::optiscaler) fn ensure_availability_allowed(
    availability: &EvaluatedAvailability,
) -> Result<(), ServiceError> {
    if let Some(reason) = availability.blocked_reason.as_deref() {
        return Err(failed(reason.to_owned()));
    }
    Ok(())
}

pub(in crate::addons::optiscaler) fn ensure_target_unchanged(
    target: &ApplyTarget,
    availability: &EvaluatedAvailability,
) -> Result<(), ServiceError> {
    let target_dir_unchanged = availability
        .target_dir
        .as_deref()
        .is_some_and(|dir| crate::paths::same_path(dir, &target.dir));
    let downstream_unchanged = match (
        availability.proxy.downstream_path.as_deref(),
        target.proxy.downstream_path.as_deref(),
    ) {
        (None, None) => true,
        (Some(left), Some(right)) => crate::paths::same_path(left, right),
        _ => false,
    };
    let source_unchanged = match (
        availability.proxy.reshade_source_path.as_deref(),
        target.proxy.reshade_source_path.as_deref(),
    ) {
        (None, None) => true,
        (Some(left), Some(right)) => crate::paths::same_path(left, right),
        _ => false,
    };
    let source_hash_unchanged =
        availability.proxy.reshade_source_sha256 == target.proxy.reshade_source_sha256;
    if availability.blocked_reason.is_some()
        || !availability
            .target_exe
            .as_deref()
            .is_some_and(|path| crate::paths::same_path(path, &target.exe))
        || !target_dir_unchanged
        || !crate::paths::same_path(&availability.proxy.slot, &target.proxy.slot)
        || availability.proxy.chain_reshade != target.proxy.chain_reshade
        || !downstream_unchanged
        || !source_unchanged
        || !source_hash_unchanged
    {
        return Err(failed(
            "OptiScaler target changed while the archive was downloading; retry",
        ));
    }
    Ok(())
}

pub(in crate::addons::optiscaler) fn capture_lifecycle_snapshot(
    context: &Context,
    game_id: &GameId,
) -> Result<OptiScalerLifecycleSnapshot, ServiceError> {
    Ok(OptiScalerLifecycleSnapshot {
        state: context.storage().get_optiscaler_install_state(game_id)?,
        topology: context.storage().get_proxy_topology(game_id)?,
    })
}

pub(in crate::addons::optiscaler) fn ensure_fresh_install_snapshot(
    snapshot: &OptiScalerLifecycleSnapshot,
) -> Result<(), ServiceError> {
    if snapshot.state.is_some() {
        return Err(failed("OptiScaler is already installed for this game"));
    }
    Ok(())
}

pub(in crate::addons::optiscaler) fn ensure_managed_install_snapshot(
    snapshot: &OptiScalerLifecycleSnapshot,
) -> Result<(), ServiceError> {
    if snapshot.state.is_none() {
        return Err(failed("OptiScaler is not installed for this game"));
    }
    Ok(())
}

/// Reconstructs the generic managed-file view needed by module matching from
/// the closed OptiScaler state. These bindings are ephemeral; exact cleanup
/// authority remains in the typed state receipts.
pub(in crate::addons::optiscaler) fn managed_bindings_from_state(
    state: Option<&OptiScalerInstallState>,
) -> Vec<ManagedAddonFile> {
    let Some(state) = state else {
        return Vec::new();
    };
    let mut bindings = state
        .release_files
        .iter()
        .map(|receipt| {
            let mode = receipt.installed.ownership();
            match mode {
                FileOwnership::Owned => ManagedAddonFile::owned(
                    receipt.path.clone(),
                    ManagedFileBaseline::Absent,
                    receipt.installed.digest().clone(),
                ),
                FileOwnership::Reused => ManagedAddonFile::reused(
                    receipt.path.clone(),
                    receipt.installed.digest().clone(),
                ),
            }
        })
        .collect::<Vec<_>>();
    bindings.extend(state.runtime_bindings.iter().map(
        |binding| match binding.installed.ownership() {
            FileOwnership::Owned => ManagedAddonFile::owned(
                binding.path.clone(),
                managed_baseline_view(&binding.baseline),
                binding.installed.digest().clone(),
            ),
            FileOwnership::Reused => {
                ManagedAddonFile::reused(binding.path.clone(), binding.installed.digest().clone())
            }
        },
    ));
    bindings
}

/// Converts the exact OptiScaler baseline to the generic managed-file view
/// used by module matching. This view is never destructive authority.
pub(in crate::addons::optiscaler) fn managed_baseline_view(
    baseline: &OptiScalerFileBaseline,
) -> ManagedFileBaseline {
    match baseline {
        OptiScalerFileBaseline::Absent => ManagedFileBaseline::Absent,
        OptiScalerFileBaseline::Present { receipt } => ManagedFileBaseline::Present {
            sha256: receipt.digest().clone(),
        },
    }
}

pub(in crate::addons::optiscaler) fn ensure_lifecycle_snapshot_unchanged(
    context: &Context,
    game_id: &GameId,
    expected: &OptiScalerLifecycleSnapshot,
) -> Result<(), ServiceError> {
    if &capture_lifecycle_snapshot(context, game_id)? != expected {
        return Err(failed(
            "OptiScaler state changed while artifacts were being prepared; retry",
        ));
    }
    Ok(())
}

pub(in crate::addons::optiscaler) fn target_paths(
    release: &OptiScalerRelease,
    modules: &HashSet<String>,
    target_dir: &Path,
    proxy_path: &Path,
) -> HashMap<String, PathBuf> {
    selected_members(release, modules)
        .map(|member| {
            let path = if member.target == "$proxy" {
                proxy_path.to_path_buf()
            } else {
                target_dir.join(&member.target)
            };
            (member.archive_path.to_ascii_lowercase(), path)
        })
        .collect()
}

pub(in crate::addons::optiscaler) fn changed_release_paths(
    release: &OptiScalerRelease,
    modules: &HashSet<String>,
    targets: &HashMap<String, PathBuf>,
) -> Result<HashSet<String>, ServiceError> {
    let mut changed = HashSet::new();
    for member in selected_members(release, modules) {
        let Some(path) = targets.get(&member.archive_path.to_ascii_lowercase()) else {
            continue;
        };
        let expected = Sha256Hash::new(member.sha256.clone())
            .map_err(|error| failed(format!("invalid release member hash: {error}")))?;
        if !file_matches(path, Some(&expected)) {
            changed.insert(crate::paths::normalized_key(path));
        }
    }
    Ok(changed)
}
