use super::prelude::*;
use super::*;

pub(in crate::repositories) fn validate_materialization_identity_state(
    journal: &OptiScalerJournal,
) -> AppResult<()> {
    let require_control = || {
        if journal.control_namespace().identity().is_none() {
            Err(AppError::storage_failed(
                "OptiScaler materialization lacks a control namespace identity",
            ))
        } else {
            Ok(())
        }
    };
    let require_workspace_frontier = |count: usize| {
        for (index, workspace) in journal.private_workspaces().iter().enumerate() {
            if workspace.identity().is_some() != (index < count) {
                return Err(AppError::storage_failed(
                    "OptiScaler workspace identity frontier is inconsistent",
                ));
            }
        }
        Ok(())
    };
    match journal.materialization() {
        MaterializationState::Planned | MaterializationState::ControlCreateIntent => {
            if journal.control_namespace().identity().is_some() {
                return Err(AppError::storage_failed(
                    "unmaterialized OptiScaler journal already contains namespace identities",
                ));
            }
            require_workspace_frontier(0)?;
        }
        MaterializationState::Workspaces { next_workspace_id } => {
            require_control()?;
            let count = usize::try_from(*next_workspace_id)
                .map_err(|_| AppError::storage_failed("OptiScaler workspace frontier overflow"))?;
            if count > journal.private_workspaces().len() {
                return Err(AppError::storage_failed(
                    "OptiScaler workspace frontier exceeds the program",
                ));
            }
            require_workspace_frontier(count)?;
        }
        MaterializationState::WorkspaceCreateIntent { workspace_id } => {
            require_control()?;
            let count = usize::try_from(*workspace_id)
                .map_err(|_| AppError::storage_failed("OptiScaler workspace intent overflow"))?;
            if count >= journal.private_workspaces().len() {
                return Err(AppError::storage_failed(
                    "OptiScaler workspace intent exceeds the program",
                ));
            }
            require_workspace_frontier(count)?;
        }
        MaterializationState::Ready => {
            require_control()?;
            require_workspace_frontier(journal.private_workspaces().len())?;
        }
    }
    Ok(())
}

pub(in crate::repositories) fn validate_materialization_transition(
    current: &MaterializationState,
    next: &MaterializationState,
    workspace_count: usize,
) -> AppResult<()> {
    if current == next {
        return Ok(());
    }
    let legal = match (current, next) {
        (MaterializationState::Planned, MaterializationState::ControlCreateIntent)
        | (
            MaterializationState::ControlCreateIntent,
            MaterializationState::Workspaces {
                next_workspace_id: 0,
            },
        ) => true,
        (
            MaterializationState::Workspaces { next_workspace_id },
            MaterializationState::WorkspaceCreateIntent { workspace_id },
        ) => next_workspace_id == workspace_id,
        (
            MaterializationState::WorkspaceCreateIntent { workspace_id },
            MaterializationState::Workspaces { next_workspace_id },
        ) => workspace_id.checked_add(1) == Some(*next_workspace_id),
        (MaterializationState::Workspaces { next_workspace_id }, MaterializationState::Ready) => {
            usize::try_from(*next_workspace_id).ok() == Some(workspace_count)
        }
        _ => false,
    };
    if legal {
        Ok(())
    } else {
        Err(AppError::storage_failed(
            "OptiScaler workspace materialization skipped a legal state",
        ))
    }
}

pub(in crate::repositories) fn validate_prepared_journal(
    journal: &OptiScalerJournal,
) -> AppResult<()> {
    validate_journal_shape(journal)?;
    if journal.materialization() != &MaterializationState::Ready
        || journal.cleanup() != &renderpilot_domain::CleanupState::Inactive
    {
        return Err(AppError::storage_failed(
            "prepared OptiScaler journal has an invalid lifecycle state",
        ));
    }
    for operation in journal.operations() {
        if operation.effect().is_precommit() {
            if !operation.effect().is_applied() {
                return Err(AppError::storage_failed(
                    "prepared OptiScaler journal contains a nonterminal operation",
                ));
            }
        } else if !operation.effect().is_planned() {
            return Err(AppError::storage_failed(
                "prepared OptiScaler cleanup operation is not planned",
            ));
        }
    }
    Ok(())
}
