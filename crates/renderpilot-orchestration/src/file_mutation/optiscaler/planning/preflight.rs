fn prepare_optiscaler<'a>(
    mutation: &OptiScalerMutation<'a>,
) -> Result<PreparedFileMutation<'a>, ServiceError> {
    preflight_threat_model(mutation.threat_model)?;
    super::recover_pending(mutation.context, mutation.guard)?;
    if mutation.feature.trim().is_empty() {
        return Err(crate::failed("OptiScaler feature must not be empty"));
    }
    let id = ulid::Ulid::generate().to_string();
    let mutation_id_anchor = mutation.context.file_mutation_root().join(&id);
    let mut journal = build_journal(
        mutation.scope,
        &mutation_id_anchor,
        &mutation.operations,
        mutation.threat_model,
    )?;
    validate_private_workspace_paths(&id, &journal)?;
    let initial_json = serde_json::to_string(&journal)
        .map_err(|error| crate::failed(format!("failed to serialize initial journal: {error}")))?;
    let executor = mutation.context.peer_mutation_executor();
    let preparing =
        executor.begin_optiscaler_journal_aggregate(OptiScalerJournalAggregateBegin::new(
            id.clone(),
            mutation.guard.game_id().clone(),
            mutation.feature.to_owned(),
            mutation.subject_id.map(str::to_owned),
            initial_json,
        )?)?;
    let preparing = materialize_namespaces(executor, preparing, &mut journal, &mutation_id_anchor)?;
    let prepared_json = serde_json::to_string(&journal)
        .map_err(|error| crate::failed(format!("failed to serialize prepared journal: {error}")))?;
    Ok(PreparedFileMutation {
        id,
        game_id: mutation.guard.game_id().clone(),
        executor,
        authority: JournalAuthority::ActivePreparing(Box::new(preparing)),
        transaction_root: mutation.context.file_mutation_root().to_path_buf(),
        journal,
        journal_json: prepared_json,
        next_operation_id: 0,
        managed_endpoint_roots: managed_endpoint_roots(mutation)?,
    })
}

fn managed_endpoint_roots(
    mutation: &OptiScalerMutation<'_>,
) -> Result<HashMap<String, PathBuf>, ServiceError> {
    let mut values = HashMap::new();
    for (root, endpoint) in &mutation.managed_endpoint_roots {
        if !crate::paths::is_within(endpoint, root) || crate::paths::same_path(endpoint, root) {
            return Err(crate::failed(format!(
                "managed OptiScaler endpoint escapes its declared root: {}",
                endpoint.display()
            )));
        }
        let key = crate::paths::normalized_key(endpoint);
        match values.entry(key) {
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(root.clone());
            }
            std::collections::hash_map::Entry::Occupied(entry)
                if crate::paths::same_path(entry.get(), root) => {}
            std::collections::hash_map::Entry::Occupied(_) => {
                return Err(crate::failed(
                    "managed OptiScaler endpoint has conflicting retained roots",
                ));
            }
        }
    }
    Ok(values)
}

pub(crate) fn preflight_threat_model(threat_model: ThreatModel) -> Result<(), ServiceError> {
    let mode = match threat_model {
        ThreatModel::CooperativeSameUid => crate::fs::AuthorityMode::CooperativeSameUid,
        ThreatModel::HostileSameUid => crate::fs::AuthorityMode::HostileSameUid,
    };
    mode.preflight()?;
    #[cfg(all(
        not(windows),
        not(target_os = "linux"),
        not(feature = "development-host-fallback")
    ))]
    return Err(crate::failed(
        "OptiScaler has no native authority adapter for this host",
    ));
    Ok(())
}
