use super::prelude::*;
use renderpilot_domain::CleanupState;

pub(in crate::repositories) fn validate_lifecycle_progress(
    current: &OptiScalerJournal,
    next: &OptiScalerJournal,
    row_state: &str,
) -> AppResult<()> {
    match row_state {
        "preparing" | "prepared" => {
            if current.cleanup() != &renderpilot_domain::CleanupState::Inactive
                || next.cleanup() != &renderpilot_domain::CleanupState::Inactive
            {
                if !rollback_cleanup_ready(current) || !rollback_cleanup_ready(next) {
                    return Err(AppError::storage_failed(
                        "OptiScaler cleanup requires preserved rollback state",
                    ));
                }
                validate_cleanup_progress(current, next, row_state)?;
            } else if row_state == "prepared"
                && (current.materialization() != &MaterializationState::Ready
                    || next.materialization() != &MaterializationState::Ready)
            {
                return Err(AppError::storage_failed(
                    "prepared OptiScaler CAS changed workspace materialization",
                ));
            }
        }
        "committed" => {
            if current.materialization() != &MaterializationState::Ready
                || next.materialization() != &MaterializationState::Ready
            {
                return Err(AppError::storage_failed(
                    "committed OptiScaler CAS changed workspace materialization",
                ));
            }
            validate_cleanup_progress(current, next, row_state)?;
        }
        _ => {
            return Err(AppError::storage_failed(
                "OptiScaler CAS has an invalid pending row state",
            ));
        }
    }
    Ok(())
}

pub(in crate::repositories) fn rollback_cleanup_ready(journal: &OptiScalerJournal) -> bool {
    journal.is_rollback_cleanup_ready()
}

pub(in crate::repositories) fn postcommit_cleanup_ready(journal: &OptiScalerJournal) -> bool {
    journal.operations().iter().all(|operation| {
        operation.effect().is_precommit() || operation.effect().is_postcommit_terminal()
    })
}

pub(in crate::repositories) fn first_cleanup_artifact(
    journal: &OptiScalerJournal,
) -> Option<(u32, renderpilot_domain::ArtifactSlot, DurableObservation)> {
    for operation in journal.operations() {
        for (slot, observation) in [
            (
                renderpilot_domain::ArtifactSlot::Custody,
                operation.slots().custody(),
            ),
            (
                renderpilot_domain::ArtifactSlot::Stage,
                operation.slots().stage(),
            ),
            (
                renderpilot_domain::ArtifactSlot::Discard,
                operation.slots().discard(),
            ),
        ] {
            if !matches!(observation, DurableObservation::Absent) {
                return Some((operation.operation_id(), slot, observation.clone()));
            }
        }
    }
    None
}

pub(in crate::repositories) fn validate_cleanup_progress(
    current: &OptiScalerJournal,
    next: &OptiScalerJournal,
    row_state: &str,
) -> AppResult<()> {
    if current.cleanup() == next.cleanup() {
        return Ok(());
    }
    if row_state == "committed" && !postcommit_cleanup_ready(current) {
        return Err(AppError::storage_failed(
            "committed cleanup cannot start before post-commit actions finish",
        ));
    }
    match (current.cleanup(), next.cleanup()) {
        (
            CleanupState::Inactive,
            CleanupState::ArtifactRemoveIntent {
                operation_id,
                artifact,
                expected,
            },
        ) => {
            if first_cleanup_artifact(current) != Some((*operation_id, *artifact, expected.clone()))
            {
                return Err(AppError::storage_failed(
                    "artifact cleanup intent skipped the ordered slot frontier",
                ));
            }
        }
        (CleanupState::ArtifactRemoveIntent { .. }, CleanupState::Inactive) => {}
        (
            CleanupState::Inactive,
            CleanupState::WorkspaceRemoveIntent {
                workspace_id,
                expected_identity,
            },
        ) => {
            if first_cleanup_artifact(current).is_some() {
                return Err(AppError::storage_failed(
                    "workspace cleanup began before artifact cleanup completed",
                ));
            }
            let expected = current
                .private_workspaces()
                .len()
                .checked_sub(1)
                .and_then(|index| u32::try_from(index).ok());
            if expected != Some(*workspace_id)
                || current
                    .private_workspaces()
                    .get(*workspace_id as usize)
                    .and_then(|workspace| workspace.identity())
                    != Some(expected_identity)
            {
                return Err(AppError::storage_failed(
                    "workspace cleanup did not begin at the reverse workspace frontier",
                ));
            }
        }
        (
            CleanupState::WorkspaceRemoveIntent {
                workspace_id,
                expected_identity,
            },
            CleanupState::WorkspaceRemoveIntent {
                workspace_id: next_id,
                expected_identity: next_identity,
            },
        ) => {
            if workspace_id.checked_sub(1) != Some(*next_id)
                || current
                    .private_workspaces()
                    .get(*workspace_id as usize)
                    .and_then(|workspace| workspace.identity())
                    != Some(expected_identity)
                || next
                    .private_workspaces()
                    .get(*next_id as usize)
                    .and_then(|workspace| workspace.identity())
                    != Some(next_identity)
            {
                return Err(AppError::storage_failed(
                    "workspace cleanup cursor skipped or changed its identity proof",
                ));
            }
        }
        (
            CleanupState::WorkspaceRemoveIntent {
                workspace_id,
                expected_identity,
            },
            CleanupState::ControlRemoveIntent {
                expected_identity: next_identity,
            },
        ) => {
            if *workspace_id != 0
                || current
                    .private_workspaces()
                    .first()
                    .and_then(|workspace| workspace.identity())
                    != Some(expected_identity)
                || next.control_namespace().identity() != Some(next_identity)
            {
                return Err(AppError::storage_failed(
                    "control cleanup began before workspace cleanup completed",
                ));
            }
        }
        (CleanupState::Inactive, CleanupState::ControlRemoveIntent { expected_identity }) => {
            if !current.private_workspaces().is_empty()
                || first_cleanup_artifact(current).is_some()
                || current.control_namespace().identity() != Some(expected_identity)
            {
                return Err(AppError::storage_failed(
                    "control cleanup began before the zero-workspace frontier",
                ));
            }
        }
        (CleanupState::ControlRemoveIntent { expected_identity }, CleanupState::Complete) => {
            if current.control_namespace().identity() != Some(expected_identity) {
                return Err(AppError::storage_failed(
                    "control cleanup changed its identity proof",
                ));
            }
        }
        _ => {
            return Err(AppError::storage_failed(
                "OptiScaler cleanup skipped a legal cursor state",
            ));
        }
    }
    Ok(())
}

pub(in crate::repositories) fn legal_effect_transition(
    left: &OperationEffect,
    right: &OperationEffect,
) -> bool {
    left == right || renderpilot_domain::validate_optiscaler_effect_transition(left, right).is_ok()
}

pub(in crate::repositories) fn transition_direction(
    left: &OperationEffect,
    right: &OperationEffect,
) -> Option<renderpilot_domain::OptiScalerTransitionDirection> {
    renderpilot_domain::validate_optiscaler_effect_transition(left, right).ok()
}

pub(in crate::repositories) fn validate_cas_frontier(
    current: &OptiScalerJournal,
    next: &OptiScalerJournal,
    row_state: &str,
) -> AppResult<()> {
    let materialization_changed = current.materialization() != next.materialization();
    let cleanup_changed = current.cleanup() != next.cleanup();
    let clears_artifact = matches!(
        (current.cleanup(), next.cleanup()),
        (
            renderpilot_domain::CleanupState::ArtifactRemoveIntent { .. },
            renderpilot_domain::CleanupState::Inactive
        )
    );
    let changed_effects = current
        .operations()
        .iter()
        .zip(next.operations())
        .enumerate()
        .filter_map(|(index, (left, right))| (left.effect() != right.effect()).then_some(index))
        .collect::<Vec<_>>();
    let changed_slots = current
        .operations()
        .iter()
        .zip(next.operations())
        .enumerate()
        .filter_map(|(index, (left, right))| (left.slots() != right.slots()).then_some(index))
        .collect::<Vec<_>>();
    let changed_workspaces = current
        .private_workspaces()
        .iter()
        .zip(next.private_workspaces())
        .enumerate()
        .filter_map(|(index, (left, right))| (left.identity() != right.identity()).then_some(index))
        .collect::<Vec<_>>();
    if changed_effects.len() > 1 || changed_slots.len() > 1 || changed_workspaces.len() > 1 {
        return Err(AppError::storage_failed(
            "OptiScaler CAS changed multiple frontiers",
        ));
    }
    if !changed_workspaces.is_empty() && (!changed_effects.is_empty() || !changed_slots.is_empty())
    {
        return Err(AppError::storage_failed(
            "OptiScaler CAS mixed workspace materialization with action progress",
        ));
    }
    if cleanup_changed
        && (materialization_changed
            || !changed_effects.is_empty()
            || (!changed_slots.is_empty() && !clears_artifact)
            || !changed_workspaces.is_empty())
    {
        return Err(AppError::storage_failed(
            "OptiScaler CAS mixed cleanup with another durable frontier",
        ));
    }
    if materialization_changed && (!changed_effects.is_empty() || !changed_slots.is_empty()) {
        return Err(AppError::storage_failed(
            "OptiScaler CAS mixed workspace materialization with action progress",
        ));
    }
    if !changed_workspaces.is_empty() && !materialization_changed {
        return Err(AppError::storage_failed(
            "OptiScaler workspace identity changed outside materialization",
        ));
    }
    if row_state == "preparing"
        && let Some(index) = changed_workspaces.first()
    {
        let workspace_id =
            u32::try_from(*index).map_err(|_| AppError::storage_failed("workspace id overflow"))?;
        let next_workspace_id = workspace_id
            .checked_add(1)
            .ok_or_else(|| AppError::storage_failed("workspace id overflow"))?;
        if !matches!(next.materialization(), MaterializationState::WorkspaceCreateIntent { workspace_id: id } if *id == workspace_id)
            && !matches!(next.materialization(), MaterializationState::Workspaces { next_workspace_id: id } if *id == next_workspace_id)
        {
            return Err(AppError::storage_failed(
                "workspace identity CAS does not match its materialization frontier",
            ));
        }
    }
    Ok(())
}
