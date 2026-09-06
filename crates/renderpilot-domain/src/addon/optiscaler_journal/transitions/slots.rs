fn validate_action_slots(
    effect: &OperationEffect,
    slots: &PrivateArtifactSlots,
) -> Result<(), OptiScalerJournalError> {
    let absent =
        |observation: &DurableObservation| matches!(observation, DurableObservation::Absent);
    let file_or_absent = |observation: &DurableObservation| {
        matches!(
            observation,
            DurableObservation::Absent | DurableObservation::File { .. }
        )
    };
    let directory_or_absent = |observation: &DurableObservation| {
        matches!(
            observation,
            DurableObservation::Absent | DurableObservation::Directory { .. }
        )
    };
    let valid = match effect {
        OperationEffect::Write(payload) => match payload.state() {
            WriteState::Planned | WriteState::StageIntent { .. } => {
                absent(slots.custody()) && absent(slots.stage()) && absent(slots.discard())
            }
            WriteState::Staged { stage } | WriteState::CaptureIntent { stage } => {
                slots.stage() == stage && absent(slots.custody()) && absent(slots.discard())
            }
            WriteState::Captured { stage, custody }
            | WriteState::PublishIntent { stage, custody } => {
                slots.stage() == stage && slots.custody() == custody && absent(slots.discard())
            }
            WriteState::Applied { custody, .. } => {
                (slots.custody() == custody || absent(slots.custody()))
                    && absent(slots.stage())
                    && absent(slots.discard())
            }
            WriteState::DiscardIntent { custody, .. }
            | WriteState::PostimageDiscarded { custody, .. } => {
                absent(custody)
                    && absent(slots.custody())
                    && absent(slots.stage())
                    && absent(slots.discard())
            }
            WriteState::RestoreIntent { .. } => {
                matches!(slots.custody(), DurableObservation::File { .. })
                    && absent(slots.stage())
                    && absent(slots.discard())
            }
            WriteState::Preserved => {
                file_or_absent(slots.custody())
                    && file_or_absent(slots.stage())
                    && absent(slots.discard())
            }
        },
        OperationEffect::Delete(payload) => match payload.state() {
            DeleteState::Captured { custody } => {
                slots.custody() == custody && absent(slots.stage()) && absent(slots.discard())
            }
            DeleteState::Applied { custody } => {
                (slots.custody() == custody || absent(slots.custody()))
                    && absent(slots.stage())
                    && absent(slots.discard())
            }
            DeleteState::RestoreIntent { preimage } => {
                matches!(preimage, DurableObservation::File { .. })
                    && slots.custody() == preimage
                    && absent(slots.stage())
                    && absent(slots.discard())
            }
            DeleteState::Planned | DeleteState::CaptureIntent | DeleteState::Preserved => {
                absent(slots.custody()) && absent(slots.stage()) && absent(slots.discard())
            }
        },
        OperationEffect::Verify(_)
        | OperationEffect::Relocate(_)
        | OperationEffect::PostCommitRemoveDirectory(_) => {
            absent(slots.custody()) && absent(slots.stage()) && absent(slots.discard())
        }
        OperationEffect::CreateDirectory(payload) => match payload.state() {
            CreateDirectoryState::Staged { stage }
            | CreateDirectoryState::PublishIntent { stage } => {
                slots.stage() == stage && absent(slots.custody()) && absent(slots.discard())
            }
            CreateDirectoryState::Preserved => {
                directory_or_absent(slots.stage())
                    && absent(slots.custody())
                    && absent(slots.discard())
            }
            CreateDirectoryState::Planned
            | CreateDirectoryState::StageIntent
            | CreateDirectoryState::Applied { .. }
            | CreateDirectoryState::DiscardIntent { .. }
            | CreateDirectoryState::PostimageDiscarded { .. } => {
                absent(slots.custody()) && absent(slots.stage()) && absent(slots.discard())
            }
        },
    };
    if valid {
        Ok(())
    } else {
        Err(OptiScalerJournalError::Invalid(
            "operation artifact slots do not match its action state",
        ))
    }
}
