fn validate_materialization_frontier(
    journal: &OptiScalerJournal,
) -> Result<(), OptiScalerJournalError> {
    let control = journal.control_namespace();
    let workspaces = journal.private_workspaces();
    let workspace_count = workspaces.len();
    let require_control_identity = || {
        if control.identity().is_none() {
            return Err(OptiScalerJournalError::Invalid(
                "materialized frontier lacks control identity",
            ));
        }
        Ok(())
    };
    let require_no_control_identity = || {
        if control.identity().is_some() {
            return Err(OptiScalerJournalError::Invalid(
                "planned materialization contains control identity",
            ));
        }
        Ok(())
    };
    let require_workspace_frontier = |count: usize| {
        for (index, workspace) in workspaces.iter().enumerate() {
            if workspace.identity().is_some() != (index < count) {
                return Err(OptiScalerJournalError::Invalid(
                    "workspace identity frontier is inconsistent",
                ));
            }
        }
        Ok(())
    };

    match journal.materialization() {
        MaterializationState::Planned | MaterializationState::ControlCreateIntent => {
            require_no_control_identity()?;
            require_workspace_frontier(0)?;
        }
        MaterializationState::Workspaces { next_workspace_id } => {
            require_control_identity()?;
            let count = usize::try_from(*next_workspace_id).map_err(|_| {
                OptiScalerJournalError::Invalid("workspace frontier overflows usize")
            })?;
            if count > workspace_count {
                return Err(OptiScalerJournalError::Invalid(
                    "workspace frontier exceeds the workspace program",
                ));
            }
            require_workspace_frontier(count)?;
        }
        MaterializationState::WorkspaceCreateIntent { workspace_id } => {
            require_control_identity()?;
            let count = usize::try_from(*workspace_id)
                .map_err(|_| OptiScalerJournalError::Invalid("workspace intent overflows usize"))?;
            if count >= workspace_count {
                return Err(OptiScalerJournalError::Invalid(
                    "workspace intent exceeds the workspace program",
                ));
            }
            require_workspace_frontier(count)?;
        }
        MaterializationState::Ready => {
            require_control_identity()?;
            require_workspace_frontier(workspace_count)?;
        }
    }
    Ok(())
}
