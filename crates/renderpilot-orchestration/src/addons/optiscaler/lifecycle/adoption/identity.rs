//! Exact adoption of unmanaged immutable releases.

use super::super::super::identity::matches_optional as file_matches;
use super::super::*;

/// Adopts an exact unmanaged release when every release-owned file matches one
/// immutable manifest entry. This creates receipts but never rewrites user
/// files. The release identity is derived from the matched bytes, so recovery
/// remains reliable after database loss.
pub(crate) async fn adopt_exact(
    context: &Context,
    manifest: &OptiScalerManifest,
    game_id: &GameId,
    policy: AdoptionPolicy,
    manual_override: bool,
    availability: &EvaluatedAvailability,
) -> Result<Option<OptiScalerOperationResult>, ServiceError> {
    let guard =
        crate::mutation_boundary::enter_game_mutation_boundary_async(context, game_id).await?;
    let current =
        matcher::evaluation_off_runtime(context, manifest, game_id, manual_override).await?;
    if current.install_state.is_some() {
        return Ok(None);
    }
    policy.ensure_allowed(&current)?;
    if !current.unmanaged
        || current.target_exe != availability.target_exe
        || current.target_dir != availability.target_dir
        || current.proxy.slot != availability.proxy.slot
        || current.proxy.chain_reshade != availability.proxy.chain_reshade
        || current.proxy.downstream_path != availability.proxy.downstream_path
    {
        return Err(failed(
            "OptiScaler unmanaged target changed while it was being adopted; retry",
        ));
    }
    let manifest = manifest.clone();
    let game_id = game_id.clone();
    adopt_exact_locked(context, &manifest, &game_id, &guard, &current)
}

fn adopt_exact_locked(
    context: &Context,
    manifest: &OptiScalerManifest,
    game_id: &GameId,
    _guard: &crate::game_mutation_lock::GameMutationGuard,
    current: &EvaluatedAvailability,
) -> Result<Option<OptiScalerOperationResult>, ServiceError> {
    let availability = current;
    if availability.proxy.conflict.is_some() {
        return Ok(None);
    }
    let target_dir = availability
        .target_dir
        .clone()
        .ok_or_else(|| failed("no unmanaged OptiScaler target directory"))?;
    let target_exe = availability
        .target_exe
        .clone()
        .ok_or_else(|| failed("no unmanaged OptiScaler target executable"))?;
    let proxy_path = availability.proxy.slot.clone();
    let mut candidates = Vec::new();
    for release in &manifest.releases {
        if let Some(modules) = exact_adoption_modules(manifest, release, &target_dir, &proxy_path)?
        {
            candidates.push((release, modules));
        }
    }
    if candidates.len() != 1 {
        return Ok(None);
    }
    let (release, modules) = candidates.pop().ok_or_else(|| {
        failed("the exact OptiScaler adoption candidate disappeared during validation")
    })?;
    let downstream = if availability.proxy.chain_reshade {
        let path = availability
            .proxy
            .downstream_path
            .clone()
            .ok_or_else(|| failed("the detected ReShade chain has no downstream path"))?;
        if !crate::paths::is_within(&path, &target_dir)
            || !crate::addons::reshade::scan::is_reshade_proxy_file(&path)
        {
            return Ok(None);
        }
        let receipt = exact_receipt_from_live(&path, renderpilot_domain::FileOwnership::Reused)?;
        Some(ProxyLink {
            implementation: ProxyImplementation::ReShade,
            path: path_ref(&path)?,
            receipt,
        })
    } else {
        None
    };
    let root_prestate = if downstream.is_some() {
        ProxyRootPrestate::RelocatedDownstream
    } else {
        ProxyRootPrestate::Absent
    };
    let topology = GameProxyTopology {
        id: format!("optiscaler:{}", game_id.as_str()),
        game_id: game_id.clone(),
        root_slot: path_ref(&proxy_path)?,
        outer: ProxyLink {
            implementation: ProxyImplementation::OptiScaler,
            path: path_ref(&proxy_path)?,
            receipt: exact_receipt_from_live(
                &proxy_path,
                renderpilot_domain::FileOwnership::Reused,
            )?,
        },
        // With no surviving topology receipt, the only safe recovery target
        // for a recognized downstream is the root slot vacated by OptiScaler.
        downstream_origin: downstream
            .as_ref()
            .map(|_| path_ref(&proxy_path))
            .transpose()?,
        downstream,
        root_prestate,
    };
    let (release_files, configuration_baseline) =
        adopted_release_file_receipts(release, &modules, &target_dir, &proxy_path)?;
    let runtime_bindings =
        adopted_module_runtime_bindings(manifest, release, &modules, &target_dir)?;
    let mut module_list = modules.into_iter().collect::<Vec<_>>();
    module_list.sort();
    let state = renderpilot_domain::from_new_adoption(
        renderpilot_domain::OptiScalerInstallStateParts {
            game_id: game_id.clone(),
            release_id: release.id.clone(),
            manifest_revision: manifest.revision.clone(),
            target_exe_path: path_ref(&target_exe)?,
            target_dir: path_ref(&target_dir)?,
            modules: module_list,
            release_files,
            runtime_bindings,
            directory_receipts: Vec::new(),
            source: Some(super::super::super::source::release_download_url(release)?.to_string()),
            archive_sha256: Some(validated_manifest_hash(
                &release.archive_sha256,
                "release archive",
            )?),
            proxy_topology_id: Some(topology.id.clone()),
            config_schema: release.config_schema,
            config_base_release: release.id.clone(),
            adoption_state: OptiScalerAdoptionState::AdoptedExact,
            prerequisite_binding: current.accepted_prerequisite_binding,
            created_at: None,
            updated_at: None,
        },
        configuration_baseline,
    )
    .map_err(|error| failed(error.to_string()))?;
    topology
        .validate()
        .map_err(|error| failed(error.to_string()))?;
    // Adoption is metadata-only from the filesystem's perspective. The
    // aggregate validates every live receipt and publishes state/topology
    // atomically without reserving an empty filesystem journal.
    context
        .storage()
        .commit_game_mutation(renderpilot_storage_sqlite::GameMutationCommit {
            game_id,
            component_set: None,
            baseline_mutations: &[],
            addon: renderpilot_storage_sqlite::InstalledAddonMutation::OptiScaler(
                renderpilot_storage_sqlite::OptiScalerAggregateMutation::AdoptExactMetadata {
                    state: &state,
                    topology: &topology,
                },
            ),
            mutation_id: None,
        })?;
    Ok(Some(OptiScalerOperationResult {
        kind: AddonKind::OptiScaler,
        state: Some((&state).into()),
        changed_paths: Vec::new(),
        preserved_paths: vec![proxy_path.to_string_lossy().into_owned()],
        config_conflicts: Vec::new(),
    }))
}

fn adopted_release_file_receipts(
    release: &OptiScalerRelease,
    modules: &HashSet<String>,
    target_dir: &Path,
    proxy_path: &Path,
) -> Result<
    (
        Vec<OptiScalerFileReceipt>,
        renderpilot_domain::OptiScalerConfigurationBaseline,
    ),
    ServiceError,
> {
    let targets = target_paths(release, modules, target_dir, proxy_path);
    let mut receipts = Vec::new();
    let mut configuration_baseline = None;
    for member in selected_members(release, modules) {
        let Some(path) = targets.get(&member.archive_path.to_ascii_lowercase()) else {
            continue;
        };
        // The configuration baseline retains bytes read through the same
        // authority entry as its receipt.  A second path hash would permit
        // the bytes checked here to diverge from the identity persisted below.
        let is_configuration = member.target == "OptiScaler.ini";
        let (actual_receipt, configuration_bytes) = if is_configuration {
            let (bytes, receipt) = read_exact_file_from_live(path, FileOwnership::Reused)?
                .ok_or_else(|| failed(format!("file is absent: {}", path.display())))?;
            (receipt, Some(bytes))
        } else {
            (exact_receipt_from_live(path, FileOwnership::Reused)?, None)
        };
        let actual = actual_receipt.digest().clone();
        let role = match member.target.as_str() {
            "$proxy" => None,
            "OptiScaler.ini" => Some(OptiScalerFileRole::Configuration),
            _ => Some(OptiScalerFileRole::Runtime),
        };
        if role != Some(OptiScalerFileRole::Configuration) {
            let expected = Sha256Hash::new(member.sha256.clone())
                .map_err(|error| failed(format!("invalid release member hash: {error}")))?;
            if actual != expected {
                return Err(failed(format!(
                    "adopted OptiScaler file {} no longer matches release {}",
                    path.display(),
                    release.id
                )));
            }
        }
        let Some(role) = role else {
            continue;
        };
        receipts.push(OptiScalerFileReceipt {
            path: path_ref(path)?,
            installed: actual_receipt.clone(),
            role,
            cleanup: if role == OptiScalerFileRole::Configuration {
                OptiScalerFileCleanup::PreserveUnchanged
            } else {
                OptiScalerFileCleanup::RemoveIfUnchanged
            },
            baseline: renderpilot_domain::OptiScalerReleaseFileBaseline::Absent,
        });
        if let Some(bytes) = configuration_bytes {
            configuration_baseline = Some(
                renderpilot_domain::OptiScalerConfigurationBaseline::present(actual_receipt, bytes)
                    .map_err(|error| failed(error.to_string()))?,
            );
        }
    }
    receipts.sort_by(|left, right| left.path.as_str().cmp(right.path.as_str()));
    let configuration_baseline =
        configuration_baseline.ok_or_else(|| failed("adopted release has no OptiScaler.ini"))?;
    Ok((receipts, configuration_baseline))
}

fn adopted_module_runtime_bindings(
    manifest: &OptiScalerManifest,
    release: &OptiScalerRelease,
    modules: &HashSet<String>,
    target_dir: &Path,
) -> Result<Vec<renderpilot_domain::OptiScalerModuleRuntimeBinding>, ServiceError> {
    let mut bindings = Vec::new();
    for module in modules {
        let Some(artifact) = matcher::module_artifact_for_release(manifest, release, module) else {
            continue;
        };
        let path = target_dir.join(&artifact.target);
        if !file_matches_module_artifact(&path, artifact)? {
            return Err(failed(format!(
                "adopted OptiScaler module {module} no longer matches its pinned artifact"
            )));
        }
        let installed = exact_receipt_from_live(&path, renderpilot_domain::FileOwnership::Reused)?;
        let baseline = renderpilot_domain::OptiScalerFileBaseline::Present {
            receipt: installed.clone(),
        };
        bindings.push(renderpilot_domain::OptiScalerModuleRuntimeBinding {
            module: module.clone(),
            path: path_ref(&path)?,
            installed,
            baseline,
        });
    }
    bindings.sort_by(|left, right| left.module.cmp(&right.module));
    Ok(bindings)
}

fn file_matches_module_artifact(
    path: &Path,
    artifact: &super::super::super::types::OptiScalerModuleArtifact,
) -> Result<bool, ServiceError> {
    let expected = validated_manifest_hash(&artifact.sha256, "independent module artifact")?;
    Ok(file_matches(path, Some(&expected))
        && (!artifact.pe_x64
            || renderpilot_detection::analyze_executable(path).architecture()
                == Some(renderpilot_domain::Architecture::X64)))
}

fn exact_adoption_modules(
    manifest: &OptiScalerManifest,
    release: &OptiScalerRelease,
    target_dir: &Path,
    proxy_path: &Path,
) -> Result<Option<HashSet<String>>, ServiceError> {
    let core_proxy_member = release
        .members
        .iter()
        .find(|member| member.module == "core" && member.target == "$proxy")
        .ok_or_else(|| failed(format!("release {} has no core proxy member", release.id)))?;
    let core_proxy_hash = validated_manifest_hash(&core_proxy_member.sha256, "core proxy member")?;
    if !file_matches(proxy_path, Some(&core_proxy_hash)) {
        return Ok(None);
    }
    let mut modules = HashSet::from(["core".to_owned()]);
    for module in &manifest.modules {
        if module.id == "core"
            || !matcher::module_is_installable(manifest, module)
            || native_module_spec(&module.id).is_some()
        {
            continue;
        }
        if let Some(artifact) = matcher::module_artifact_for_release(manifest, release, &module.id)
        {
            let target = target_dir.join(&artifact.target);
            if file_matches_module_artifact(&target, artifact)? {
                modules.insert(module.id.clone());
            }
            continue;
        }
        let members = release
            .members
            .iter()
            .filter(|member| member.module == module.id)
            .collect::<Vec<_>>();
        if members.is_empty() {
            continue;
        }
        let mut exact = true;
        for member in members {
            if !unmanaged_member_matches(member, target_dir, proxy_path)? {
                exact = false;
                break;
            }
        }
        if exact {
            modules.insert(module.id.clone());
        }
    }
    modules = validate_module_selection(manifest, &release.id, &modules)?;
    // Dependency closure may add a module which was not part of the initially
    // observed exact set. Adoption is allowed only if every resulting member is
    // present too; otherwise a receipt would claim files it never proved.
    for module in &modules {
        if native_module_spec(module).is_some() {
            return Ok(None);
        }
        if let Some(artifact) = matcher::module_artifact_for_release(manifest, release, module)
            && !file_matches_module_artifact(&target_dir.join(&artifact.target), artifact)?
        {
            return Ok(None);
        }
        for member in release
            .members
            .iter()
            .filter(|member| member.module == *module)
        {
            if !unmanaged_member_matches(member, target_dir, proxy_path)? {
                return Ok(None);
            }
        }
    }
    Ok(Some(modules))
}

fn unmanaged_member_matches(
    member: &super::super::super::types::OptiScalerArchiveMember,
    target_dir: &Path,
    proxy_path: &Path,
) -> Result<bool, ServiceError> {
    let path = if member.target == "$proxy" {
        proxy_path.to_path_buf()
    } else {
        target_dir.join(&member.target)
    };
    if member.target == "OptiScaler.ini" {
        return Ok(maybe_exact_receipt_from_live(&path, FileOwnership::Reused)?.is_some());
    }
    let expected = validated_manifest_hash(&member.sha256, "release archive member")?;
    Ok(file_matches(&path, Some(&expected)))
}

fn validated_manifest_hash(value: &str, subject: &str) -> Result<Sha256Hash, ServiceError> {
    Sha256Hash::new(value).map_err(|error| {
        failed(format!(
            "validated {subject} has an invalid SHA-256: {error}"
        ))
    })
}
