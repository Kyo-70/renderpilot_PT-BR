#[derive(Debug)]
struct OpData {
    id: u32,
    workspace_id: Option<u32>,
    slots: PrivateArtifactSlots,
    effect: DomainOperationEffect,
}

fn build_journal(
    scope: &MutationScope,
    mutation_id_anchor: &Path,
    planned: &[OptiScalerPlannedOperation],
    threat_model: ThreatModel,
) -> Result<OptiScalerJournal, ServiceError> {
    let transaction_id = mutation_id_anchor
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| crate::failed("transaction id is not valid UTF-8"))?;
    let capability = NamespaceCapability::new(private_workspace_capability()?)
        .map_err(|error| crate::failed(error.to_string()))?;
    let control_dir = mutation_id_anchor
        .parent()
        .ok_or_else(|| crate::failed("mutation id anchor has no parent"))?
        .join(format!("control-{transaction_id}-{capability}"));
    let control_namespace = ControlNamespaceBinding::new(
        control_dir.to_string_lossy().into_owned(),
        None,
        capability.clone(),
    )
    .map_err(|error| crate::failed(error.to_string()))?;
    let (planned_workspaces, workspace_ids) =
        namespace::planned_workspaces(transaction_id, capability.as_str(), planned, &scope.roots)?;
    let private_workspaces = planned_workspaces
        .iter()
        .map(|workspace| {
            PrivateWorkspaceBinding::new(
                workspace.workspace_id(),
                workspace.root_index(),
                workspace.path().to_string_lossy().into_owned(),
                None,
                capability.clone(),
            )
            .map_err(|error| crate::failed(error.to_string()))
        })
        .collect::<Result<Vec<_>, _>>()?;

    let mut op_data: Vec<OpData> = Vec::new();
    let mut last_touch: HashMap<String, (u32, DomainEndpoint)> = HashMap::new();
    let mut last_ownership: HashMap<String, FileOwnership> = HashMap::new();
    let mut last_absent = HashSet::new();
    let mut absent_dirs = HashSet::new();

    for (index, item) in planned.iter().enumerate() {
        let id = u32::try_from(index).map_err(|_| crate::failed("operation index overflow"))?;

        // Precompute normalized keys for the relocation case upfront to avoid
        // repeated calls inside build_relocate_effect.
        let relocate_keys: Option<(String, String)> = match item {
            OptiScalerPlannedOperation::Relocate { source, destination } => Some((
                crate::paths::normalized_key(&source.path),
                crate::paths::normalized_key(&destination.path),
            )),
            _ => None,
        };

        let (mut effect, paths) = match item {
            OptiScalerPlannedOperation::Write(value) => (
                build_single_effect(
                    scope,
                    value,
                    OptiScalerAction::Write,
                    &absent_dirs,
                    last_touch.contains_key(&crate::paths::normalized_key(&value.path)),
                )?,
                vec![(value.path.clone(), DomainEndpoint::Single)],
            ),
            OptiScalerPlannedOperation::Delete(value) => (
                build_single_effect(
                    scope,
                    value,
                    OptiScalerAction::Delete,
                    &absent_dirs,
                    last_touch.contains_key(&crate::paths::normalized_key(&value.path)),
                )?,
                vec![(value.path.clone(), DomainEndpoint::Single)],
            ),
            OptiScalerPlannedOperation::Verify(value) => (
                build_single_effect(
                    scope,
                    value,
                    OptiScalerAction::Verify,
                    &absent_dirs,
                    last_touch.contains_key(&crate::paths::normalized_key(&value.path)),
                )?,
                vec![(value.path.clone(), DomainEndpoint::Single)],
            ),
            OptiScalerPlannedOperation::CreateDirectory(value) => {
                let effect = build_single_effect(
                    scope,
                    value,
                    OptiScalerAction::CreateDirectory,
                    &absent_dirs,
                    last_touch.contains_key(&crate::paths::normalized_key(&value.path)),
                )?;
                absent_dirs.insert(crate::paths::normalized_key(&value.path));
                (effect, vec![(value.path.clone(), DomainEndpoint::Single)])
            }
            OptiScalerPlannedOperation::Relocate { source, destination } => {
                let (src_key, dst_key) = relocate_keys.as_ref().unwrap();
                (
                    build_relocate_effect(
                        scope,
                        source,
                        destination,
                        &absent_dirs,
                        last_touch.contains_key(src_key),
                        last_touch.contains_key(dst_key),
                    )?,
                    vec![
                        (source.path.clone(), DomainEndpoint::Source),
                        (destination.path.clone(), DomainEndpoint::Destination),
                    ],
                )
            }
            OptiScalerPlannedOperation::PostCommitRemoveDirectory(receipt) => {
                let path = PathBuf::from(receipt.path.as_str());
                super::scope::require_path_in_scope(&path, scope)?;
                let endpoint = DomainOperationEndpoint::new(
                    DomainEndpoint::Single,
                    path.to_string_lossy().into_owned(),
                    DomainPreimage::Initial {
                        observation: DurableObservation::Directory {
                            identity: receipt.identity.clone(),
                        },
                        receipt: None,
                        owned_basis: None,
                    },
                    JournalAfter::Pending,
                )
                .map_err(|error| crate::failed(error.to_string()))?;
                (
                    DomainOperationEffect::PostCommitRemoveDirectory(
                        DomainRemoveDirectoryEffect::new(
                            endpoint,
                            DomainRemoveDirectoryState::Planned {
                                directory: DurableObservation::Directory {
                                    identity: receipt.identity.clone(),
                                },
                            },
                        )
                        .map_err(|error| crate::failed(error.to_string()))?,
                    ),
                    vec![(path, DomainEndpoint::Single)],
                )
            }
        };

        if matches!(
            item,
            OptiScalerPlannedOperation::Write(_) | OptiScalerPlannedOperation::Delete(_)
        ) {
            for (path, _) in &paths {
                let key = crate::paths::normalized_key(path);
                let prior_absent_write = matches!(item, OptiScalerPlannedOperation::Write(_))
                    && last_absent.contains(&key);
                if last_touch.contains_key(&key)
                    && last_ownership.get(&key) != Some(&FileOwnership::Owned)
                    && !prior_absent_write
                {
                    return Err(crate::failed(
                        "destructive operation requires an Owned producer",
                    ));
                }
            }
        }

        for (path, role) in &paths {
            let key = crate::paths::normalized_key(path);
            if let Some((prior, prior_role)) = last_touch.get(&key).copied() {
                set_prior_postimage(&mut effect, path, *role, prior, prior_role)?;
            }
        }

        let artifacts = PrivateArtifactSlots::new(
            DurableObservation::Absent,
            DurableObservation::Absent,
            DurableObservation::Absent,
        )
        .map_err(|error| crate::failed(error.to_string()))?;
        let workspace_id = workspace_ids
            .get(index)
            .copied()
            .ok_or_else(|| crate::failed("workspace program is shorter than planned operations"))?;

        let produced_ownership = match item {
            OptiScalerPlannedOperation::Write(_) => vec![Some(FileOwnership::Owned)],
            OptiScalerPlannedOperation::Delete(_)
            | OptiScalerPlannedOperation::CreateDirectory(_)
            | OptiScalerPlannedOperation::PostCommitRemoveDirectory(_) => vec![None],
            OptiScalerPlannedOperation::Verify(_) => paths
                .iter()
                .map(|(path, _)| planned_input_ownership(path, item, &last_touch, &last_ownership))
                .collect(),
            OptiScalerPlannedOperation::Relocate { source, .. } => {
                let ownership =
                    planned_input_ownership(&source.path, item, &last_touch, &last_ownership)
                        .ok_or_else(|| {
                            crate::failed("relocation source has no known producer ownership")
                        })?;
                vec![None, Some(ownership)]
            }
        };

        for ((path, role), ownership) in paths.into_iter().zip(produced_ownership) {
            let key = crate::paths::normalized_key(&path);
            last_touch.insert(key.clone(), (id, role));
            if let Some(ownership) = ownership {
                last_absent.remove(&key);
                last_ownership.insert(key, ownership);
            } else {
                last_ownership.remove(&key);
                if matches!(
                    item,
                    OptiScalerPlannedOperation::Delete(_)
                        | OptiScalerPlannedOperation::Relocate { .. }
                        | OptiScalerPlannedOperation::PostCommitRemoveDirectory(_)
                ) {
                    last_absent.insert(key);
                }
            }
        }

        op_data.push(OpData {
            id,
            workspace_id,
            slots: artifacts,
            effect,
        });
    }

    // Compute the list of dependencies using references only. This allows us
    // to later consume op_data and move the effects/slots instead of cloning.
    let deps_list: Vec<Vec<u32>> = (0..op_data.len())
        .map(|index| {
            (0..index)
                .filter_map(|candidate| {
                    let is_parent =
                        action(&op_data[candidate].effect) == OptiScalerAction::CreateDirectory;
                    let covers = op_data[candidate]
                        .effect
                        .endpoints()
                        .into_iter()
                        .any(|parent| {
                            op_data[index]
                                .effect
                                .endpoints()
                                .into_iter()
                                .any(|endpoint| {
                                    Path::new(endpoint.path()).parent().is_some_and(|value| {
                                        crate::paths::same_path(value, Path::new(parent.path()))
                                    })
                                })
                        });
                    (is_parent && covers).then(|| u32::try_from(candidate).ok()).flatten()
                })
                .collect()
        })
        .collect();

    // Consume op_data and move the effects and slots directly into the final
    // records. No cloning of the operation payloads is performed.
    let with_deps: Vec<_> = op_data
        .into_iter()
        .zip(deps_list)
        .map(|(data, deps)| {
            DomainOperationRecord::new(
                data.id,
                deps,
                data.workspace_id,
                data.slots,
                data.effect,
            )
            .map_err(|error| crate::failed(error.to_string()))
        })
        .collect::<Result<_, _>>()?;
    OptiScalerJournal::new_with_threat_model(
        threat_model,
        roots(&scope.roots),
        control_namespace,
        private_workspaces,
        with_deps,
    )
    .map_err(|error| crate::failed(error.to_string()))
}
