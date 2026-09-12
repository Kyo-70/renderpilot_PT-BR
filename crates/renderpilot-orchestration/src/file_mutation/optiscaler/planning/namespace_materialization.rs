/// Materializes the control namespace and the ordered workspace frontier.
///
/// Once the aggregate exists every error deliberately leaves its durable row
/// and reservation in storage. Recovery resumes this exact state machine;
/// callers must never discard a partially materialized program.
fn materialize_namespaces(
    executor: &PeerMutationExecutor,
    mut preparing: PreparingOptiScalerJournalAggregate,
    journal: &mut OptiScalerJournal,
    mutation_id_anchor: &Path,
) -> Result<PreparingOptiScalerJournalAggregate, ServiceError> {
    let cas = |executor: &PeerMutationExecutor,
               preparing: PreparingOptiScalerJournalAggregate,
               journal: &OptiScalerJournal|
     -> Result<PreparingOptiScalerJournalAggregate, ServiceError> {
        let json = serde_json::to_string(journal).map_err(|error| {
            crate::failed(format!("failed to serialize journal transition: {error}"))
        })?;
        executor
            .cas_preparing_optiscaler_journal_aggregate(preparing, json)
            .map_err(|error| {
                crate::failed(format!("OptiScaler materialization CAS failed: {error}"))
            })
    };

    journal.set_materialization(DomainMaterializationState::ControlCreateIntent);
    preparing = cas(executor, preparing, journal)?;
    let parent = mutation_id_anchor
        .parent()
        .ok_or_else(|| crate::failed("mutation id anchor has no parent"))?;
    let root = open_or_create_control_parent(parent)?;
    let capability = capability_from_hex(journal.control_namespace().capability().as_str())?;
    let control = crate::fs::ControlNamespace::create(
        &root,
        preparing.begin().operation_id(),
        &capability,
        crate::fs::AuthorityMode::CooperativeSameUid,
    )?;
    journal
        .control_namespace_mut()
        .set_identity(control.identity().to_owned())
        .map_err(|error| crate::failed(error.to_string()))?;
    journal.set_materialization(DomainMaterializationState::Workspaces {
        next_workspace_id: 0,
    });
    preparing = cas(executor, preparing, journal)?;

    for workspace_id in 0..journal.private_workspaces().len() {
        let workspace_id =
            u32::try_from(workspace_id).map_err(|_| crate::failed("workspace id overflow"))?;
        journal.set_materialization(DomainMaterializationState::WorkspaceCreateIntent {
            workspace_id,
        });
        preparing = cas(executor, preparing, journal)?;
        let path = derived_private_workspace_path(
            preparing.begin().operation_id(),
            journal,
            workspace_id,
        )?;
        let identity = create_private_directory(&path)?;
        journal.private_workspaces_mut()
            [usize::try_from(workspace_id).map_err(|_| crate::failed("workspace id overflow"))?]
        .set_identity(identity)
        .map_err(|error| crate::failed(error.to_string()))?;
        journal.set_materialization(DomainMaterializationState::Workspaces {
            next_workspace_id: workspace_id
                .checked_add(1)
                .ok_or_else(|| crate::failed("workspace id overflow"))?,
        });
        preparing = cas(executor, preparing, journal)?;
    }
    journal.set_materialization(DomainMaterializationState::Ready);
    cas(executor, preparing, journal)
}
