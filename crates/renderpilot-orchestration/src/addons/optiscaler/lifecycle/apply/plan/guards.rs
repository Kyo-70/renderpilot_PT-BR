use super::*;

pub(super) fn plan_rewrite_guards(
    targets: &super::targets::TargetPlan<'_>,
    topology: &super::topology::TopologyPlan,
    config: &ConfigInputs,
    existing_topology: Option<&GameProxyTopology>,
) -> Result<Vec<MutationTarget>, ServiceError> {
    let rewritten_paths = targets
        .new_paths
        .values()
        .filter(|path| {
            targets
                .release_write_paths
                .contains(&crate::paths::normalized_key(path))
        })
        .cloned()
        .chain(
            targets
                .native_targets
                .iter()
                .filter(|target| {
                    targets
                        .native_copy_paths
                        .contains(&crate::paths::normalized_key(&target.destination))
                })
                .map(|target| target.destination.clone()),
        )
        .chain(
            targets
                .artifact_targets
                .iter()
                .filter(|(_, path)| {
                    targets
                        .artifact_write_paths
                        .contains(&crate::paths::normalized_key(path))
                })
                .map(|(_, path)| path.clone()),
        );
    let mut rewrite_guards = Vec::new();
    for path in rewritten_paths {
        let expected = if config.target_mode == ConfigTargetMode::InPlace
            && crate::paths::same_path(&path, &config.destination_path)
        {
            config.current_sha256.clone()
        } else {
            topology
                .old_expected
                .get(&crate::paths::normalized_key(&path))
                .cloned()
        };
        rewrite_guards.push(match expected {
            Some(expected) => MutationTarget::quarantine(&path, Some(expected)),
            None => MutationTarget::absent_file(&path),
        });
    }
    for target in &targets.native_targets {
        let key = crate::paths::normalized_key(&target.destination);
        if !targets.native_copy_paths.contains(&key) {
            let expected = targets
                .native_expected_sha256
                .get(&key)
                .ok_or_else(|| {
                    failed(format!(
                        "native target hash is missing for {}",
                        target.destination.display()
                    ))
                })?
                .clone();
            rewrite_guards.push(MutationTarget::quarantine(
                &target.destination,
                Some(expected),
            ));
        }
    }
    for (artifact, destination) in &targets.artifact_targets {
        if !targets
            .artifact_write_paths
            .contains(&crate::paths::normalized_key(destination))
        {
            rewrite_guards.push(MutationTarget::quarantine(
                destination,
                Some(artifact.sha256.clone()),
            ));
        }
    }
    // Relocation and chain maintenance may delete or copy these paths
    // before the release writer runs. Bind them to the committed topology,
    // too, so the custody journal cannot adopt drift as its preimage.
    if let Some(topology) = existing_topology {
        rewrite_guards.push(MutationTarget::quarantine(
            Path::new(topology.outer.path.as_str()),
            Some(topology.outer.receipt.digest().clone()),
        ));
        if let Some(downstream) = &topology.downstream {
            rewrite_guards.push(MutationTarget::quarantine(
                Path::new(downstream.path.as_str()),
                Some(downstream.receipt.digest().clone()),
            ));
        }
    }
    Ok(rewrite_guards)
}
