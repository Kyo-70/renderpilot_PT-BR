use super::prelude::*;
use super::*;

pub(in crate::repositories) fn validate_immutable_program(
    current: &OptiScalerJournal,
    next: &OptiScalerJournal,
) -> AppResult<()> {
    if current.kind() != next.kind()
        || current.threat_model() != next.threat_model()
        || current.roots() != next.roots()
    {
        return Err(AppError::storage_failed(
            "OptiScaler journal changed an immutable program header",
        ));
    }
    if !same_control_namespace_static(current, next) {
        return Err(AppError::storage_failed(
            "OptiScaler journal changed its immutable namespace binding",
        ));
    }
    if current.operations().len() != next.operations().len() {
        return Err(AppError::storage_failed(
            "OptiScaler journal changed its operation program",
        ));
    }
    if current.private_workspaces().len() != next.private_workspaces().len() {
        return Err(AppError::storage_failed(
            "OptiScaler journal changed its workspace program",
        ));
    }
    for (left, right) in current
        .private_workspaces()
        .iter()
        .zip(next.private_workspaces())
    {
        if !same_workspace_static(left, right) {
            return Err(AppError::storage_failed(
                "OptiScaler journal changed immutable workspace metadata",
            ));
        }
    }
    for (left, right) in current.operations().iter().zip(next.operations()) {
        if left.operation_id() != right.operation_id()
            || left.parent_dependencies() != right.parent_dependencies()
            || left.workspace_id() != right.workspace_id()
            || left.effect().endpoints().len() != right.effect().endpoints().len()
        {
            return Err(AppError::storage_failed(
                "OptiScaler journal changed immutable operation metadata",
            ));
        }
        for (left_endpoint, right_endpoint) in left
            .effect()
            .endpoints()
            .into_iter()
            .zip(right.effect().endpoints())
        {
            if left_endpoint.endpoint() != right_endpoint.endpoint()
                || normalized_path_key(left_endpoint.path())
                    != normalized_path_key(right_endpoint.path())
                || left_endpoint.preimage() != right_endpoint.preimage()
            {
                return Err(AppError::storage_failed(
                    "OptiScaler journal changed an immutable operation endpoint",
                ));
            }
        }
        if std::mem::discriminant(left.effect()) != std::mem::discriminant(right.effect()) {
            return Err(AppError::storage_failed(
                "OptiScaler journal changed an operation action",
            ));
        }
    }
    Ok(())
}

pub(in crate::repositories) fn same_workspace_static(
    left: &renderpilot_domain::PrivateWorkspaceBinding,
    right: &renderpilot_domain::PrivateWorkspaceBinding,
) -> bool {
    left.workspace_id() == right.workspace_id()
        && left.root_index() == right.root_index()
        && normalized_path_key(left.path()) == normalized_path_key(right.path())
        && left.capability() == right.capability()
}

pub(in crate::repositories) fn same_control_namespace_static(
    left: &OptiScalerJournal,
    right: &OptiScalerJournal,
) -> bool {
    normalized_path_key(left.control_namespace().path())
        == normalized_path_key(right.control_namespace().path())
        && left.control_namespace().capability() == right.control_namespace().capability()
}

pub(in crate::repositories) fn validate_identity_progress(
    current: &OptiScalerJournal,
    next: &OptiScalerJournal,
) -> AppResult<()> {
    ensure_identity_progress(
        current.control_namespace().identity(),
        next.control_namespace().identity(),
        "control namespace",
    )?;
    for (left, right) in current
        .private_workspaces()
        .iter()
        .zip(next.private_workspaces())
    {
        ensure_identity_progress(left.identity(), right.identity(), "workspace")?;
    }
    Ok(())
}

pub(in crate::repositories) fn ensure_identity_progress(
    left: Option<&str>,
    right: Option<&str>,
    label: &str,
) -> AppResult<()> {
    if left.is_some() && left != right {
        return Err(AppError::storage_failed(format!(
            "OptiScaler {label} identity changed after materialization"
        )));
    }
    Ok(())
}

pub(in crate::repositories) fn validate_expected_after_progress(
    current: &OptiScalerJournal,
    next: &OptiScalerJournal,
) -> AppResult<()> {
    for (left, right) in current.operations().iter().zip(next.operations()) {
        let transition = transition_direction(left.effect(), right.effect());
        for (left_endpoint, right_endpoint) in left
            .effect()
            .endpoints()
            .into_iter()
            .zip(right.effect().endpoints())
        {
            match (
                left_endpoint.expected_after(),
                right_endpoint.expected_after(),
            ) {
                (ExpectedAfter::Pending, ExpectedAfter::Pending)
                | (ExpectedAfter::Known(_), ExpectedAfter::Known(_))
                    if left_endpoint.expected_after() == right_endpoint.expected_after() => {}
                (ExpectedAfter::Pending, ExpectedAfter::Known(observation))
                    if observation.is_exact() =>
                {
                    let applied = applied_observation(right.effect(), right_endpoint.endpoint());
                    let preserved = right.effect().is_preserved()
                        && matches!(
                            transition,
                            Some(renderpilot_domain::OptiScalerTransitionDirection::Reverse)
                        )
                        && resolve_preimage(current, left_endpoint) == Some(observation);
                    if !((matches!(
                        transition,
                        Some(renderpilot_domain::OptiScalerTransitionDirection::Forward)
                    ) && applied == Some(observation))
                        || preserved)
                    {
                        return Err(AppError::storage_failed(
                            "OptiScaler endpoint postimage changed outside its terminal edge",
                        ));
                    }
                }
                _ => {
                    return Err(AppError::storage_failed(
                        "OptiScaler endpoint postimage changed outside its Applied edge",
                    ));
                }
            }
        }
    }
    Ok(())
}

pub(in crate::repositories) fn validate_effect_progress(
    current: &OptiScalerJournal,
    next: &OptiScalerJournal,
) -> AppResult<()> {
    for (index, (left, right)) in current
        .operations()
        .iter()
        .zip(next.operations())
        .enumerate()
    {
        if !legal_effect_transition(left.effect(), right.effect()) {
            return Err(AppError::storage_failed(format!(
                "OptiScaler operation {} has an illegal action transition",
                left.operation_id()
            )));
        }
        if left.effect() == right.effect() {
            if left.slots() != right.slots() {
                validate_cleanup_slot_delta(current, next, index, left, right)?;
            }
        } else {
            renderpilot_domain::validate_optiscaler_operation_transition(left, right)
                .map_err(|error| AppError::storage_failed(error.to_string()))?;
            validate_storage_preimage_transition(current, left, right)?;
        }
    }
    Ok(())
}

pub(in crate::repositories) fn validate_cleanup_slot_delta(
    current: &OptiScalerJournal,
    next: &OptiScalerJournal,
    index: usize,
    left: &renderpilot_domain::OperationRecord,
    right: &renderpilot_domain::OperationRecord,
) -> AppResult<()> {
    let renderpilot_domain::CleanupState::ArtifactRemoveIntent {
        operation_id,
        artifact,
        expected,
    } = current.cleanup()
    else {
        return Err(AppError::storage_failed(
            "OptiScaler same-state CAS changed artifact slot custody",
        ));
    };
    if next.cleanup() != &renderpilot_domain::CleanupState::Inactive
        || usize::try_from(*operation_id).ok() != Some(index)
    {
        return Err(AppError::storage_failed(
            "OptiScaler cleanup slot change skipped its cursor",
        ));
    }
    let (before, after) = match artifact {
        renderpilot_domain::ArtifactSlot::Custody => {
            (left.slots().custody(), right.slots().custody())
        }
        renderpilot_domain::ArtifactSlot::Stage => (left.slots().stage(), right.slots().stage()),
        renderpilot_domain::ArtifactSlot::Discard => {
            (left.slots().discard(), right.slots().discard())
        }
    };
    if before != expected || !matches!(after, DurableObservation::Absent) {
        return Err(AppError::storage_failed(
            "OptiScaler cleanup did not clear its exact artifact slot",
        ));
    }
    let unchanged = match artifact {
        renderpilot_domain::ArtifactSlot::Custody => {
            left.slots().stage() == right.slots().stage()
                && left.slots().discard() == right.slots().discard()
        }
        renderpilot_domain::ArtifactSlot::Stage => {
            left.slots().custody() == right.slots().custody()
                && left.slots().discard() == right.slots().discard()
        }
        renderpilot_domain::ArtifactSlot::Discard => {
            left.slots().custody() == right.slots().custody()
                && left.slots().stage() == right.slots().stage()
        }
    };
    if !unchanged {
        return Err(AppError::storage_failed(
            "OptiScaler cleanup changed more than its named artifact slot",
        ));
    }
    Ok(())
}

pub(in crate::repositories) fn validate_storage_preimage_transition(
    current: &OptiScalerJournal,
    left_record: &renderpilot_domain::OperationRecord,
    right_record: &renderpilot_domain::OperationRecord,
) -> AppResult<()> {
    let invalid = || AppError::storage_failed("OptiScaler preimage continuity is invalid");
    match (left_record.effect(), right_record.effect()) {
        (OperationEffect::Write(left), OperationEffect::Write(right)) => {
            use renderpilot_domain::WriteState;
            let endpoint = left.endpoint();
            match (left.state(), right.state()) {
                (WriteState::CaptureIntent { .. }, WriteState::Captured { custody, .. }) => {
                    let preimage = resolve_preimage(current, endpoint).ok_or_else(invalid)?;
                    let custody_matches = match (preimage, custody) {
                        (DurableObservation::Absent, DurableObservation::Absent) => true,
                        (
                            DurableObservation::File {
                                digest: preimage_digest,
                                ..
                            },
                            DurableObservation::File { digest, .. },
                        ) => digest == preimage_digest,
                        _ => false,
                    };
                    if !custody_matches {
                        return Err(invalid());
                    }
                }
                (
                    WriteState::PublishIntent { stage, custody },
                    WriteState::Applied {
                        live,
                        custody: right_custody,
                    },
                ) => {
                    if custody != right_custody {
                        return Err(invalid());
                    }
                    let preimage = resolve_preimage(current, endpoint).ok_or_else(invalid)?;
                    match preimage {
                        DurableObservation::Absent => {
                            if !matches!(custody, DurableObservation::Absent) || live != stage {
                                return Err(invalid());
                            }
                        }
                        DurableObservation::File { identity, digest } => {
                            let DurableObservation::File {
                                identity: live_identity,
                                digest: live_digest,
                            } = live
                            else {
                                return Err(invalid());
                            };
                            let DurableObservation::File {
                                digest: custody_digest,
                                ..
                            } = custody
                            else {
                                return Err(invalid());
                            };
                            if live_identity != identity
                                || live_digest != stage.digest().ok_or_else(invalid)?
                                || custody_digest != digest
                            {
                                return Err(invalid());
                            }
                        }
                        _ => return Err(invalid()),
                    }
                }
                (
                    WriteState::Applied { live, .. },
                    WriteState::DiscardIntent { postimage, custody },
                ) => {
                    if live != postimage
                        || !matches!(custody, DurableObservation::Absent)
                        || !matches!(
                            resolve_preimage(current, endpoint),
                            Some(DurableObservation::Absent)
                        )
                    {
                        return Err(invalid());
                    }
                }
                (
                    WriteState::Applied { live, .. },
                    WriteState::RestoreIntent { preimage, discard },
                ) if live != discard || resolve_preimage(current, endpoint) != Some(preimage) => {
                    return Err(invalid());
                }
                _ => {}
            }
        }
        (OperationEffect::Relocate(left), OperationEffect::Relocate(right)) => {
            use renderpilot_domain::RelocateState;
            if let (
                RelocateState::MoveIntent,
                RelocateState::Applied {
                    source_after,
                    destination_after,
                },
            ) = (left.state(), right.state())
            {
                if !matches!(source_after, DurableObservation::Absent)
                    || !matches!(destination_after, DurableObservation::File { .. })
                {
                    return Err(invalid());
                }
                if let Some(source_before) = initial_preimage(left.source())
                    && destination_after != source_before
                {
                    return Err(invalid());
                }
                if let Some(destination_before) = initial_preimage(left.destination())
                    && !matches!(destination_before, DurableObservation::Absent)
                {
                    return Err(invalid());
                }
            }
        }
        _ => {}
    }
    Ok(())
}
