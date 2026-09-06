fn hash() -> Sha256Hash {
    Sha256Hash::new("a".repeat(64)).expect("valid hash")
}

fn capability() -> NamespaceCapability {
    NamespaceCapability::new("a".repeat(64)).expect("capability")
}

fn capability_text() -> String {
    capability().as_str().to_owned()
}

fn namespace() -> ControlNamespaceBinding {
    ControlNamespaceBinding::new(
        format!("transactions/control-abc-{}", capability_text()),
        None,
        capability(),
    )
    .expect("namespace")
}

fn private_path(operation_id: u32) -> String {
    format!(
        "game/.renderpilot-optiscaler-workspace-abc-{operation_id}-{}",
        capability_text()
    )
}

fn private_workspace(workspace_id: u32) -> PrivateWorkspaceBinding {
    PrivateWorkspaceBinding::new(
        workspace_id,
        0,
        private_path(workspace_id),
        None,
        capability(),
    )
    .expect("workspace")
}

fn journal_with_operations(
    roots: Vec<String>,
    control: ControlNamespaceBinding,
    operations: Vec<OperationRecord>,
) -> Result<OptiScalerJournal, OptiScalerJournalError> {
    let workspaces = operations
        .iter()
        .filter_map(super::OperationRecord::workspace_id)
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .map(private_workspace)
        .collect();
    OptiScalerJournal::new(roots, control, workspaces, operations)
}

fn slots() -> PrivateArtifactSlots {
    PrivateArtifactSlots::new(
        DurableObservation::Absent,
        DurableObservation::Absent,
        DurableObservation::Absent,
    )
    .expect("artifacts")
}

fn operation() -> OperationRecord {
    let endpoint = OperationEndpoint::new(
        Endpoint::Single,
        "game/dxgi.dll",
        Preimage::Initial {
            observation: DurableObservation::Absent,
            receipt: None,
            owned_basis: None,
        },
        ExpectedAfter::Pending,
    )
    .expect("endpoint");
    OperationRecord::new(
        0,
        Vec::new(),
        Some(0),
        slots(),
        OperationEffect::Write(WriteEffect {
            endpoint,
            state: WriteState::Planned,
        }),
    )
    .expect("operation")
}
