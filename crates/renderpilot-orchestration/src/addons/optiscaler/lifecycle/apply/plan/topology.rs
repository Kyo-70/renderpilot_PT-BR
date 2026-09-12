use super::*;

pub(super) struct TopologyPlan {
    pub(super) existing_topology: Option<GameProxyTopology>,
    pub(super) peer_host_transition: Option<adoption::PeerHostTransitionPlan>,
    pub(super) removed_old_paths: Vec<PathBuf>,
    pub(super) old_proxy_path: PathBuf,
    pub(super) old_expected: HashMap<String, Sha256Hash>,
    pub(super) other_claim_paths: HashSet<String>,
    pub(super) retained_claims: Vec<renderpilot_storage_sqlite::OptiScalerRetainedClaim>,
}

pub(super) fn plan_topology(
    prepared: &ApplyPlan<'_>,
    targets: &super::targets::TargetPlan<'_>,
) -> Result<TopologyPlan, ServiceError> {
    let ApplyPlan {
        context,
        manifest,
        game_id,
        old_state,
        old_managed_files,
        target,
        ..
    } = prepared;
    let other_claims = other_managed_claims(context, game_id)?;
    let other_claim_paths = other_claims
        .iter()
        .map(|claim| crate::paths::normalized_key(Path::new(claim.path.as_str())))
        .collect::<HashSet<_>>();
    let existing_topology = context.storage().get_proxy_topology(game_id)?;
    let peer_host_transition = plan_apply_peer_transition(
        context,
        game_id,
        old_state.as_ref(),
        existing_topology.as_ref(),
        &target.proxy,
    )?;
    let mut old_paths: HashSet<_> = old_state
        .as_ref()
        .map(|state| {
            state
                .release_files
                .iter()
                .map(|receipt| PathBuf::from(receipt.path.as_str()))
                .chain(
                    old_managed_files
                        .iter()
                        .map(|managed| PathBuf::from(managed.path().as_str())),
                )
                .collect()
        })
        .unwrap_or_default();
    if let Some(topology) = &existing_topology {
        old_paths.extend(
            topology
                .participant_paths()
                .map(|path| PathBuf::from(path.as_str())),
        );
    } else {
        old_paths.insert(targets.proxy_path.clone());
    }
    let mut all_targets = targets.new_paths.clone();
    for (index, target) in targets.native_targets.iter().enumerate() {
        if targets
            .native_copy_paths
            .contains(&crate::paths::normalized_key(&target.destination))
        {
            all_targets.insert(
                format!("native:{}:{index}", target.module_id),
                target.destination.clone(),
            );
        }
    }
    for (artifact, destination) in &targets.artifact_targets {
        if targets
            .artifact_write_paths
            .contains(&crate::paths::normalized_key(destination))
        {
            all_targets.insert(
                format!("module-artifact:{}", artifact.module_id),
                destination.clone(),
            );
        }
    }
    let desired_paths = targets
        .new_paths
        .values()
        .cloned()
        .chain(
            targets
                .native_targets
                .iter()
                .map(|target| target.destination.clone()),
        )
        .chain(
            targets
                .artifact_targets
                .iter()
                .map(|(_, path)| path.clone()),
        )
        .chain(target.proxy.downstream_path.iter().cloned())
        .collect::<Vec<_>>();
    let removed_old_paths = old_paths
        .iter()
        .filter(|old| {
            !desired_paths
                .iter()
                .any(|desired| crate::paths::same_path(old, desired))
        })
        .cloned()
        .collect::<Vec<_>>();
    let retained_claims = other_claims
        .into_iter()
        .filter(|claim| {
            removed_old_paths
                .iter()
                .any(|path| crate::paths::same_path(path, Path::new(claim.path.as_str())))
        })
        .collect::<Vec<_>>();
    let known_downstream = existing_topology.as_ref().and_then(|topology| {
        topology
            .downstream
            .as_ref()
            .map(|link| PathBuf::from(link.path.as_str()))
    });
    preflight_targets(
        &all_targets,
        &old_paths,
        &target.proxy,
        known_downstream.as_deref(),
        &targets
            .retained_fsr
            .iter()
            .map(|plan| crate::paths::normalized_key(&plan.target))
            .collect(),
    )?;
    let old_release = old_state.as_ref().and_then(|state| {
        manifest
            .releases
            .iter()
            .find(|release| release.id == state.release_id)
    });
    let old_proxy_path = existing_topology
        .as_ref()
        .map(|topology| PathBuf::from(topology.root_slot.as_str()))
        .unwrap_or_else(|| targets.proxy_path.clone());
    let mut old_expected = match (old_release, old_state.as_ref()) {
        (Some(release), Some(state)) => {
            expected_target_hashes(release, &state.modules, state, &old_proxy_path)?
        }
        _ => HashMap::new(),
    };
    for managed in old_managed_files {
        old_expected.insert(
            crate::paths::normalized_key(Path::new(managed.path().as_str())),
            managed.installed_sha256().clone(),
        );
    }
    if let Some(topology) = &existing_topology {
        old_expected.insert(
            crate::paths::normalized_key(Path::new(topology.outer.path.as_str())),
            topology.outer.receipt.digest().clone(),
        );
        if let Some(downstream) = &topology.downstream {
            old_expected.insert(
                crate::paths::normalized_key(Path::new(downstream.path.as_str())),
                downstream.receipt.digest().clone(),
            );
        }
    }
    if let (Some(source), Some(sha256)) = (
        target.proxy.reshade_source_path.as_ref(),
        target.proxy.reshade_source_sha256.as_ref(),
    ) {
        old_expected.insert(crate::paths::normalized_key(source), sha256.clone());
    }
    Ok(TopologyPlan {
        existing_topology,
        peer_host_transition,
        removed_old_paths,
        old_proxy_path,
        old_expected,
        other_claim_paths,
        retained_claims,
    })
}
