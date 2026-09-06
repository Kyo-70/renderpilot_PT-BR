/// The sole direction vocabulary for an OptiScaler action CAS edge.
///
/// Storage owns persistence and compare-and-swap, but it must not invent a
/// second state graph.  This small, domain-local result is intentionally
/// concrete: callers can use it to select the lifecycle frontier while all
/// legal action edges remain defined here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptiScalerTransitionDirection {
    /// The native action advances toward its applied state.
    Forward,
    /// The native action is being abandoned or reversed.
    Reverse,
}
/// Lifecycle context supplied by the pending-row owner while validating the
/// cleanup overlay. The journal wire itself intentionally does not persist
/// this context: the same journal shape can be resumed from either rollback
/// or committed storage state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptiScalerCleanupLifecycle {
    /// Cleanup of artifacts retained by a rollback.
    Rollback,
    /// Cleanup of committed custody artifacts and post-commit directories.
    Committed,
}

fn same_endpoint_identity(left: &OperationEndpoint, right: &OperationEndpoint) -> bool {
    left.endpoint() == right.endpoint()
        && normalized_path_key(left.path()) == normalized_path_key(right.path())
        && left.preimage() == right.preimage()
}

/// Validates the only action/state/slot combinations that may enter the
/// artifact cleanup cursor.
pub fn validate_optiscaler_cleanup_artifact(
    effect: &OperationEffect,
    artifact: ArtifactSlot,
    lifecycle: OptiScalerCleanupLifecycle,
) -> Result<(), OptiScalerJournalError> {
    let eligible = match (lifecycle, effect, artifact) {
        (
            OptiScalerCleanupLifecycle::Committed,
            OperationEffect::Write(effect),
            ArtifactSlot::Custody,
        ) => matches!(effect.state(), WriteState::Applied { .. }),
        (
            OptiScalerCleanupLifecycle::Committed,
            OperationEffect::Delete(effect),
            ArtifactSlot::Custody,
        ) => matches!(effect.state(), DeleteState::Applied { .. }),
        (
            OptiScalerCleanupLifecycle::Rollback,
            OperationEffect::Write(effect),
            ArtifactSlot::Custody | ArtifactSlot::Stage,
        ) => matches!(effect.state(), WriteState::Preserved),
        (
            OptiScalerCleanupLifecycle::Rollback,
            OperationEffect::CreateDirectory(effect),
            ArtifactSlot::Stage,
        ) => matches!(effect.state(), CreateDirectoryState::Preserved),
        _ => false,
    };
    if eligible {
        Ok(())
    } else {
        Err(OptiScalerJournalError::Invalid(
            "cleanup artifact is not authorized for its action state and lifecycle",
        ))
    }
}

/// Validates cleanup state that depends on the pending-row lifecycle context.
/// Structural journal deserialization remains context-free; storage must call
/// this boundary immediately after decoding both current and candidate CAS
/// journals, and before accepting a terminal cleanup journal.
pub fn validate_optiscaler_cleanup_overlay(
    journal: &OptiScalerJournal,
    lifecycle: OptiScalerCleanupLifecycle,
) -> Result<(), OptiScalerJournalError> {
    match journal.cleanup() {
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
            validate_optiscaler_cleanup_artifact(operation.effect(), *artifact, lifecycle)?;
            if slot_observation(operation.slots(), *artifact) != expected {
                return Err(OptiScalerJournalError::Invalid(
                    "artifact cleanup intent is not bound to the current slot",
                ));
            }
        }
        CleanupState::WorkspaceRemoveIntent { .. }
        | CleanupState::ControlRemoveIntent { .. }
        | CleanupState::Complete => {
            if !journal.slots_are_empty() {
                return Err(OptiScalerJournalError::Invalid(
                    "namespace cleanup requires all artifact slots to be absent",
                ));
            }
        }
        CleanupState::Inactive => {}
    }
    Ok(())
}
