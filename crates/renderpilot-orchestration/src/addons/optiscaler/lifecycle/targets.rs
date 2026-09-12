use super::*;

pub(in crate::addons::optiscaler) fn native_module_targets(
    context: &Context,
    game_id: &GameId,
    release: &OptiScalerRelease,
    modules: &HashSet<String>,
    prepared: &[PreparedNativeModule],
    target_dir: &Path,
) -> Result<Vec<NativeTarget>, ServiceError> {
    let game = context.storage().require_game(game_id)?;
    let install_root = Path::new(game.install_path().as_str());
    let components = context.storage().list_components_for_game(game_id)?;
    let mut module_ids = modules.iter().collect::<Vec<_>>();
    module_ids.sort();
    let mut targets = Vec::new();
    for module in module_ids {
        if release
            .members
            .iter()
            .any(|member| member.module == *module)
            || native_module_spec(module).is_none()
        {
            continue;
        }
        if let Some(component) = native_module_component(&components, module) {
            for file in component.files() {
                let path = PathBuf::from(file.path().as_str());
                if !crate::paths::is_within(&path, install_root) {
                    return Err(failed(format!(
                        "native OptiScaler module {module} points outside the game installation: {}",
                        path.display()
                    )));
                }
                let install_name =
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .ok_or_else(|| {
                            failed(format!(
                                "native OptiScaler module {module} has no valid file name: {}",
                                path.display()
                            ))
                        })?;
                let destination = native_module_download_target(module, target_dir, install_name)?;
                if targets.iter().any(|target: &NativeTarget| {
                    crate::paths::same_path(&target.destination, &destination)
                }) {
                    continue;
                }
                targets.push(NativeTarget {
                    module_id: module.clone(),
                    source: path,
                    destination,
                });
            }
            continue;
        }
        let artifact = prepared
            .iter()
            .find(|prepared| prepared.module_id == *module)
            .map(|prepared| &prepared.artifact)
            .ok_or_else(|| failed(format!(
                "OptiScaler module {module} has neither a native component nor a prepared NVIDIA artifact"
            )))?;
        for file in artifact.files() {
            let source = PathBuf::from(file.path().as_str());
            let install_name = file
                .install_as()
                .map(str::to_owned)
                .or_else(|| (artifact.files().len() == 1).then(|| artifact.file_name().to_owned()))
                .or_else(|| {
                    source
                        .file_name()
                        .and_then(|name| name.to_str())
                        .map(str::to_owned)
                })
                .ok_or_else(|| failed("prepared NVIDIA artifact has no install name"))?;
            let install_path = Path::new(&install_name);
            if install_path.components().count() != 1
                || !native_module_matches_path(module, install_path)
            {
                return Err(failed(format!(
                    "NVIDIA artifact {} contains an unexpected file for module {module}: {}",
                    artifact.id().as_str(),
                    install_name
                )));
            }
            targets.push(NativeTarget {
                module_id: module.clone(),
                source,
                destination: native_module_download_target(module, target_dir, &install_name)?,
            });
        }
    }
    Ok(targets)
}

pub(in crate::addons::optiscaler) fn native_module_download_target(
    module_id: &str,
    target_dir: &Path,
    install_name: &str,
) -> Result<PathBuf, ServiceError> {
    match module_id {
        // This is the layout documented for FSR/XeSS-only games which need a
        // supplied DLSS implementation. It is deliberately not shared by RR:
        // RR requires engine integration and has no generic install target.
        "nvidia_sr" => Ok(target_dir.join("OptiScaler").join(install_name)),
        _ => Err(failed(format!(
            "native OptiScaler module {module_id} has no reviewed download placement"
        ))),
    }
}

/// Recovers the exact feature directory managed by releases which exposed RR
/// as a generic module. The receipt is the source of truth: no historical
/// destination is guessed and no path-specific cleanup rule is required.
pub(in crate::addons::optiscaler) fn rr_feature_receipt_dir(
    managed_files: &[ManagedAddonFile],
) -> Option<PathBuf> {
    managed_files
        .iter()
        .map(|managed| Path::new(managed.path().as_str()))
        .find(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.eq_ignore_ascii_case("nvngx_dlssd.dll"))
        })
        .and_then(Path::parent)
        .map(Path::to_path_buf)
}

pub(in crate::addons::optiscaler) fn ensure_native_modules_available(
    release: &OptiScalerRelease,
    modules: &HashSet<String>,
    targets: &[NativeTarget],
) -> Result<(), ServiceError> {
    for module in modules {
        if release
            .members
            .iter()
            .any(|member| member.module == *module)
        {
            continue;
        }
        if native_module_spec(module).is_none() {
            continue;
        }
        let sources = targets
            .iter()
            .filter(|target| target.module_id == *module)
            .map(|target| &target.source)
            .collect::<Vec<_>>();
        if sources.is_empty() {
            return Err(failed(format!(
                "selected native OptiScaler module {module} has no detected game component"
            )));
        }
        if sources.iter().any(|source| {
            !source.is_file()
                || renderpilot_detection::analyze_executable(source).architecture()
                    != Some(renderpilot_domain::Architecture::X64)
        }) {
            return Err(failed(format!(
                "selected native OptiScaler module {module} contains a missing or invalid x64 PE image"
            )));
        }
    }
    Ok(())
}
