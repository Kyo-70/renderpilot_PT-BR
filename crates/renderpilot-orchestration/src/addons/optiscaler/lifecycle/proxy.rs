use super::*;

pub(in crate::addons::optiscaler) fn preflight_targets(
    targets: &HashMap<String, PathBuf>,
    old_paths: &HashSet<PathBuf>,
    proxy: &EvaluatedProxyPlan,
    known_downstream: Option<&Path>,
    retained_fsr_targets: &HashSet<String>,
) -> Result<(), ServiceError> {
    for target in targets.values() {
        // custody journal creates and observes nested release directories through its
        // typed participant plan.  The preflight authority can classify
        // direct game-slot entries here; probing a missing nested parent
        // would require a second path-based authority walk.
        let direct_game_entry =
            target
                .parent()
                .zip(proxy.slot.parent())
                .is_some_and(|(target_parent, game_parent)| {
                    crate::paths::same_path(target_parent, game_parent)
                });
        if !direct_game_entry {
            continue;
        }
        if maybe_exact_receipt_from_live(target, FileOwnership::Reused)?.is_none()
            || old_paths
                .iter()
                .any(|old| crate::paths::same_path(old, target))
        {
            continue;
        }
        if crate::paths::same_path(target, &proxy.slot) && proxy.chain_reshade {
            continue;
        }
        if retained_fsr_targets.contains(&crate::paths::normalized_key(target)) {
            continue;
        }
        if target
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case("OptiScaler.ini"))
            && target
                .parent()
                .is_some_and(|parent| !super::super::tool::unmanaged_install_present(parent))
        {
            // A standalone INI is a preset/recovery source, not a live install.
            // The semantic merge below adopts it without discarding user edits.
            continue;
        }
        return Err(failed(format!(
            "refusing to overwrite unmanaged file {}",
            target.display()
        )));
    }
    let downstream_present = proxy
        .downstream_path
        .as_ref()
        .filter(|path| {
            path.parent()
                .zip(proxy.slot.parent())
                .is_some_and(|(path_parent, game_parent)| {
                    crate::paths::same_path(path_parent, game_parent)
                })
        })
        .map(|path| maybe_exact_receipt_from_live(path, FileOwnership::Reused))
        .transpose()?
        .flatten()
        .is_some();
    if proxy.chain_reshade
        && downstream_present
        && !proxy.reshade_source_path.as_deref().is_some_and(|source| {
            proxy
                .downstream_path
                .as_deref()
                .is_some_and(|downstream| crate::paths::same_path(source, downstream))
        })
        && !known_downstream.is_some_and(|known| {
            proxy
                .downstream_path
                .as_ref()
                .is_some_and(|path| crate::paths::same_path(known, path))
        })
    {
        return Err(failed(
            "ReShade64.dll already exists; proxy chain is ambiguous",
        ));
    }
    Ok(())
}

#[derive(Clone, Copy)]
pub(in crate::addons::optiscaler) struct ProxyTopologyExecution<'a> {
    pub(in crate::addons::optiscaler) context: &'a Context,
    pub(in crate::addons::optiscaler) game_id: &'a GameId,
    pub(in crate::addons::optiscaler) proxy: &'a EvaluatedProxyPlan,
    pub(in crate::addons::optiscaler) release: &'a OptiScalerRelease,
    pub(in crate::addons::optiscaler) archive: &'a PreparedArchive,
    pub(in crate::addons::optiscaler) updating: bool,
    pub(in crate::addons::optiscaler) downstream_ownership: FileOwnership,
}

pub(in crate::addons::optiscaler) fn execute_proxy_topology(
    execution: ProxyTopologyExecution<'_>,
    mutation: &mut PreparedFileMutation<'_>,
    changed: &mut Vec<String>,
) -> Result<GameProxyTopology, ServiceError> {
    let ProxyTopologyExecution {
        context,
        game_id,
        proxy,
        release,
        archive,
        updating,
        downstream_ownership,
    } = execution;
    let core = release
        .members
        .iter()
        .find(|member| member.target == "$proxy")
        .ok_or_else(|| failed("release has no proxy member"))?;
    let outer_hash = Sha256Hash::new(super::super::archive::sha256_hex(
        archive.bytes(&core.archive_path)?,
    ))
    .map_err(|error| failed(error.to_string()))?;
    let downstream = evaluated_downstream_install_plan(proxy, updating, downstream_ownership)?;
    crate::addons::proxy_chain::execute_install_plan(
        context,
        &crate::addons::proxy_chain::ProxyInstallPlan {
            game_id,
            root_slot: &proxy.slot,
            outer_sha256: outer_hash,
            updating,
            downstream,
        },
        archive.bytes(&core.archive_path)?,
        mutation,
        changed,
    )
}

pub(in crate::addons::optiscaler) fn evaluated_downstream_install_plan(
    proxy: &EvaluatedProxyPlan,
    updating: bool,
    downstream_ownership: FileOwnership,
) -> Result<Option<crate::addons::proxy_chain::DownstreamInstallPlan<'_>>, ServiceError> {
    Ok(
        match (
            proxy.chain_reshade,
            proxy.downstream_path.as_deref(),
            proxy.reshade_source_path.as_deref(),
            proxy.reshade_source_sha256.as_ref(),
        ) {
            (false, None, None, None) => None,
            (true, Some(destination_path), Some(source_path), Some(expected_source_sha256)) => {
                Some(
                    crate::addons::proxy_chain::DownstreamInstallPlan::Transfer {
                        source_path,
                        expected_source_sha256,
                        destination_path,
                        destination_ownership: downstream_ownership,
                    },
                )
            }
            (true, Some(destination_path), None, None) if updating => Some(
                crate::addons::proxy_chain::DownstreamInstallPlan::Existing { destination_path },
            ),
            (false, _, _, _) => {
                return Err(failed(
                    "a non-chain OptiScaler proxy plan contains ReShade paths",
                ));
            }
            _ => {
                return Err(failed(
                    "OptiScaler ReShade chain plan is incomplete or inconsistent",
                ));
            }
        },
    )
}

pub(in crate::addons::optiscaler) fn expected_target_hashes(
    release: &OptiScalerRelease,
    modules: &[String],
    state: &OptiScalerInstallState,
    proxy_path: &Path,
) -> Result<HashMap<String, Sha256Hash>, ServiceError> {
    let modules: HashSet<_> = modules.iter().cloned().collect();
    selected_members(release, &modules)
        .map(|member| {
            let path = if member.target == "$proxy" {
                proxy_path.to_path_buf()
            } else {
                Path::new(state.target_dir.as_str()).join(&member.target)
            };
            let hash = Sha256Hash::new(member.sha256.clone())
                .map_err(|error| failed(format!("invalid release member hash: {error}")))?;
            Ok((crate::paths::normalized_key(&path), hash))
        })
        .collect()
}

pub(in crate::addons::optiscaler) fn other_managed_claims(
    context: &Context,
    game_id: &GameId,
) -> Result<Vec<renderpilot_storage_sqlite::OptiScalerRetainedClaim>, ServiceError> {
    let mut claims = Vec::new();
    let installed_addons = context.storage().list_installed_addons()?;
    for managed in installed_addons
        .iter()
        .filter(|record| record.game_id() == game_id && record.kind() != AddonKind::OptiScaler)
        .flat_map(|record| record.managed_files())
    {
        let path = Path::new(managed.path().as_str());
        let live =
            maybe_exact_receipt_from_live(path, FileOwnership::Reused)?.ok_or_else(|| {
                failed(format!(
                    "remaining managed claim is absent at {}",
                    path.display()
                ))
            })?;
        if live.digest() != managed.installed_sha256() {
            return Err(failed(format!(
                "remaining managed claim changed at {}",
                path.display()
            )));
        }
        claims.push(renderpilot_storage_sqlite::OptiScalerRetainedClaim {
            path: managed.path().clone(),
            receipt: live,
        });
    }
    claims.sort_by(|left, right| {
        crate::paths::normalized_key(Path::new(left.path.as_str())).cmp(
            &crate::paths::normalized_key(Path::new(right.path.as_str())),
        )
    });
    for pair in claims.windows(2) {
        if crate::paths::same_path(
            Path::new(pair[0].path.as_str()),
            Path::new(pair[1].path.as_str()),
        ) && pair[0].receipt != pair[1].receipt
        {
            return Err(failed(format!(
                "remaining managed claims disagree at {}",
                pair[0].path
            )));
        }
    }
    claims.dedup_by(|left, right| {
        crate::paths::normalized_key(Path::new(left.path.as_str()))
            == crate::paths::normalized_key(Path::new(right.path.as_str()))
            && left.receipt == right.receipt
    });
    Ok(claims)
}

pub(in crate::addons::optiscaler) fn ensure_native_claims_compatible(
    context: &Context,
    game_id: &GameId,
    native_targets: &[NativeTarget],
) -> Result<(), ServiceError> {
    for target in native_targets {
        let required = renderpilot_detection::sha256_file(&target.source)
            .map_err(|error| failed(error.to_string()))?;
        if let Some((path, installed)) = context
            .storage()
            .list_installed_addons()?
            .into_iter()
            .filter(|record| record.game_id() == game_id && record.kind() != AddonKind::OptiScaler)
            .flat_map(|record| record.managed_files().to_vec())
            .map(|managed| (managed.path().clone(), managed.installed_sha256().clone()))
            .find(|(path, installed)| {
                crate::paths::same_path(Path::new(path.as_str()), target.destination.as_path())
                    && installed != &required
            })
        {
            return Err(failed(format!(
                "OptiScaler module {} requires {} at {}, but another managed consumer at {} requires {}",
                target.module_id,
                required,
                target.destination.display(),
                path,
                installed
            )));
        }
    }
    Ok(())
}

pub(in crate::addons::optiscaler) fn ensure_module_artifact_claims_compatible(
    context: &Context,
    game_id: &GameId,
    artifact_targets: &[(&PreparedModuleArtifact, PathBuf)],
) -> Result<(), ServiceError> {
    let other_files = context
        .storage()
        .list_installed_addons()?
        .into_iter()
        .filter(|record| record.game_id() == game_id && record.kind() != AddonKind::OptiScaler)
        .flat_map(|record| record.managed_files().to_vec())
        .collect::<Vec<_>>();
    for (artifact, destination) in artifact_targets {
        if let Some(conflict) = other_files.iter().find(|managed| {
            crate::paths::same_path(Path::new(managed.path().as_str()), destination.as_path())
                && managed.installed_sha256() != &artifact.sha256
        }) {
            return Err(failed(format!(
                "OptiScaler module {} requires {} at {}, but another managed consumer requires {}",
                artifact.module_id,
                artifact.sha256,
                destination.display(),
                conflict.installed_sha256()
            )));
        }
    }
    Ok(())
}

/// Releases a manifest-proven OptiScaler artifact from its exact journal
/// receipt. `Reused` describes adoption provenance only: the caller may
/// release it if the operation was admitted as the typed artifact role.
/// A present baseline on an Owned runtime has no generic release route, so
/// it fails closed instead of being reconstructed by a path-based copy.
pub(in crate::addons::optiscaler) fn release_exact_file(
    mutation: &mut PreparedFileMutation<'_>,
    path: &Path,
    installed: &FileReceipt,
    baseline: &OptiScalerFileBaseline,
    managed_root: Option<&Path>,
    changed: &mut Vec<String>,
) -> Result<(), ServiceError> {
    let managed_root = managed_root
        .filter(|root| crate::paths::is_within(path, root) && !crate::paths::same_path(path, root));
    let live = match managed_root {
        Some(root) => maybe_exact_managed_receipt_from_live(root, path, installed.ownership())?,
        None => maybe_exact_receipt_from_live(path, installed.ownership())?,
    };
    let Some(live) = live else {
        // A missing previously managed endpoint is already in its required
        // terminal state. Consume the journal's exact Verify(Absent) rather
        // than silently skipping it: storage must see the absence proof
        // before it can clear the durable binding.
        mutation.verify_unchanged(path)?;
        return Ok(());
    };
    if live.identity() != installed.identity() || live.digest() != installed.digest() {
        return Err(failed(format!(
            "OptiScaler exact artifact drifted before release: {}",
            path.display()
        )));
    }
    if installed.ownership() == FileOwnership::Reused
        || matches!(baseline, OptiScalerFileBaseline::Absent)
    {
        mutation.delete_file_exact(path, installed)?;
        changed.push(path.to_string_lossy().into_owned());
    } else {
        // A baseline must be restored by an identity-preserving custody
        // relocation. The current public API intentionally does not expose a
        // path-based copy as authority.
        return Err(failed(format!(
            "OptiScaler runtime with a present Owned baseline cannot be released without an exact restoration transition: {}",
            path.display()
        )));
    }
    Ok(())
}
