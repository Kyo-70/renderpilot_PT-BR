use super::*;

pub(super) fn collect_optiscaler_receipts(
    state: Option<&OptiScalerInstallState>,
    topology: Option<&GameProxyTopology>,
) -> AppResult<BTreeMap<String, (String, FileReceipt)>> {
    let mut receipts = BTreeMap::new();
    if let Some(state) = state {
        for file in &state.release_files {
            insert_receipt(&mut receipts, file.path.as_str(), &file.installed)?;
        }
        for file in &state.runtime_bindings {
            insert_receipt(&mut receipts, file.path.as_str(), &file.installed)?;
        }
    }
    if let Some(topology) = topology {
        insert_receipt(
            &mut receipts,
            topology.outer.path.as_str(),
            &topology.outer.receipt,
        )?;
        if let Some(downstream) = &topology.downstream {
            insert_receipt(&mut receipts, downstream.path.as_str(), &downstream.receipt)?;
        }
    }
    Ok(receipts)
}

pub(super) fn collect_optiscaler_directories(
    state: Option<&OptiScalerInstallState>,
) -> BTreeMap<String, (String, String)> {
    state
        .into_iter()
        .flat_map(|state| state.directory_receipts.iter())
        .map(|directory| {
            (
                normalized_path_key(directory.path.as_str()),
                (
                    directory.path.as_str().to_owned(),
                    directory.identity.clone(),
                ),
            )
        })
        .collect()
}

pub(super) fn insert_receipt(
    receipts: &mut BTreeMap<String, (String, FileReceipt)>,
    path: &str,
    receipt: &FileReceipt,
) -> AppResult<()> {
    let key = normalized_path_key(path);
    if let Some((_, existing)) = receipts.get(&key)
        && existing != receipt
    {
        return Err(renderpilot_application::AppError::invalid_input(format!(
            "OptiScaler aggregate has conflicting receipts for {path}"
        )));
    }
    receipts.insert(key, (path.to_owned(), receipt.clone()));
    Ok(())
}

pub(super) fn insert_path(
    paths: &mut BTreeMap<String, pending_file_mutations::OptiScalerBoundPath>,
    path: &str,
    transition: pending_file_mutations::OptiScalerBoundTransition,
) -> AppResult<()> {
    let key = normalized_path_key(path);
    if let Some(existing) = paths.get(&key)
        && existing.transition != transition
    {
        return Err(renderpilot_application::AppError::invalid_input(format!(
            "OptiScaler aggregate has conflicting transitions for {path}"
        )));
    }
    paths
        .entry(key)
        .or_insert_with(|| pending_file_mutations::OptiScalerBoundPath {
            path: path.to_owned(),
            transition,
        });
    Ok(())
}

pub(super) fn is_owned(receipt: &FileReceipt) -> bool {
    receipt.ownership() == FileOwnership::Owned
}

pub(super) fn is_reused(receipt: &FileReceipt) -> bool {
    receipt.ownership() == FileOwnership::Reused
}

pub(super) fn same_receipt_identity(left: &FileReceipt, right: &FileReceipt) -> bool {
    left.identity() == right.identity()
}

pub(super) fn same_receipt_identity_digest_ownership(
    left: &FileReceipt,
    right: &FileReceipt,
) -> bool {
    same_receipt_identity(left, right)
        && left.digest() == right.digest()
        && left.ownership() == right.ownership()
}

pub(super) fn optiscaler_transition_feature(
    before_state: Option<&OptiScalerInstallState>,
    after_state: Option<&OptiScalerInstallState>,
) -> &'static str {
    use renderpilot_domain::mutation_features::{
        OPTISCALER_INSTALL, OPTISCALER_RELOCATE, OPTISCALER_UNINSTALL, OPTISCALER_UPDATE,
    };
    match (before_state, after_state) {
        (None, Some(_)) => OPTISCALER_INSTALL,
        (Some(_), None) => OPTISCALER_UNINSTALL,
        (Some(before), Some(after))
            if before.target_exe_path != after.target_exe_path
                || before.target_dir != after.target_dir =>
        {
            OPTISCALER_RELOCATE
        }
        (Some(_), Some(_)) | (None, None) => OPTISCALER_UPDATE,
    }
}

pub(super) fn stable_topology_subject<'a>(
    before_state: Option<&'a OptiScalerInstallState>,
    after_state: Option<&'a OptiScalerInstallState>,
    before_topology: Option<&'a GameProxyTopology>,
    after_topology: Option<&'a GameProxyTopology>,
) -> AppResult<&'a str> {
    let before_state_subject = before_state.and_then(|state| state.proxy_topology_id.as_deref());
    let after_state_subject = after_state.and_then(|state| state.proxy_topology_id.as_deref());
    if before_state_subject.is_some()
        && after_state_subject.is_some()
        && before_state_subject != after_state_subject
    {
        return Err(renderpilot_application::AppError::invalid_input(
            "OptiScaler topology subject cannot change during a filesystem transition",
        ));
    }

    let before_topology_subject = before_topology.map(|topology| topology.id.as_str());
    let after_topology_subject = after_topology.map(|topology| topology.id.as_str());
    if before_topology_subject.is_some()
        && after_topology_subject.is_some()
        && before_topology_subject != after_topology_subject
    {
        return Err(renderpilot_application::AppError::invalid_input(
            "proxy topology subject cannot change during a filesystem transition",
        ));
    }

    let state_subject = after_state_subject.or(before_state_subject);
    let topology_subject = after_topology_subject.or(before_topology_subject);
    match (state_subject, topology_subject) {
        (Some(state), Some(topology)) if state == topology => Ok(state),
        (Some(_), Some(_)) => Err(renderpilot_application::AppError::invalid_input(
            "OptiScaler state and proxy topology subjects do not match",
        )),
        (Some(subject), None) | (None, Some(subject)) => Ok(subject),
        (None, None) => Err(renderpilot_application::AppError::invalid_input(
            "OptiScaler filesystem transition requires a topology subject",
        )),
    }
}

pub(super) fn optiscaler_transition_paths(
    before_state: Option<&OptiScalerInstallState>,
    after_state: Option<&OptiScalerInstallState>,
    before_topology: Option<&GameProxyTopology>,
    after_topology: Option<&GameProxyTopology>,
    peer: OptiScalerPeerMutation<'_>,
) -> Vec<String> {
    let mut paths = Vec::new();
    paths.extend(changed_release_paths(before_state, after_state));
    paths.extend(changed_runtime_paths(before_state, after_state));
    paths.extend(changed_directory_paths(before_state, after_state));
    paths.extend(changed_topology_paths(before_topology, after_topology));
    match peer {
        OptiScalerPeerMutation::Replace { before, after } => {
            paths.extend(changed_peer_paths(Some(before), Some(after)));
        }
        OptiScalerPeerMutation::Keep => {}
    }
    paths
}

pub(super) fn changed_release_paths(
    before: Option<&OptiScalerInstallState>,
    after: Option<&OptiScalerInstallState>,
) -> Vec<String> {
    changed_map_paths(&release_path_map(before), &release_path_map(after))
}

pub(super) fn changed_runtime_paths(
    before: Option<&OptiScalerInstallState>,
    after: Option<&OptiScalerInstallState>,
) -> Vec<String> {
    changed_map_paths(&runtime_path_map(before), &runtime_path_map(after))
}

pub(super) fn changed_directory_paths(
    before: Option<&OptiScalerInstallState>,
    after: Option<&OptiScalerInstallState>,
) -> Vec<String> {
    changed_map_paths(&directory_path_map(before), &directory_path_map(after))
}

pub(super) fn release_path_map(
    state: Option<&OptiScalerInstallState>,
) -> BTreeMap<
    String,
    (
        String,
        (Sha256Hash, OptiScalerFileRole, OptiScalerFileCleanup),
    ),
> {
    state
        .into_iter()
        .flat_map(|state| state.release_files.iter())
        .map(|receipt| {
            (
                normalized_path_key(receipt.path.as_str()),
                (
                    receipt.path.as_str().to_owned(),
                    (
                        receipt.installed.digest().clone(),
                        receipt.role,
                        receipt.cleanup,
                    ),
                ),
            )
        })
        .collect()
}

type RuntimePathState = (String, FileReceipt, OptiScalerFileBaseline);

pub(super) fn runtime_path_map(
    state: Option<&OptiScalerInstallState>,
) -> BTreeMap<String, (String, RuntimePathState)> {
    state
        .into_iter()
        .flat_map(|state| state.runtime_bindings.iter())
        .map(|binding| {
            (
                normalized_path_key(binding.path.as_str()),
                (
                    binding.path.as_str().to_owned(),
                    (
                        binding.module.clone(),
                        binding.installed.clone(),
                        binding.baseline.clone(),
                    ),
                ),
            )
        })
        .collect()
}

pub(super) fn directory_path_map(
    state: Option<&OptiScalerInstallState>,
) -> BTreeMap<String, (String, String)> {
    state
        .into_iter()
        .flat_map(|state| state.directory_receipts.iter())
        .map(|receipt| {
            (
                normalized_path_key(receipt.path.as_str()),
                (receipt.path.as_str().to_owned(), receipt.identity.clone()),
            )
        })
        .collect()
}

pub(super) fn changed_topology_paths(
    before: Option<&GameProxyTopology>,
    after: Option<&GameProxyTopology>,
) -> Vec<String> {
    changed_map_paths(&topology_path_map(before), &topology_path_map(after))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum TopologyPathState {
    Root {
        implementation: ProxyImplementation,
        sha256: Sha256Hash,
        root_prestate: ProxyRootPrestate,
    },
    Downstream {
        implementation: ProxyImplementation,
        sha256: Sha256Hash,
    },
    Origin,
}

pub(super) fn topology_path_map(
    topology: Option<&GameProxyTopology>,
) -> BTreeMap<String, (String, TopologyPathState)> {
    let Some(topology) = topology else {
        return BTreeMap::new();
    };
    let mut paths = BTreeMap::new();
    paths.insert(
        normalized_path_key(topology.root_slot.as_str()),
        (
            topology.root_slot.as_str().to_owned(),
            TopologyPathState::Root {
                implementation: topology.outer.implementation,
                sha256: topology.outer.receipt.digest().clone(),
                root_prestate: topology.root_prestate,
            },
        ),
    );
    if let Some(downstream) = &topology.downstream {
        paths.insert(
            normalized_path_key(downstream.path.as_str()),
            (
                downstream.path.as_str().to_owned(),
                TopologyPathState::Downstream {
                    implementation: downstream.implementation,
                    sha256: downstream.receipt.digest().clone(),
                },
            ),
        );
    }
    if let Some(origin) = &topology.downstream_origin
        && normalized_path_key(origin.as_str()) != normalized_path_key(topology.root_slot.as_str())
    {
        paths.insert(
            normalized_path_key(origin.as_str()),
            (origin.as_str().to_owned(), TopologyPathState::Origin),
        );
    }
    paths
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum PeerPathState {
    Created,
    BackedUp,
    Managed {
        mode: ManagedFileMode,
        baseline: ManagedFileBaseline,
        installed_sha256: Sha256Hash,
    },
    RegisteredExecutable,
}

pub(super) fn peer_path_map(
    addon: Option<&InstalledAddon>,
) -> BTreeMap<String, (String, PeerPathState)> {
    let Some(addon) = addon else {
        return BTreeMap::new();
    };
    let mut map = BTreeMap::new();
    insert_peer_paths(&mut map, addon);
    map
}

fn insert_peer_paths(
    paths: &mut BTreeMap<String, (String, PeerPathState)>,
    addon: &InstalledAddon,
) {
    for path in addon.created_files() {
        paths
            .entry(normalized_path_key(path.as_str()))
            .or_insert_with(|| (path.as_str().to_owned(), PeerPathState::Created));
    }
    for path in addon.backed_up_files() {
        paths
            .entry(normalized_path_key(path.as_str()))
            .or_insert_with(|| (path.as_str().to_owned(), PeerPathState::BackedUp));
    }
    for file in addon.managed_files() {
        paths.insert(
            normalized_path_key(file.path().as_str()),
            (
                file.path().as_str().to_owned(),
                PeerPathState::Managed {
                    mode: file.mode(),
                    baseline: file.baseline().clone(),
                    installed_sha256: file.installed_sha256().clone(),
                },
            ),
        );
    }
    if let Some(path) = addon.registered_exe_path() {
        paths
            .entry(normalized_path_key(path.as_str()))
            .or_insert_with(|| {
                (
                    path.as_str().to_owned(),
                    PeerPathState::RegisteredExecutable,
                )
            });
    }
}

pub(super) fn changed_peer_paths(
    before: Option<&InstalledAddon>,
    after: Option<&InstalledAddon>,
) -> Vec<String> {
    let before_map = peer_path_map(before);
    let after_map = peer_path_map(after);
    let mut paths = changed_map_paths(&before_map, &after_map);
    paths.extend(changed_map_paths(
        &peer_owned_sidecar_path_map(before),
        &peer_owned_sidecar_path_map(after),
    ));
    paths
}

/// Owned managed baselines are physical sidecars whose custody follows the
/// managed host path. Reused baselines remain foreign and must never enter an
/// OptiScaler filesystem transition.
pub(super) fn peer_owned_sidecar_path_map(
    addon: Option<&InstalledAddon>,
) -> BTreeMap<String, (String, Sha256Hash)> {
    addon
        .into_iter()
        .flat_map(InstalledAddon::managed_files)
        .filter_map(|file| match (file.mode(), file.baseline()) {
            (ManagedFileMode::Owned, ManagedFileBaseline::Present { sha256 }) => {
                let path = format!("{}.bak", file.path().as_str());
                Some((normalized_path_key(&path), (path, sha256.clone())))
            }
            _ => None,
        })
        .collect()
}

pub(super) fn changed_map_paths<T: PartialEq>(
    before: &BTreeMap<String, (String, T)>,
    after: &BTreeMap<String, (String, T)>,
) -> Vec<String> {
    let keys = before
        .keys()
        .chain(after.keys())
        .map(std::string::String::as_str)
        .collect::<std::collections::BTreeSet<_>>();
    keys.into_iter()
        .filter_map(|key| {
            let before_entry = before.get(key);
            let after_entry = after.get(key);
            if before_entry.map(|(_, value)| value) == after_entry.map(|(_, value)| value) {
                return None;
            }
            before_entry.or(after_entry).map(|(path, _)| path.clone())
        })
        .collect()
}
