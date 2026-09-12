use super::*;

pub(in crate::addons::optiscaler::lifecycle::uninstall) fn canonical_postcommit_directories(
    receipts: &[renderpilot_domain::OptiScalerDirectoryReceipt],
) -> Result<Vec<renderpilot_domain::OptiScalerDirectoryReceipt>, ServiceError> {
    let mut result = receipts.to_vec();
    let mut keys = HashSet::new();
    for receipt in &result {
        if !keys.insert(crate::paths::normalized_key(Path::new(
            receipt.path.as_str(),
        ))) {
            return Err(failed(format!(
                "OptiScaler uninstall has duplicate postcommit directory receipt: {}",
                receipt.path
            )));
        }
    }
    result.sort_by(|left, right| {
        let left_path = Path::new(left.path.as_str());
        let right_path = Path::new(right.path.as_str());
        right_path
            .components()
            .count()
            .cmp(&left_path.components().count())
            .then_with(|| {
                crate::paths::normalized_key(left_path)
                    .cmp(&crate::paths::normalized_key(right_path))
            })
    });
    Ok(result)
}

/// Projects only the bounded directory closure owned by this uninstall.
///
/// Enumeration is performed through the retained directory authority. The
/// closed plan supplies the complete set of permitted direct child names; the
/// authority rejects every unknown, malformed, or link/reparse child before
/// returning observations.
pub(in crate::addons::optiscaler::lifecycle::uninstall) fn validate_directory_emptiness(
    directories: &[renderpilot_domain::OptiScalerDirectoryReceipt],
    precommit: &[UninstallStep],
) -> Result<(), ServiceError> {
    let scheduled_directories = directories
        .iter()
        .map(|receipt| crate::paths::normalized_key(Path::new(receipt.path.as_str())))
        .collect::<HashSet<_>>();
    let scheduled_directory_paths = directories
        .iter()
        .map(|receipt| PathBuf::from(receipt.path.as_str()))
        .collect::<Vec<_>>();
    let mut removal_receipts = HashMap::new();
    let mut removal_paths = Vec::new();
    let mut destinations = HashSet::new();
    let mut destination_paths = Vec::new();
    let mut surviving_destinations = Vec::new();
    let mut created_directories = HashSet::new();
    for step in precommit {
        match step {
            UninstallStep::PreserveConfiguration { plan } => {
                let key = crate::paths::normalized_key(&plan.destination);
                destinations.insert(key);
                destination_paths.push(plan.destination.clone());
                surviving_destinations.push(plan.destination.clone());
            }
            UninstallStep::RestoreConfiguration { path, .. } => {
                let key = crate::paths::normalized_key(path);
                destinations.insert(key);
                destination_paths.push(path.clone());
                surviving_destinations.push(path.clone());
            }
            UninstallStep::DeleteOwned { path, receipt, .. }
            | UninstallStep::DeleteReusedArtifact { path, receipt, .. }
            | UninstallStep::DeleteTopologyOuter { path, receipt, .. } => {
                removal_receipts.insert(crate::paths::normalized_key(path), receipt.clone());
                removal_paths.push(path.clone());
            }
            UninstallStep::RelocatePeer {
                source,
                destination,
                receipt,
                ..
            }
            | UninstallStep::RelocatePeerSidecar {
                source,
                destination,
                receipt,
                ..
            } => {
                removal_receipts.insert(crate::paths::normalized_key(source), receipt.clone());
                removal_paths.push(source.clone());
                let key = crate::paths::normalized_key(destination);
                destinations.insert(key);
                destination_paths.push(destination.clone());
                surviving_destinations.push(destination.clone());
            }
            UninstallStep::RestoreRetainedFsrOriginal {
                original_backup,
                target,
                original,
            } => {
                removal_receipts.insert(
                    crate::paths::normalized_key(original_backup),
                    original.clone(),
                );
                removal_paths.push(original_backup.clone());
                let key = crate::paths::normalized_key(target);
                destinations.insert(key);
                destination_paths.push(target.clone());
                surviving_destinations.push(target.clone());
            }
            UninstallStep::CreateDirectory { path } => {
                let key = crate::paths::normalized_key(path);
                destinations.insert(key.clone());
                destination_paths.push(path.clone());
                created_directories.insert(key);
                surviving_destinations.push(path.clone());
            }
            UninstallStep::VerifyNoMutation { .. } => {}
        }
    }
    if created_directories
        .iter()
        .any(|key| scheduled_directories.contains(key))
    {
        return Err(failed(
            "OptiScaler directory cannot be both created and scheduled for postcommit removal",
        ));
    }
    for destination in surviving_destinations {
        if scheduled_directory_paths.iter().any(|directory| {
            crate::paths::is_within(&destination, directory)
                && !scheduled_directories.contains(&crate::paths::normalized_key(&destination))
        }) {
            return Err(failed(format!(
                "OptiScaler postcommit directory contains a surviving mutation destination: {}",
                destination.display()
            )));
        }
    }
    for receipt in directories {
        let path = Path::new(receipt.path.as_str());
        let authority = crate::fs::VerifiedDir::open(path).map_err(|error| {
            failed(format!(
                "OptiScaler postcommit directory authority is unavailable at {}: {error}",
                path.display()
            ))
        })?;
        if authority.identity() != receipt.identity {
            return Err(failed(format!(
                "OptiScaler postcommit directory identity drifted at {}",
                path.display()
            )));
        }
        let mut reserved = Vec::new();
        let mut reserved_keys = HashSet::new();
        let mut reserve = |candidate: &Path| -> Result<(), ServiceError> {
            let Some(parent) = candidate.parent() else {
                return Ok(());
            };
            if crate::paths::normalized_key(parent) != crate::paths::normalized_key(path) {
                return Ok(());
            }
            let leaf = crate::fs::LeafName::from_path(candidate).map_err(|error| {
                failed(format!(
                    "OptiScaler closed plan contains an invalid direct child {}: {error}",
                    candidate.display()
                ))
            })?;
            let key = crate::paths::normalized_key(Path::new(leaf.as_os_str()));
            if reserved_keys.insert(key) {
                reserved.push(leaf);
            }
            Ok(())
        };
        for scheduled in &scheduled_directory_paths {
            reserve(scheduled)?;
        }
        for removal in &removal_paths {
            reserve(removal)?;
        }
        for destination in &destination_paths {
            reserve(destination)?;
        }
        let children = authority
            .enumerate_reserved_children(&reserved)
            .map_err(|error| {
                failed(format!(
                    "failed to enumerate exact OptiScaler postcommit directory {}: {error}",
                    path.display()
                ))
            })?;
        for child in children {
            let child_path = path.join(child.name.as_os_str());
            let child_key = crate::paths::normalized_key(&child_path);
            if scheduled_directories.contains(&child_key) {
                let expected_identity = directories
                    .iter()
                    .find(|candidate| {
                        crate::paths::normalized_key(Path::new(candidate.path.as_str()))
                            == child_key
                    })
                    .map(|candidate| candidate.identity.as_str())
                    .ok_or_else(|| failed("scheduled child disappeared from the closed plan"))?;
                if !child.observation.is_directory()
                    || child.observation.identity != expected_identity
                {
                    return Err(failed(format!(
                        "scheduled OptiScaler postcommit child identity or type changed: {}",
                        child_path.display()
                    )));
                }
                continue;
            }
            if let Some(expected) = removal_receipts.get(&child_key) {
                if child.observation.kind != crate::fs::EntryKind::File
                    || child.observation.identity != expected.identity()
                    || child.observation.digest.as_deref() != Some(expected.digest().as_str())
                {
                    return Err(failed(format!(
                        "scheduled OptiScaler removal child identity, digest, or type changed: {}",
                        child_path.display()
                    )));
                }
                continue;
            }
            if destinations.contains(&child_key) {
                return Err(failed(format!(
                    "OptiScaler postcommit directory contains a surviving mutation destination: {}",
                    child_path.display()
                )));
            }
            return Err(failed(format!(
                "OptiScaler postcommit directory is not empty under the closed uninstall plan: {}",
                child_path.display()
            )));
        }
    }
    Ok(())
}
