fn slot_observation(slots: &PrivateArtifactSlots, artifact: ArtifactSlot) -> &DurableObservation {
    match artifact {
        ArtifactSlot::Custody => slots.custody(),
        ArtifactSlot::Stage => slots.stage(),
        ArtifactSlot::Discard => slots.discard(),
    }
}

fn validate_cleanup_cursor(journal: &OptiScalerJournal) -> Result<(), OptiScalerJournalError> {
    match journal.cleanup() {
        CleanupState::Inactive => {}
        CleanupState::Complete => {
            if !journal.slots_are_empty() {
                return Err(OptiScalerJournalError::Invalid(
                    "complete cleanup requires all artifact slots to be absent",
                ));
            }
        }
        CleanupState::ArtifactRemoveIntent {
            operation_id,
            artifact,
            expected,
        } => {
            let index = usize::try_from(*operation_id).map_err(|_| {
                OptiScalerJournalError::Invalid("cleanup operation ordinal overflows usize")
            })?;
            let operation =
                journal
                    .operations()
                    .get(index)
                    .ok_or(OptiScalerJournalError::Invalid(
                        "cleanup operation does not exist",
                    ))?;
            if !expected.is_exact() || matches!(expected, DurableObservation::Absent) {
                return Err(OptiScalerJournalError::Invalid(
                    "artifact cleanup requires an occupied exact slot",
                ));
            }
            if slot_observation(operation.slots(), *artifact) != expected {
                return Err(OptiScalerJournalError::Invalid(
                    "artifact cleanup intent is not bound to the current slot",
                ));
            }
        }
        CleanupState::WorkspaceRemoveIntent {
            workspace_id,
            expected_identity,
        } => {
            if !journal.slots_are_empty() {
                return Err(OptiScalerJournalError::Invalid(
                    "workspace cleanup requires all artifact slots to be absent",
                ));
            }
            let index = usize::try_from(*workspace_id).map_err(|_| {
                OptiScalerJournalError::Invalid("cleanup workspace ordinal overflows usize")
            })?;
            let workspace =
                journal
                    .private_workspaces()
                    .get(index)
                    .ok_or(OptiScalerJournalError::Invalid(
                        "cleanup workspace does not exist",
                    ))?;
            if workspace.identity() != Some(expected_identity.as_str()) {
                return Err(OptiScalerJournalError::Invalid(
                    "workspace cleanup intent is not bound to its identity",
                ));
            }
        }
        CleanupState::ControlRemoveIntent { expected_identity } => {
            if !journal.slots_are_empty() {
                return Err(OptiScalerJournalError::Invalid(
                    "control cleanup requires all artifact slots to be absent",
                ));
            }
            if journal.control_namespace().identity() != Some(expected_identity.as_str()) {
                return Err(OptiScalerJournalError::Invalid(
                    "control cleanup intent is not bound to its identity",
                ));
            }
        }
    }
    Ok(())
}

fn validate_rollback_latch(operations: &[OperationRecord]) -> Result<(), OptiScalerJournalError> {
    let mut latched = false;
    for operation in operations {
        let effect = operation.effect();
        if !effect.is_precommit() {
            continue;
        }
        if latched && !effect.is_reverse_or_preserved() {
            return Err(OptiScalerJournalError::Invalid(
                "forward action is not allowed after rollback latch",
            ));
        }
        if effect.is_reverse_or_preserved() {
            latched = true;
        }
    }
    Ok(())
}
