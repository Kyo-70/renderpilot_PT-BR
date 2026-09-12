//! Managed release, configuration, proxy, and native-library drift detection.

use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum DriftIssue {
    ReleaseMissing,
    NativeBindingMissing(String),
}

#[derive(Debug, Clone, Default)]
pub(super) struct DriftEvaluation {
    pub(super) paths: Vec<PathBuf>,
    pub(super) issues: Vec<DriftIssue>,
}

impl DriftEvaluation {
    pub(super) fn repair_required(&self) -> bool {
        !self.paths.is_empty() || !self.issues.is_empty()
    }
}

pub(super) fn detect(
    context: &Context,
    manifest: &OptiScalerManifest,
    state: &renderpilot_domain::OptiScalerInstallState,
    proxy_slot: &Path,
    compatibility_invariants: &[super::super::types::ManagedIniValue],
) -> Result<DriftEvaluation, ServiceError> {
    let Some(release) = manifest
        .releases
        .iter()
        .find(|release| release.id == state.release_id)
    else {
        return Ok(DriftEvaluation {
            paths: Vec::new(),
            issues: vec![DriftIssue::ReleaseMissing],
        });
    };
    let selected: HashSet<_> = state.modules.iter().cloned().collect();
    let managed_bindings = super::super::lifecycle::managed_bindings_from_state(Some(state));
    let native_paths = super::super::lifecycle::NativeModulePaths::from_bindings(&managed_bindings);
    let mut evaluation = DriftEvaluation::default();
    for member in super::super::archive::selected_members(release, &selected) {
        let target = if member.target == "$proxy" {
            proxy_slot.to_path_buf()
        } else {
            Path::new(state.target_dir.as_str()).join(&member.target)
        };
        if member.target == "OptiScaler.ini" {
            let Ok(bytes) = std::fs::read(&target) else {
                evaluation.paths.push(target);
                continue;
            };
            let load_reshade = context
                .storage()
                .get_proxy_topology(&state.game_id)?
                .is_some_and(|topology| topology.downstream.is_some());
            let invariants = super::super::lifecycle::managed_config_invariants(
                release,
                &selected,
                Path::new(state.target_dir.as_str()),
                &native_paths,
                load_reshade,
                compatibility_invariants,
            );
            let parsed_config = super::super::config::parse_values(&bytes);
            let invariants_hold = invariants.iter().all(|invariant| {
                parsed_config.value_matches(&invariant.section, &invariant.key, &invariant.value)
            });
            if !invariants_hold {
                evaluation.paths.push(target);
            }
            continue;
        }
        match renderpilot_detection::sha256_file(&target) {
            Ok(hash) if hash.as_str() == member.sha256 => {}
            _ => evaluation.paths.push(target),
        }
    }
    for module in &state.modules {
        if let Some(artifact) = module_artifact_for_release(manifest, release, module) {
            let target = Path::new(state.target_dir.as_str()).join(&artifact.target);
            match renderpilot_detection::sha256_file(&target) {
                Ok(hash) if hash.as_str() == artifact.sha256 => {}
                _ => evaluation.paths.push(target),
            }
            continue;
        }
        if native_module_spec(module).is_none() {
            continue;
        }
        let module_paths = managed_bindings
            .iter()
            .filter(|file| native_module_matches_path(module, Path::new(file.path().as_str())))
            .map(|file| Path::new(file.path().as_str()).to_path_buf())
            .collect::<Vec<_>>();
        if module_paths.is_empty() {
            evaluation
                .issues
                .push(DriftIssue::NativeBindingMissing(module.clone()));
            continue;
        }
        for target in module_paths {
            let path = PathRef::new(target.to_string_lossy().into_owned())
                .map_err(|error| crate::failed(error.to_string()))?;
            let expected = managed_bindings
                .iter()
                .find(|file| file.path() == &path)
                .map(|file| file.installed_sha256().clone());
            if !super::super::identity::matches_optional(&target, expected.as_ref()) {
                evaluation.paths.push(target);
            }
        }
    }
    evaluation.paths.sort();
    evaluation.paths.dedup();
    Ok(evaluation)
}
