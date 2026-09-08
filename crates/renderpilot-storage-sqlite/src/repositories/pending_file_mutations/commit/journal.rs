use super::prelude::*;
use super::*;

pub(in crate::repositories) fn parse_journal(
    json: &str,
    context: &str,
) -> AppResult<OptiScalerJournal> {
    let journal = deserialize_journal(json, context)?;
    validate_journal_shape(&journal)?;
    Ok(journal)
}

/// Parses the terminal journal retained after committed artifact cleanup.
///
/// A committed row may legitimately have lost the custody artifact that was
/// recorded by an applied write/delete. That absence is historical evidence
/// of the committed cleanup cursor, not a general relaxation of journal
/// shape. Keep this parser separate from the ordinary and rollback/prepared
/// parsers so the exception is available only at the committed terminal
/// boundary.
pub(in crate::repositories) fn parse_committed_terminal_journal(
    json: &str,
    context: &str,
) -> AppResult<OptiScalerJournal> {
    let journal = deserialize_journal(json, context)?;
    validate_journal_shape_for_committed_terminal(&journal)?;
    Ok(journal)
}

pub(in crate::repositories) fn deserialize_journal(
    json: &str,
    context: &str,
) -> AppResult<OptiScalerJournal> {
    serde_json::from_str(json)
        .map_err(|error| AppError::storage_failed(format!("{context} is invalid: {error}")))
}

pub(in crate::repositories) fn validate_journal_shape(
    journal: &OptiScalerJournal,
) -> AppResult<()> {
    validate_journal_shape_with_artifact_clear(journal, None)
}

pub(in crate::repositories) fn validate_journal_shape_with_artifact_clear(
    journal: &OptiScalerJournal,
    cleared_artifact: Option<(u32, renderpilot_domain::ArtifactSlot)>,
) -> AppResult<()> {
    validate_journal_shape_with_options(journal, cleared_artifact, false)
}

pub(in crate::repositories) fn validate_journal_shape_for_cas(
    journal: &OptiScalerJournal,
    row_state: &str,
    cleared_artifact: Option<(u32, renderpilot_domain::ArtifactSlot)>,
) -> AppResult<()> {
    validate_journal_shape_with_options(
        journal,
        cleared_artifact,
        row_state == PendingFileMutationState::Committed.as_str(),
    )
}

pub(in crate::repositories) fn validate_journal_shape_for_committed_terminal(
    journal: &OptiScalerJournal,
) -> AppResult<()> {
    validate_journal_shape_with_options(journal, None, true)
}

pub(in crate::repositories) fn validate_journal_shape_with_options(
    journal: &OptiScalerJournal,
    cleared_artifact: Option<(u32, renderpilot_domain::ArtifactSlot)>,
    allow_terminal_artifact_absence: bool,
) -> AppResult<()> {
    if journal.kind() != OptiScalerJournalKind::OptiScaler {
        return Err(AppError::storage_failed(
            "OptiScaler journal has an invalid kind",
        ));
    }
    if journal.threat_model() != ThreatModel::CooperativeSameUid {
        return Err(AppError::storage_failed(
            "OptiScaler journal requires cooperative namespace authority",
        ));
    }
    journal
        .control_namespace()
        .validate()
        .map_err(|error| AppError::storage_failed(error.to_string()))?;
    if journal.control_namespace().path().contains('\0')
        || journal
            .roots()
            .iter()
            .any(|root| root.trim().is_empty() || root.contains('\0'))
    {
        return Err(AppError::storage_failed(
            "OptiScaler journal contains an invalid lexical path",
        ));
    }
    for (index, operation) in journal.operations().iter().enumerate() {
        let expected = u32::try_from(index).map_err(|_| {
            AppError::storage_failed("OptiScaler journal operation ordinal overflow")
        })?;
        if operation.operation_id() != expected {
            return Err(AppError::storage_failed(
                "OptiScaler journal operation ordinals are not contiguous",
            ));
        }
        operation
            .validate()
            .map_err(|error| AppError::storage_failed(error.to_string()))?;
        validate_state_observations(operation.effect())?;
        validate_endpoint_state_contract(operation.effect())?;
        validate_artifact_state_shape(
            operation,
            cleared_artifact
                .filter(|(operation_id, _)| *operation_id == operation.operation_id())
                .map(|(_, artifact)| artifact),
            allow_terminal_artifact_absence,
        )?;
        for endpoint in operation.effect().endpoints() {
            if endpoint.path().contains('\0') {
                return Err(AppError::storage_failed(
                    "OptiScaler journal contains a NUL target path",
                ));
            }
        }
    }
    validate_preimage_bindings(journal)?;
    validate_namespace_shape(journal)
}

pub(in crate::repositories) fn validate_namespace_shape(
    journal: &OptiScalerJournal,
) -> AppResult<()> {
    let control = journal.control_namespace().path();
    if has_parent_escape(control) {
        return Err(AppError::storage_failed(
            "OptiScaler control namespace has an invalid lexical custody path",
        ));
    }
    for workspace in journal.private_workspaces() {
        let private = workspace.path();
        if has_parent_escape(private) || private == control {
            return Err(AppError::storage_failed(
                "OptiScaler private workspace has an invalid lexical custody path",
            ));
        }
        for endpoint in journal
            .operations()
            .iter()
            .flat_map(|operation| operation.effect().endpoints())
        {
            if renderpilot_domain::normalized_path_relation(private, endpoint.path())
                != renderpilot_domain::NormalizedPathRelation::Disjoint
            {
                return Err(AppError::storage_failed(
                    "OptiScaler private workspace overlaps a target path",
                ));
            }
        }
    }
    Ok(())
}

pub(in crate::repositories) fn validate_state_observations(
    effect: &OperationEffect,
) -> AppResult<()> {
    use renderpilot_domain::{
        CreateDirectoryState, DeleteState, RelocateState, RemoveDirectoryState, VerifyState,
        WriteState,
    };
    let mut observations = Vec::new();
    match effect {
        OperationEffect::Write(effect) => match effect.state() {
            WriteState::Staged { stage } | WriteState::CaptureIntent { stage } => {
                observations.push(stage);
            }
            WriteState::Captured { stage, custody }
            | WriteState::PublishIntent { stage, custody } => observations.extend([stage, custody]),
            WriteState::Applied { live, custody } => observations.extend([live, custody]),
            WriteState::DiscardIntent { postimage, custody } => {
                observations.extend([postimage, custody]);
            }
            WriteState::PostimageDiscarded { discard, custody } => {
                observations.extend([discard, custody]);
            }
            WriteState::RestoreIntent { preimage, discard } => {
                observations.extend([preimage, discard]);
            }
            WriteState::Planned | WriteState::StageIntent { .. } | WriteState::Preserved => {}
        },
        OperationEffect::Delete(effect) => match effect.state() {
            DeleteState::Captured { custody }
            | DeleteState::Applied { custody }
            | DeleteState::RestoreIntent { preimage: custody } => observations.push(custody),
            DeleteState::Planned | DeleteState::CaptureIntent | DeleteState::Preserved => {}
        },
        OperationEffect::Verify(effect) => {
            if let VerifyState::Applied { observed } = effect.state() {
                observations.push(observed);
            }
        }
        OperationEffect::Relocate(effect) => {
            if let RelocateState::Applied {
                source_after,
                destination_after,
            } = effect.state()
            {
                observations.extend([source_after, destination_after]);
            }
        }
        OperationEffect::CreateDirectory(effect) => match effect.state() {
            CreateDirectoryState::Staged { stage }
            | CreateDirectoryState::PublishIntent { stage } => observations.push(stage),
            CreateDirectoryState::Applied { live } => observations.push(live),
            CreateDirectoryState::DiscardIntent { directory } => observations.push(directory),
            CreateDirectoryState::PostimageDiscarded { discard } => observations.push(discard),
            CreateDirectoryState::Planned
            | CreateDirectoryState::StageIntent
            | CreateDirectoryState::Preserved => {}
        },
        OperationEffect::PostCommitRemoveDirectory(effect) => match effect.state() {
            RemoveDirectoryState::Planned { directory }
            | RemoveDirectoryState::Applied { directory } => observations.push(directory),
            RemoveDirectoryState::RemoveIntent { directory, live } => {
                observations.extend([directory, live]);
            }
        },
    }
    if observations
        .iter()
        .any(|observation| !observation.is_exact())
    {
        return Err(AppError::storage_failed(
            "OptiScaler intent or result contains an uncertain observation",
        ));
    }
    Ok(())
}

pub(in crate::repositories) fn validate_artifact_state_shape(
    operation: &renderpilot_domain::OperationRecord,
    cleared_artifact: Option<renderpilot_domain::ArtifactSlot>,
    allow_terminal_artifact_absence: bool,
) -> AppResult<()> {
    use renderpilot_domain::{CreateDirectoryState, DeleteState, WriteState};
    let artifacts = operation.slots();
    let absent = DurableObservation::Absent;
    let (custody, stage, discard) = match operation.effect() {
        OperationEffect::Write(effect) => match effect.state() {
            WriteState::Planned | WriteState::StageIntent { .. } => (&absent, &absent, &absent),
            WriteState::Staged { stage } | WriteState::CaptureIntent { stage } => {
                (&absent, stage, &absent)
            }
            WriteState::Captured { stage, custody }
            | WriteState::PublishIntent { stage, custody } => (custody, stage, &absent),
            WriteState::Applied { custody, .. }
            | WriteState::DiscardIntent { custody, .. }
            | WriteState::PostimageDiscarded { custody, .. } => (custody, &absent, &absent),
            WriteState::RestoreIntent { preimage, .. } => {
                let DurableObservation::File {
                    digest: preimage_digest,
                    ..
                } = preimage
                else {
                    return Err(AppError::storage_failed(
                        "OptiScaler restore intent requires an existing file preimage",
                    ));
                };
                let custody_matches = match artifacts.custody() {
                    DurableObservation::Absent => true,
                    DurableObservation::File { digest, .. } => digest == preimage_digest,
                    DurableObservation::Directory { .. }
                    | DurableObservation::NonRegular
                    | DurableObservation::Unreadable => false,
                };
                if !custody_matches
                    || artifacts.stage() != &absent
                    || artifacts.discard() != &absent
                {
                    return Err(AppError::storage_failed(
                        "OptiScaler restore intent artifacts do not match state",
                    ));
                }
                return Ok(());
            }
            WriteState::Preserved => return Ok(()),
        },
        OperationEffect::Delete(effect) => match effect.state() {
            DeleteState::Captured { custody } | DeleteState::Applied { custody } => {
                (custody, &absent, &absent)
            }
            DeleteState::RestoreIntent { .. } | DeleteState::Preserved => return Ok(()),
            DeleteState::Planned | DeleteState::CaptureIntent => (&absent, &absent, &absent),
        },
        OperationEffect::CreateDirectory(effect) => match effect.state() {
            CreateDirectoryState::Planned
            | CreateDirectoryState::StageIntent
            | CreateDirectoryState::Applied { .. }
            | CreateDirectoryState::DiscardIntent { .. }
            | CreateDirectoryState::PostimageDiscarded { .. } => (&absent, &absent, &absent),
            CreateDirectoryState::Staged { stage }
            | CreateDirectoryState::PublishIntent { stage } => (&absent, stage, &absent),
            CreateDirectoryState::Preserved => return Ok(()),
        },
        OperationEffect::PostCommitRemoveDirectory(_) => (&absent, &absent, &absent),
        OperationEffect::Verify(_) | OperationEffect::Relocate(_) => {
            if ![artifacts.custody(), artifacts.stage(), artifacts.discard()]
                .into_iter()
                .all(|observation| matches!(observation, DurableObservation::Absent))
            {
                return Err(AppError::storage_failed(
                    "OptiScaler verification or relocation has an unexpected artifact slot",
                ));
            }
            return Ok(());
        }
    };
    let terminal_historical_artifact = allow_terminal_artifact_absence
        && match operation.effect() {
            OperationEffect::Write(effect) => {
                matches!(effect.state(), WriteState::Applied { .. })
            }
            OperationEffect::Delete(effect) => {
                matches!(effect.state(), DeleteState::Applied { .. })
            }
            _ => false,
        };
    let slot_matches = |actual: &DurableObservation,
                        expected: &DurableObservation,
                        slot: renderpilot_domain::ArtifactSlot| {
        actual == expected
            || (matches!(actual, DurableObservation::Absent)
                && (cleared_artifact == Some(slot)
                    || (terminal_historical_artifact
                        && !matches!(expected, DurableObservation::Absent))))
    };
    if !slot_matches(
        artifacts.custody(),
        custody,
        renderpilot_domain::ArtifactSlot::Custody,
    ) || !slot_matches(
        artifacts.stage(),
        stage,
        renderpilot_domain::ArtifactSlot::Stage,
    ) || !slot_matches(
        artifacts.discard(),
        discard,
        renderpilot_domain::ArtifactSlot::Discard,
    ) {
        return Err(AppError::storage_failed(
            "OptiScaler artifact slots do not match their action state",
        ));
    }
    Ok(())
}

pub(in crate::repositories) fn validate_endpoint_state_contract(
    effect: &OperationEffect,
) -> AppResult<()> {
    for endpoint in effect.endpoints() {
        if let ExpectedAfter::Known(expected) = endpoint.expected_after() {
            if !state_allows_known_after(effect) {
                return Err(AppError::storage_failed(
                    "OptiScaler endpoint postimage is known before its terminal action edge",
                ));
            }
            if let Some(applied) = applied_observation(effect, endpoint.endpoint())
                && applied != expected
            {
                return Err(AppError::storage_failed(
                    "OptiScaler terminal result does not match its endpoint token",
                ));
            }
        }
    }
    Ok(())
}

pub(in crate::repositories) fn state_allows_known_after(effect: &OperationEffect) -> bool {
    use renderpilot_domain::{
        CreateDirectoryState, DeleteState, RelocateState, RemoveDirectoryState, VerifyState,
        WriteState,
    };
    match effect {
        OperationEffect::Write(effect) => matches!(
            effect.state(),
            WriteState::Applied { .. }
                | WriteState::DiscardIntent { .. }
                | WriteState::PostimageDiscarded { .. }
                | WriteState::RestoreIntent { .. }
                | WriteState::Preserved
        ),
        OperationEffect::Delete(effect) => matches!(
            effect.state(),
            DeleteState::Applied { .. }
                | DeleteState::RestoreIntent { .. }
                | DeleteState::Preserved
        ),
        OperationEffect::Verify(effect) => {
            matches!(
                effect.state(),
                VerifyState::Applied { .. } | VerifyState::Preserved
            )
        }
        OperationEffect::Relocate(effect) => matches!(
            effect.state(),
            RelocateState::Applied { .. } | RelocateState::ReverseIntent | RelocateState::Preserved
        ),
        OperationEffect::CreateDirectory(effect) => matches!(
            effect.state(),
            CreateDirectoryState::Applied { .. }
                | CreateDirectoryState::DiscardIntent { .. }
                | CreateDirectoryState::PostimageDiscarded { .. }
                | CreateDirectoryState::Preserved
        ),
        OperationEffect::PostCommitRemoveDirectory(effect) => {
            matches!(effect.state(), RemoveDirectoryState::Applied { .. })
        }
    }
}

pub(in crate::repositories) fn applied_observation(
    effect: &OperationEffect,
    endpoint: Endpoint,
) -> Option<&DurableObservation> {
    static ABSENT: DurableObservation = DurableObservation::Absent;
    use renderpilot_domain::{
        CreateDirectoryState, DeleteState, RelocateState, RemoveDirectoryState, VerifyState,
        WriteState,
    };
    match effect {
        OperationEffect::Write(effect) => match (effect.state(), endpoint) {
            (WriteState::Applied { live, .. }, Endpoint::Single) => Some(live),
            _ => None,
        },
        OperationEffect::Delete(effect) => match (effect.state(), endpoint) {
            (DeleteState::Applied { .. }, Endpoint::Single) => Some(&ABSENT),
            _ => None,
        },
        OperationEffect::Verify(effect) => match (effect.state(), endpoint) {
            (VerifyState::Applied { observed }, Endpoint::Single) => Some(observed),
            _ => None,
        },
        OperationEffect::Relocate(effect) => match (effect.state(), endpoint) {
            (
                RelocateState::Applied {
                    source_after,
                    destination_after: _,
                },
                Endpoint::Source,
            ) => Some(source_after),
            (
                RelocateState::Applied {
                    source_after: _,
                    destination_after,
                },
                Endpoint::Destination,
            ) => Some(destination_after),
            _ => None,
        },
        OperationEffect::CreateDirectory(effect) => match (effect.state(), endpoint) {
            (CreateDirectoryState::Applied { live }, Endpoint::Single) => Some(live),
            _ => None,
        },
        OperationEffect::PostCommitRemoveDirectory(effect) => match (effect.state(), endpoint) {
            (RemoveDirectoryState::Applied { .. }, Endpoint::Single) => Some(&ABSENT),
            _ => None,
        },
    }
}
