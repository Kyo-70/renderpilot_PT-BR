fn validate_namespace_bindings(journal: &OptiScalerJournal) -> Result<(), OptiScalerJournalError> {
    let control = journal.control_namespace();
    validate_control_leaf(control.path(), control.capability())?;

    let mut native_identities = HashSet::new();
    if let Some(identity) = control.identity()
        && !native_identities.insert(identity)
    {
        return Err(OptiScalerJournalError::Invalid(
            "namespace identities must be unique",
        ));
    }

    for (index, workspace) in journal.private_workspaces().iter().enumerate() {
        let expected_id = u32::try_from(index)
            .map_err(|_| OptiScalerJournalError::Invalid("workspace program is too large"))?;
        if workspace.workspace_id() != expected_id {
            return Err(OptiScalerJournalError::Invalid(
                "workspace ids must be contiguous and ordered",
            ));
        }
        let root_index = usize::try_from(workspace.root_index())
            .map_err(|_| OptiScalerJournalError::Invalid("workspace root index overflows usize"))?;
        if journal.roots().get(root_index).is_none() {
            return Err(OptiScalerJournalError::Invalid(
                "workspace root index is outside the declared roots",
            ));
        }
        if workspace.capability() != control.capability() {
            return Err(OptiScalerJournalError::Invalid(
                "workspace capability is not bound to the control namespace",
            ));
        }
        validate_workspace_leaf(
            workspace.path(),
            workspace.workspace_id(),
            workspace.capability(),
        )?;
        if normalized_path_relation(control.path(), workspace.path())
            != NormalizedPathRelation::Disjoint
        {
            return Err(OptiScalerJournalError::Invalid(
                "control namespace overlaps a private workspace",
            ));
        }
        if let Some(identity) = workspace.identity()
            && !native_identities.insert(identity)
        {
            return Err(OptiScalerJournalError::Invalid(
                "namespace identities must be unique",
            ));
        }
        for previous in journal.private_workspaces().iter().take(index) {
            if normalized_path_relation(previous.path(), workspace.path())
                != NormalizedPathRelation::Disjoint
            {
                return Err(OptiScalerJournalError::Invalid(
                    "private workspace paths must be lexically disjoint",
                ));
            }
        }
    }

    let mut referenced = vec![false; journal.private_workspaces().len()];
    for operation in journal.operations() {
        if let Some(workspace_id) = operation.workspace_id() {
            let index = usize::try_from(workspace_id).map_err(|_| {
                OptiScalerJournalError::Invalid("operation workspace id overflows usize")
            })?;
            journal
                .private_workspaces()
                .get(index)
                .ok_or(OptiScalerJournalError::Invalid(
                    "operation references a missing private workspace",
                ))?;
            referenced[index] = true;
        }
    }
    if referenced.iter().any(|referenced| !referenced) {
        return Err(OptiScalerJournalError::Invalid(
            "every private workspace must be referenced",
        ));
    }
    for operation in journal.operations() {
        for endpoint in operation.effect().endpoints() {
            if normalized_path_relation(control.path(), endpoint.path())
                != NormalizedPathRelation::Disjoint
            {
                return Err(OptiScalerJournalError::Invalid(
                    "control namespace overlaps an operation endpoint",
                ));
            }
        }
    }
    for workspace in journal.private_workspaces() {
        for operation in journal.operations() {
            for endpoint in operation.effect().endpoints() {
                if normalized_path_relation(workspace.path(), endpoint.path())
                    != NormalizedPathRelation::Disjoint
                {
                    return Err(OptiScalerJournalError::Invalid(
                        "private workspace overlaps an operation endpoint",
                    ));
                }
            }
        }
    }
    Ok(())
}

fn validate_control_leaf(
    path: &str,
    capability: &NamespaceCapability,
) -> Result<(), OptiScalerJournalError> {
    validate_capability_bound_leaf(path, capability)
}

fn validate_workspace_leaf(
    path: &str,
    workspace_id: u32,
    capability: &NamespaceCapability,
) -> Result<(), OptiScalerJournalError> {
    validate_capability_bound_leaf(path, capability)?;
    let marker = format!("-{workspace_id}-");
    if !path.rsplit('/').next().unwrap_or(path).contains(&marker) {
        return Err(OptiScalerJournalError::Invalid(
            "private workspace leaf is not bound to its workspace id",
        ));
    }
    Ok(())
}

fn validate_capability_bound_leaf(
    path: &str,
    capability: &NamespaceCapability,
) -> Result<(), OptiScalerJournalError> {
    let leaf = path.rsplit('/').next().unwrap_or(path);
    let capability_suffix = format!("-{}", capability.as_str());
    if !leaf.ends_with(&capability_suffix) {
        return Err(OptiScalerJournalError::Invalid(
            "namespace leaf is not bound to its capability",
        ));
    }
    Ok(())
}
