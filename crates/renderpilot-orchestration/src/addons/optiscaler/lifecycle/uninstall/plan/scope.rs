use super::*;

pub(in crate::addons::optiscaler::lifecycle::uninstall) fn uninstall_scope(
    state: &OptiScalerInstallState,
    topology: &GameProxyTopology,
    plan: &UninstallFsPlan,
) -> Result<MutationScope, ServiceError> {
    let target_dir = PathBuf::from(state.target_dir.as_str());
    let mut roots = vec![target_dir.clone()];
    for step in &plan.precommit {
        for path in step.touched_paths() {
            if crate::paths::is_within(path, &target_dir) {
                continue;
            }
            let parent = path.parent().ok_or_else(|| {
                failed(format!("OptiScaler path has no parent: {}", path.display()))
            })?;
            roots.push(existing_scope_root(parent)?);
        }
    }
    for receipt in &plan.postcommit_directories {
        let path = Path::new(receipt.path.as_str());
        if !crate::paths::is_within(path, &target_dir) {
            let parent = path.parent().ok_or_else(|| {
                failed(format!(
                    "OptiScaler directory has no parent: {}",
                    path.display()
                ))
            })?;
            roots.push(existing_scope_root(parent)?);
        }
    }
    let root_slot = Path::new(topology.root_slot.as_str());
    let root_parent = root_slot
        .parent()
        .ok_or_else(|| failed("OptiScaler proxy root has no parent"))?;
    if !crate::paths::is_within(root_slot, &target_dir) {
        roots.push(existing_scope_root(root_parent)?);
    }
    let scheduled_keys = plan
        .postcommit_directories
        .iter()
        .map(|receipt| crate::paths::normalized_key(Path::new(receipt.path.as_str())))
        .collect::<HashSet<_>>();
    if roots
        .iter()
        .any(|root| scheduled_keys.contains(&crate::paths::normalized_key(root)))
    {
        return Err(failed(
            "OptiScaler postcommit directory cannot alias a journal custody root",
        ));
    }
    MutationScope::new(roots)
}

fn existing_scope_root(path: &Path) -> Result<PathBuf, ServiceError> {
    let mut cursor = path;
    loop {
        match std::fs::symlink_metadata(cursor) {
            Ok(metadata) if metadata.is_dir() => return Ok(cursor.to_path_buf()),
            Ok(_) => {
                return Err(failed(format!(
                    "OptiScaler mutation scope parent is not a directory: {}",
                    cursor.display()
                )));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                cursor = cursor
                    .parent()
                    .ok_or_else(|| failed("OptiScaler mutation path has no reachable ancestor"))?;
            }
            Err(error) => {
                return Err(failed(format!(
                    "failed to inspect OptiScaler mutation scope {}: {error}",
                    cursor.display()
                )));
            }
        }
    }
}
