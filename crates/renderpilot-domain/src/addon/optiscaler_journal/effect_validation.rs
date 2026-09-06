fn exact(
    observation: &DurableObservation,
    label: &'static str,
) -> Result<(), OptiScalerJournalError> {
    observation.validate()?;
    if !observation.is_exact() {
        return Err(OptiScalerJournalError::Invalid(label));
    }
    Ok(())
}

fn exact_file_or_absent(
    observation: &DurableObservation,
    label: &'static str,
) -> Result<(), OptiScalerJournalError> {
    exact(observation, label)?;
    if !matches!(
        observation,
        DurableObservation::Absent | DurableObservation::File { .. }
    ) {
        return Err(OptiScalerJournalError::Invalid(label));
    }
    Ok(())
}

fn exact_file(
    observation: &DurableObservation,
    label: &'static str,
) -> Result<(), OptiScalerJournalError> {
    exact(observation, label)?;
    if !matches!(observation, DurableObservation::File { .. }) {
        return Err(OptiScalerJournalError::Invalid(label));
    }
    Ok(())
}

fn exact_absent(
    observation: &DurableObservation,
    label: &'static str,
) -> Result<(), OptiScalerJournalError> {
    exact(observation, label)?;
    if !matches!(observation, DurableObservation::Absent) {
        return Err(OptiScalerJournalError::Invalid(label));
    }
    Ok(())
}

fn exact_directory(
    observation: &DurableObservation,
    label: &'static str,
) -> Result<(), OptiScalerJournalError> {
    exact(observation, label)?;
    if !matches!(observation, DurableObservation::Directory { .. }) {
        return Err(OptiScalerJournalError::Invalid(label));
    }
    Ok(())
}

fn pending(endpoint: &OperationEndpoint) -> Result<(), OptiScalerJournalError> {
    if !matches!(endpoint.expected_after(), ExpectedAfter::Pending) {
        return Err(OptiScalerJournalError::Invalid(
            "action state has a postimage before durable application",
        ));
    }
    Ok(())
}

fn known(
    endpoint: &OperationEndpoint,
    expected: &DurableObservation,
) -> Result<(), OptiScalerJournalError> {
    exact(expected, "action postimage observation must be exact")?;
    match endpoint.expected_after() {
        ExpectedAfter::Known(actual) if actual == expected => Ok(()),
        ExpectedAfter::Known(_) => Err(OptiScalerJournalError::Invalid(
            "endpoint postimage does not match action state",
        )),
        ExpectedAfter::Pending => Err(OptiScalerJournalError::Invalid(
            "durable action state is missing its exact postimage",
        )),
    }
}

fn pending_or_known_kind(
    endpoint: &OperationEndpoint,
    kind: fn(&DurableObservation) -> bool,
    label: &'static str,
) -> Result<(), OptiScalerJournalError> {
    match endpoint.expected_after() {
        ExpectedAfter::Pending => Ok(()),
        ExpectedAfter::Known(observation) => {
            exact(observation, "endpoint postimage must be exact")?;
            if !kind(observation) {
                return Err(OptiScalerJournalError::Invalid(label));
            }
            Ok(())
        }
    }
}

fn validate_write_effect(payload: &WriteEffect) -> Result<(), OptiScalerJournalError> {
    match &payload.state {
        WriteState::Planned | WriteState::StageIntent { .. } => pending(&payload.endpoint)?,
        WriteState::Staged { stage } | WriteState::CaptureIntent { stage } => {
            exact_file(stage, "write staging observation must be an exact file")?;
            pending(&payload.endpoint)?;
        }
        WriteState::PublishIntent { stage, custody } => {
            exact_file(stage, "write staging observation must be an exact file")?;
            exact_file_or_absent(custody, "write custody observation is uncertain")?;
            pending(&payload.endpoint)?;
        }
        WriteState::Captured { stage, custody } => {
            exact_file(stage, "write staging observation must be an exact file")?;
            exact_file_or_absent(
                custody,
                "write custody observation must be an exact file or absent",
            )?;
            pending(&payload.endpoint)?;
        }
        WriteState::Applied { live, custody } => {
            exact_file(live, "write postimage must be an exact file")?;
            exact_file_or_absent(custody, "write custody observation is uncertain")?;
            known(&payload.endpoint, live)?;
        }
        WriteState::DiscardIntent { postimage, custody } => {
            exact_file(postimage, "write postimage must be an exact file")?;
            exact_absent(custody, "discarding write custody must be absent")?;
            known(&payload.endpoint, postimage)?;
        }
        WriteState::PostimageDiscarded { discard, custody } => {
            exact_file(discard, "discarded write postimage must be an exact file")?;
            exact_absent(custody, "discarded write custody must be absent")?;
            known(&payload.endpoint, discard)?;
        }
        WriteState::RestoreIntent { preimage, discard } => {
            exact_file_or_absent(preimage, "write restore preimage is uncertain")?;
            exact_file(discard, "write restore postimage must be an exact file")?;
            known(&payload.endpoint, discard)?;
        }
        WriteState::Preserved => {
            pending_or_known_kind(
                &payload.endpoint,
                |observation| matches!(observation, DurableObservation::File { .. }),
                "preserved write postimage must be an exact file",
            )?;
        }
    }
    Ok(())
}

fn validate_delete_effect(payload: &DeleteEffect) -> Result<(), OptiScalerJournalError> {
    match &payload.state {
        DeleteState::Planned | DeleteState::CaptureIntent => pending(&payload.endpoint)?,
        DeleteState::Captured { custody } => {
            exact_file(custody, "delete custody observation must be an exact file")?;
            pending(&payload.endpoint)?;
        }
        DeleteState::Applied { custody } => {
            exact_file(custody, "delete custody observation must be an exact file")?;
            known(&payload.endpoint, &DurableObservation::Absent)?;
        }
        DeleteState::RestoreIntent { preimage } => {
            exact_file(preimage, "delete restore preimage must be an exact file")?;
            known(&payload.endpoint, &DurableObservation::Absent)?;
        }
        DeleteState::Preserved => match payload.endpoint.expected_after() {
            ExpectedAfter::Pending => {}
            ExpectedAfter::Known(observation) => {
                exact(observation, "preserved delete postimage must be exact")?;
                if !matches!(observation, DurableObservation::Absent) {
                    return Err(OptiScalerJournalError::Invalid(
                        "preserved delete postimage must be absent",
                    ));
                }
            }
        },
    }
    Ok(())
}

fn validate_verify_effect(payload: &VerifyEffect) -> Result<(), OptiScalerJournalError> {
    match &payload.state {
        VerifyState::Planned => pending(&payload.endpoint)?,
        VerifyState::Applied { observed } => {
            exact(observed, "verify observation must be exact")?;
            known(&payload.endpoint, observed)?;
        }
        VerifyState::Preserved => {
            pending_or_known_kind(
                &payload.endpoint,
                DurableObservation::is_exact,
                "preserved verify postimage is uncertain",
            )?;
        }
    }
    Ok(())
}

fn validate_relocate_effect(payload: &RelocateEffect) -> Result<(), OptiScalerJournalError> {
    match &payload.state {
        RelocateState::Planned | RelocateState::MoveIntent => {
            pending(&payload.source)?;
            pending(&payload.destination)?;
        }
        RelocateState::Applied {
            source_after,
            destination_after,
        } => {
            exact(source_after, "relocation source postimage must be exact")?;
            exact_file(
                destination_after,
                "relocation destination must be an exact file",
            )?;
            if !matches!(source_after, DurableObservation::Absent) {
                return Err(OptiScalerJournalError::Invalid(
                    "relocation source postimage must be absent",
                ));
            }
            known(&payload.source, source_after)?;
            known(&payload.destination, destination_after)?;
        }
        RelocateState::ReverseIntent | RelocateState::Preserved => {
            match (
                payload.source.expected_after(),
                payload.destination.expected_after(),
            ) {
                (ExpectedAfter::Pending, ExpectedAfter::Pending) => {}
                (ExpectedAfter::Known(source), ExpectedAfter::Known(destination)) => {
                    exact(source, "relocation source postimage must be exact")?;
                    exact_file(destination, "relocation destination must be an exact file")?;
                    if !matches!(source, DurableObservation::Absent) {
                        return Err(OptiScalerJournalError::Invalid(
                            "relocation source postimage must be absent",
                        ));
                    }
                }
                _ => {
                    return Err(OptiScalerJournalError::Invalid(
                        "relocation preserved endpoints must be both pending or both known",
                    ));
                }
            }
        }
    }
    Ok(())
}

fn validate_create_directory_effect(
    payload: &CreateDirectoryEffect,
) -> Result<(), OptiScalerJournalError> {
    match &payload.state {
        CreateDirectoryState::Planned | CreateDirectoryState::StageIntent => {
            pending(&payload.endpoint)?;
        }
        CreateDirectoryState::Staged { stage } | CreateDirectoryState::PublishIntent { stage } => {
            exact_directory(stage, "directory staging observation must be exact")?;
            pending(&payload.endpoint)?;
        }
        CreateDirectoryState::Applied { live } => {
            exact_directory(live, "created directory postimage must be exact")?;
            known(&payload.endpoint, live)?;
        }
        CreateDirectoryState::DiscardIntent { directory } => {
            exact_directory(directory, "directory postimage must be exact")?;
            known(&payload.endpoint, directory)?;
        }
        CreateDirectoryState::PostimageDiscarded { discard } => {
            exact_directory(discard, "discarded directory postimage must be exact")?;
            known(&payload.endpoint, discard)?;
        }
        CreateDirectoryState::Preserved => {
            pending_or_known_kind(
                &payload.endpoint,
                |observation| matches!(observation, DurableObservation::Directory { .. }),
                "preserved directory postimage must be a directory",
            )?;
        }
    }
    Ok(())
}

fn validate_remove_directory_effect(
    payload: &RemoveDirectoryEffect,
) -> Result<(), OptiScalerJournalError> {
    match &payload.state {
        RemoveDirectoryState::Planned { directory } => {
            exact_directory(directory, "directory cleanup preimage must be exact")?;
            pending(&payload.endpoint)?;
        }
        RemoveDirectoryState::RemoveIntent { directory, live } => {
            exact_directory(directory, "directory cleanup preimage must be exact")?;
            exact(live, "directory cleanup live observation must be exact")?;
            if !matches!(
                live,
                DurableObservation::Absent | DurableObservation::Directory { .. }
            ) {
                return Err(OptiScalerJournalError::Invalid(
                    "directory cleanup live observation must be a directory or absent",
                ));
            }
            if matches!(live, DurableObservation::Directory { .. }) && live != directory {
                return Err(OptiScalerJournalError::Invalid(
                    "directory cleanup live identity changed unexpectedly",
                ));
            }
            if !matches!(payload.endpoint.expected_after(), ExpectedAfter::Pending) {
                return Err(OptiScalerJournalError::Invalid(
                    "directory cleanup intent must not publish its endpoint postimage",
                ));
            }
        }
        RemoveDirectoryState::Applied { directory } => {
            exact_directory(directory, "directory cleanup preimage must be exact")?;
            known(&payload.endpoint, &DurableObservation::Absent)?;
        }
    }
    Ok(())
}

fn validate_state(effect: &OperationEffect) -> Result<(), OptiScalerJournalError> {
    match effect {
        OperationEffect::Write(payload) => validate_write_effect(payload),
        OperationEffect::Delete(payload) => validate_delete_effect(payload),
        OperationEffect::Verify(payload) => validate_verify_effect(payload),
        OperationEffect::Relocate(payload) => validate_relocate_effect(payload),
        OperationEffect::CreateDirectory(payload) => validate_create_directory_effect(payload),
        OperationEffect::PostCommitRemoveDirectory(payload) => {
            validate_remove_directory_effect(payload)
        }
    }
}
