use super::super::runtime;
use super::*;

pub(super) struct TargetPlan<'a> {
    pub(super) proxy_path: PathBuf,
    pub(super) new_paths: HashMap<String, PathBuf>,
    pub(super) release_write_paths: HashSet<String>,
    pub(super) native_targets: Vec<NativeTarget>,
    pub(super) native_paths: NativeModulePaths,
    pub(super) artifact_targets: Vec<(&'a PreparedModuleArtifact, PathBuf)>,
    pub(super) artifact_write_paths: HashSet<String>,
    pub(super) runtime_plans: HashMap<String, runtime::RuntimeApplyPlan>,
    pub(super) native_expected_sha256: HashMap<String, Sha256Hash>,
    pub(super) native_copy_paths: HashSet<String>,
    pub(super) retained_fsr: Vec<RetainedFsrEntryPointPlan>,
}

pub(super) fn plan_targets<'a>(
    prepared: &'a ApplyPlan<'_>,
) -> Result<TargetPlan<'a>, ServiceError> {
    let ApplyPlan {
        context,
        game_id,
        old_state,
        release,
        modules,
        artifacts,
        target,
        ..
    } = prepared;
    let ArtifactSet {
        module_artifacts,
        native_artifacts,
        ..
    } = artifacts;
    let proxy_path = target.proxy.slot.clone();
    let new_paths = target_paths(release, modules, &target.dir, &proxy_path);
    let release_write_paths = changed_release_paths(release, modules, &new_paths)?;
    let retained_fsr = plan_retained_fsr_entry_points(
        context,
        game_id,
        old_state.as_ref(),
        release,
        modules,
        &new_paths,
        &release_write_paths,
    )?;
    let native_targets = native_module_targets(
        context,
        game_id,
        release,
        modules,
        native_artifacts,
        &target.dir,
    )?;
    let native_paths = NativeModulePaths::from_targets(&native_targets);
    let artifact_targets = module_artifacts
        .iter()
        .map(|artifact| (artifact, target.dir.join(&artifact.manifest.target)))
        .collect::<Vec<_>>();
    let native_expected_sha256 = native_targets
        .iter()
        .map(|target| {
            let source_hash =
                renderpilot_detection::sha256_file(&target.source).map_err(|error| {
                    failed(format!(
                        "failed to hash prepared native module {}: {error}",
                        target.source.display()
                    ))
                })?;
            Ok((
                crate::paths::normalized_key(&target.destination),
                source_hash,
            ))
        })
        .collect::<Result<HashMap<_, _>, ServiceError>>()?;
    let mut runtime_plans = HashMap::new();
    for target in &native_targets {
        let key = crate::paths::normalized_key(&target.destination);
        let expected = native_expected_sha256.get(&key).ok_or_else(|| {
            failed(format!(
                "native target hash is missing for {}",
                target.destination.display()
            ))
        })?;
        let plan = runtime::plan_runtime_target(
            old_state
                .as_ref()
                .and_then(|state| prior_runtime_binding(Some(state), &target.destination)),
            &target.module_id,
            &target.destination,
            expected.clone(),
        )?;
        if runtime_plans.insert(key, plan).is_some() {
            return Err(failed(format!(
                "duplicate OptiScaler runtime target: {}",
                target.destination.display()
            )));
        }
    }
    for (artifact, destination) in &artifact_targets {
        let key = crate::paths::normalized_key(destination);
        let plan = runtime::plan_runtime_target(
            old_state
                .as_ref()
                .and_then(|state| prior_runtime_binding(Some(state), destination)),
            &artifact.module_id,
            destination,
            artifact.sha256.clone(),
        )?;
        if runtime_plans.insert(key, plan).is_some() {
            return Err(failed(format!(
                "duplicate OptiScaler runtime target: {}",
                destination.display()
            )));
        }
    }
    let native_copy_paths = native_targets
        .iter()
        .filter_map(|target| {
            let key = crate::paths::normalized_key(&target.destination);
            runtime_plans
                .get(&key)
                .filter(|plan| plan.requires_write())
                .map(|_| key)
        })
        .collect::<HashSet<_>>();
    let artifact_write_paths = artifact_targets
        .iter()
        .filter_map(|(_, destination)| {
            let key = crate::paths::normalized_key(destination);
            runtime_plans
                .get(&key)
                .filter(|plan| plan.requires_write())
                .map(|_| key)
        })
        .collect::<HashSet<_>>();
    ensure_native_modules_available(release, modules, &native_targets)?;
    ensure_native_claims_compatible(context, game_id, &native_targets)?;
    ensure_module_artifact_claims_compatible(context, game_id, &artifact_targets)?;
    Ok(TargetPlan {
        proxy_path,
        new_paths,
        release_write_paths,
        native_targets,
        native_paths,
        artifact_targets,
        artifact_write_paths,
        runtime_plans,
        native_expected_sha256,
        native_copy_paths,
        retained_fsr,
    })
}

/// Plans the one supported external baseline: an official AMD FSR entry-point
/// DLL.  It is never adopted as OptiScaler.  A fresh plan first relocates the
/// exact game DLL to its deterministic sibling backup; later plans reuse that
/// immutable backup and only replace/verify the OptiScaler target.
fn plan_retained_fsr_entry_points(
    context: &Context,
    game_id: &GameId,
    old_state: Option<&OptiScalerInstallState>,
    release: &OptiScalerRelease,
    modules: &HashSet<String>,
    new_paths: &HashMap<String, PathBuf>,
    release_write_paths: &HashSet<String>,
) -> Result<Vec<RetainedFsrEntryPointPlan>, ServiceError> {
    let mut plans = Vec::new();
    let other_claims = context
        .storage()
        .list_installed_addons()?
        .into_iter()
        .filter(|record| record.game_id() == game_id && record.kind() != AddonKind::OptiScaler)
        .flat_map(|record| {
            record
                .managed_files()
                .iter()
                .map(|file| PathBuf::from(file.path().as_str()))
                .chain(
                    record
                        .created_files()
                        .iter()
                        .map(|path| PathBuf::from(path.as_str())),
                )
                .chain(
                    record
                        .backed_up_files()
                        .iter()
                        .map(|path| PathBuf::from(path.as_str())),
                )
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    for member in selected_members(release, modules).filter(|member| member.target != "$proxy") {
        let Some(target) = new_paths.get(&member.archive_path.to_ascii_lowercase()) else {
            continue;
        };
        let Some(file_name) = target.file_name().and_then(|name| name.to_str()) else {
            return Err(failed(format!(
                "OptiScaler release entry point has no file name: {}",
                target.display()
            )));
        };
        if !renderpilot_domain::fsr::is_entry_point(file_name) {
            continue;
        }

        let previous = old_state
            .and_then(|state| prior_release_receipt(Some(state), target))
            .and_then(|receipt| {
                receipt
                    .baseline
                    .retained_original()
                    .map(|original| (receipt, original))
            });
        if let Some((_receipt, (component_id, custody_path, original))) = previous {
            plans.push(RetainedFsrEntryPointPlan {
                target: target.clone(),
                original_backup: PathBuf::from(custody_path.as_str()),
                component_id: component_id.clone(),
                original: original.clone(),
                action: RetainedFsrOriginalAction::Preserve,
            });
            continue;
        }

        // A byte-identical live OptiScaler entry point takes the existing exact
        // artifact/adoption route.  It never receives a synthetic FSR backup.
        if !release_write_paths.contains(&crate::paths::normalized_key(target)) {
            continue;
        }

        let live = maybe_exact_receipt_from_live(target, FileOwnership::Reused)?;
        let Some(live) = live else { continue };
        let backup = retained_fsr_original_backup_path(target)?;
        if maybe_exact_receipt_from_live(&backup, FileOwnership::Reused)?.is_some() {
            return Err(failed(format!(
                "OptiScaler original FSR backup slot is occupied: {}",
                backup.display()
            )));
        }
        if other_claims.iter().any(|managed| {
            crate::paths::same_path(managed, target) || crate::paths::same_path(managed, &backup)
        }) {
            return Err(failed(format!(
                "OptiScaler cannot retain an AMD FSR entry point claimed by another add-on: {}",
                target.display()
            )));
        }
        let mut candidates = Vec::new();
        for component in context.storage().list_components_for_game(game_id)? {
            if component.technology().family() != renderpilot_domain::LibraryTechnology::AmdFsr {
                continue;
            }
            for file in component.files() {
                if crate::paths::same_path(Path::new(file.path().as_str()), target)
                    && file.sha256().is_some_and(|digest| digest == live.digest())
                {
                    candidates.push(component.id().clone());
                }
            }
        }
        if candidates.len() != 1 {
            return Err(failed(format!(
                "OptiScaler requires exactly one exact AMD FSR component receipt for existing entry point {}",
                target.display()
            )));
        }
        plans.push(RetainedFsrEntryPointPlan {
            target: target.clone(),
            original_backup: backup,
            component_id: candidates
                .into_iter()
                .next()
                .expect("checked exact candidate count"),
            original: live,
            action: RetainedFsrOriginalAction::Acquire,
        });
    }
    for prior in old_state
        .into_iter()
        .flat_map(|state| state.release_files.iter())
    {
        let Some((component_id, custody_path, original)) = prior.baseline.retained_original()
        else {
            continue;
        };
        let target = PathBuf::from(prior.path.as_str());
        if plans
            .iter()
            .any(|plan| crate::paths::same_path(&plan.target, &target))
        {
            continue;
        }
        plans.push(RetainedFsrEntryPointPlan {
            target,
            original_backup: PathBuf::from(custody_path.as_str()),
            component_id: component_id.clone(),
            original: original.clone(),
            action: RetainedFsrOriginalAction::Restore {
                active: prior.installed.clone(),
            },
        });
    }
    plans.sort_by_key(|plan| crate::paths::normalized_key(&plan.target));
    if plans
        .windows(2)
        .any(|pair| crate::paths::same_path(&pair[0].target, &pair[1].target))
    {
        return Err(failed(
            "OptiScaler release has duplicate AMD FSR entry-point targets",
        ));
    }
    Ok(plans)
}

fn retained_fsr_original_backup_path(target: &Path) -> Result<PathBuf, ServiceError> {
    let parent = target
        .parent()
        .ok_or_else(|| failed("OptiScaler FSR entry point has no parent directory"))?;
    let name = target
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            failed(format!(
                "OptiScaler FSR entry point has no valid file name: {}",
                target.display()
            ))
        })?;
    Ok(parent.join(format!(".{name}.renderpilot-optiscaler-original")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::addons::optiscaler::types::{
        OptiScalerArchiveMember, OptiScalerReleaseProvider, OptiScalerReleaseSource,
    };
    use renderpilot_application::{ComponentRepository, GameRepository};
    use renderpilot_domain::{
        ComponentFile, ComponentId, ComponentKind, GameIdentity, GameInstallation, GameRuntime,
        Launcher, LibraryComponent, Platform, Swappability,
    };

    #[test]
    fn exact_amd_fsr_entry_point_is_retained_as_an_original_backup() {
        let database = tempfile::tempdir().expect("database");
        let root = tempfile::tempdir().expect("game root");
        let context = Context::open_at(database.path().join("catalog.sqlite")).expect("context");
        let game_id = GameId::new("manual:retained-fsr-original").expect("game id");
        context
            .storage()
            .upsert_game(&GameInstallation::new(
                GameIdentity::new(game_id.clone(), "FSR game", Launcher::Manual).expect("identity"),
                Platform::Windows,
                GameRuntime::NativeWindows,
                path_ref(root.path()).expect("game root"),
            ))
            .expect("game");

        let target = root.path().join("amd_fidelityfx_dx12.dll");
        std::fs::write(&target, b"official AMD FSR DLL").expect("official FSR entry point");
        let digest = renderpilot_detection::sha256_file(&target).expect("digest");
        let component = LibraryComponent::new(
            ComponentId::new("component:official-amd-fsr").expect("component id"),
            game_id.clone(),
            ComponentKind::NativeLibrary,
            renderpilot_domain::LibraryTechnology::AmdFsr,
            Swappability::Swappable,
        )
        .with_file(
            ComponentFile::new(path_ref(&target).expect("component path")).with_sha256(digest),
        );
        context
            .storage()
            .replace_components_for_game(&game_id, &[component])
            .expect("component catalog");

        let release = OptiScalerRelease {
            id: "test-release".to_owned(),
            source: OptiScalerReleaseSource {
                provider: OptiScalerReleaseProvider::GithubRelease,
                repository: "example/optiscaler".to_owned(),
                tag: "v1.0.0".to_owned(),
                asset: "optiscaler.7z".to_owned(),
            },
            archive_sha256: "a".repeat(64),
            archive_size: 1,
            config_schema: 1,
            members: vec![OptiScalerArchiveMember {
                archive_path: "entry.dll".to_owned(),
                target: "amd_fidelityfx_dx12.dll".to_owned(),
                sha256: "b".repeat(64),
                size: 1,
                module: "core".to_owned(),
                pe_x64: false,
            }],
        };
        let targets = HashMap::from([("entry.dll".to_owned(), target.clone())]);
        let write_paths = HashSet::from([crate::paths::normalized_key(&target)]);

        let retained = plan_retained_fsr_entry_points(
            &context,
            &game_id,
            None,
            &release,
            &HashSet::from(["core".to_owned()]),
            &targets,
            &write_paths,
        )
        .expect("retained FSR plan");

        assert_eq!(retained.len(), 1);
        assert_eq!(retained[0].target, target);
        assert_eq!(
            retained[0].original_backup,
            root.path()
                .join(".amd_fidelityfx_dx12.dll.renderpilot-optiscaler-original")
        );
        assert_eq!(
            retained[0].component_id,
            ComponentId::new("component:official-amd-fsr").expect("component id")
        );
        assert_eq!(retained[0].action, RetainedFsrOriginalAction::Acquire);
        assert_eq!(retained[0].original.ownership(), FileOwnership::Reused);
    }
}
