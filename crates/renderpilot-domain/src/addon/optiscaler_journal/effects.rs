/// State for a parent-directory creation in the ordered program.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum CreateDirectoryState {
    /// Directory creation is planned but not yet staged.
    Planned,
    /// Intent to create the directory in the private staging slot before execution.
    StageIntent,
    /// Directory successfully created and observed in the staging slot.
    Staged {
        /// Observation of the staged directory.
        stage: DurableObservation,
    },
    /// Intent to publish the staged directory to its live destination path.
    PublishIntent {
        /// Observation of the staged directory awaiting publication.
        stage: DurableObservation,
    },
    /// Directory successfully published to the live path.
    Applied {
        /// Observation of the live directory.
        live: DurableObservation,
    },
    /// Intent to move the created directory into the discard slot during rollback.
    DiscardIntent {
        /// Observation of the live directory prior to discard.
        directory: DurableObservation,
    },
    /// Directory moved into the discard slot during rollback.
    PostimageDiscarded {
        /// Observation of the discarded directory.
        discard: DurableObservation,
    },
    /// Terminal rollback state preserving historical directory observations.
    Preserved,
}

/// State for post-commit removal of an empty private directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum RemoveDirectoryState {
    /// The directory is still present and its exact identity is retained as
    /// historical evidence.  No private artifact slot is used for this
    /// post-commit cleanup action.
    Planned {
        /// Retained directory observation before removal.
        directory: DurableObservation,
    },
    /// An empty-directory removal has been announced.  The live observation
    /// is either the retained directory or Absent when the syscall already
    /// completed before the CAS.
    RemoveIntent {
        /// Retained directory observation before removal.
        directory: DurableObservation,
        /// Current live observation of the directory endpoint.
        live: DurableObservation,
    },
    /// The exact historical directory is retained while the live endpoint is
    /// durably absent.
    Applied {
        /// Retained historical directory observation.
        directory: DurableObservation,
    },
}

/// Payload for a write action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WriteEffect {
    endpoint: OperationEndpoint,
    state: WriteState,
}

impl WriteEffect {
    /// Constructs and validates a write payload.
    pub fn new(
        endpoint: OperationEndpoint,
        state: WriteState,
    ) -> Result<Self, OptiScalerJournalError> {
        endpoint.validate()?;
        let effect = Self { endpoint, state };
        validate_write_effect(&effect)?;
        Ok(effect)
    }

    /// Returns the write endpoint.
    #[must_use]
    pub fn endpoint(&self) -> &OperationEndpoint {
        &self.endpoint
    }

    /// Returns mutable endpoint access for legal expected-postimage CAS.
    pub fn endpoint_mut(&mut self) -> &mut OperationEndpoint {
        &mut self.endpoint
    }

    /// Returns the durable write state.
    #[must_use]
    pub fn state(&self) -> &WriteState {
        &self.state
    }

    /// Returns mutable state access for one legal action transition.
    pub fn state_mut(&mut self) -> &mut WriteState {
        &mut self.state
    }
}

/// Payload for a delete action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeleteEffect {
    endpoint: OperationEndpoint,
    state: DeleteState,
}

impl DeleteEffect {
    /// Constructs and validates a delete payload.
    pub fn new(
        endpoint: OperationEndpoint,
        state: DeleteState,
    ) -> Result<Self, OptiScalerJournalError> {
        endpoint.validate()?;
        let effect = Self { endpoint, state };
        validate_delete_effect(&effect)?;
        Ok(effect)
    }

    /// Returns the delete endpoint.
    #[must_use]
    pub fn endpoint(&self) -> &OperationEndpoint {
        &self.endpoint
    }

    /// Returns mutable endpoint access.
    pub fn endpoint_mut(&mut self) -> &mut OperationEndpoint {
        &mut self.endpoint
    }

    /// Returns the durable delete state.
    #[must_use]
    pub fn state(&self) -> &DeleteState {
        &self.state
    }

    /// Returns mutable state access.
    pub fn state_mut(&mut self) -> &mut DeleteState {
        &mut self.state
    }
}

/// Payload for a verify action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerifyEffect {
    endpoint: OperationEndpoint,
    state: VerifyState,
}

impl VerifyEffect {
    /// Constructs and validates a verify payload.
    pub fn new(
        endpoint: OperationEndpoint,
        state: VerifyState,
    ) -> Result<Self, OptiScalerJournalError> {
        endpoint.validate()?;
        let effect = Self { endpoint, state };
        validate_verify_effect(&effect)?;
        Ok(effect)
    }

    /// Returns the verify endpoint.
    #[must_use]
    pub fn endpoint(&self) -> &OperationEndpoint {
        &self.endpoint
    }

    /// Returns mutable endpoint access.
    pub fn endpoint_mut(&mut self) -> &mut OperationEndpoint {
        &mut self.endpoint
    }

    /// Returns the durable verify state.
    #[must_use]
    pub fn state(&self) -> &VerifyState {
        &self.state
    }

    /// Returns mutable state access.
    pub fn state_mut(&mut self) -> &mut VerifyState {
        &mut self.state
    }
}

/// Payload for a relocation action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelocateEffect {
    source: OperationEndpoint,
    destination: OperationEndpoint,
    state: RelocateState,
}

impl RelocateEffect {
    /// Constructs and validates a relocation payload.
    pub fn new(
        source: OperationEndpoint,
        destination: OperationEndpoint,
        state: RelocateState,
    ) -> Result<Self, OptiScalerJournalError> {
        if source.endpoint() != Endpoint::Source || destination.endpoint() != Endpoint::Destination
        {
            return Err(OptiScalerJournalError::Invalid(
                "relocation endpoints must be source and destination",
            ));
        }
        if normalized_path_key(source.path()) == normalized_path_key(destination.path()) {
            return Err(OptiScalerJournalError::Invalid(
                "one operation must not touch one normalized path twice",
            ));
        }
        source.validate()?;
        destination.validate()?;
        let effect = Self {
            source,
            destination,
            state,
        };
        validate_relocate_effect(&effect)?;
        Ok(effect)
    }

    /// Returns the source endpoint.
    #[must_use]
    pub fn source(&self) -> &OperationEndpoint {
        &self.source
    }

    /// Returns the destination endpoint.
    #[must_use]
    pub fn destination(&self) -> &OperationEndpoint {
        &self.destination
    }

    /// Returns mutable source endpoint access.
    pub fn source_mut(&mut self) -> &mut OperationEndpoint {
        &mut self.source
    }

    /// Returns mutable destination endpoint access.
    pub fn destination_mut(&mut self) -> &mut OperationEndpoint {
        &mut self.destination
    }

    /// Returns the durable relocation state.
    #[must_use]
    pub fn state(&self) -> &RelocateState {
        &self.state
    }

    /// Returns mutable state access.
    pub fn state_mut(&mut self) -> &mut RelocateState {
        &mut self.state
    }
}

/// Payload for a parent-directory creation action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateDirectoryEffect {
    endpoint: OperationEndpoint,
    state: CreateDirectoryState,
}

impl CreateDirectoryEffect {
    /// Constructs and validates a directory-creation payload.
    pub fn new(
        endpoint: OperationEndpoint,
        state: CreateDirectoryState,
    ) -> Result<Self, OptiScalerJournalError> {
        endpoint.validate()?;
        let effect = Self { endpoint, state };
        validate_create_directory_effect(&effect)?;
        Ok(effect)
    }

    /// Returns the directory endpoint.
    #[must_use]
    pub fn endpoint(&self) -> &OperationEndpoint {
        &self.endpoint
    }

    /// Returns mutable endpoint access.
    pub fn endpoint_mut(&mut self) -> &mut OperationEndpoint {
        &mut self.endpoint
    }

    /// Returns durable directory state.
    #[must_use]
    pub fn state(&self) -> &CreateDirectoryState {
        &self.state
    }

    /// Returns mutable state access.
    pub fn state_mut(&mut self) -> &mut CreateDirectoryState {
        &mut self.state
    }
}

/// Payload for a post-commit empty-directory removal action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemoveDirectoryEffect {
    endpoint: OperationEndpoint,
    state: RemoveDirectoryState,
}

impl RemoveDirectoryEffect {
    /// Constructs and validates a post-commit directory-removal payload.
    pub fn new(
        endpoint: OperationEndpoint,
        state: RemoveDirectoryState,
    ) -> Result<Self, OptiScalerJournalError> {
        endpoint.validate()?;
        let effect = Self { endpoint, state };
        validate_remove_directory_effect(&effect)?;
        Ok(effect)
    }

    /// Returns the directory endpoint.
    #[must_use]
    pub fn endpoint(&self) -> &OperationEndpoint {
        &self.endpoint
    }

    /// Returns mutable endpoint access.
    pub fn endpoint_mut(&mut self) -> &mut OperationEndpoint {
        &mut self.endpoint
    }

    /// Returns durable cleanup state.
    #[must_use]
    pub fn state(&self) -> &RemoveDirectoryState {
        &self.state
    }

    /// Returns mutable state access.
    pub fn state_mut(&mut self) -> &mut RemoveDirectoryState {
        &mut self.state
    }
}

/// One action in the ordered mutation program.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "action",
    content = "payload",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum OperationEffect {
    /// Atomic file write through private staging and custody slots.
    Write(WriteEffect),
    /// Safe file deletion with prior capture into the custody slot for rollback.
    Delete(DeleteEffect),
    /// Read-only pre-state or post-state verification of an endpoint.
    Verify(VerifyEffect),
    /// Identity-preserving relocation of a file between endpoints.
    Relocate(Box<RelocateEffect>),
    /// Creation of a parent directory in the ordered program.
    CreateDirectory(CreateDirectoryEffect),
    /// Post-commit removal of an empty private directory.
    PostCommitRemoveDirectory(RemoveDirectoryEffect),
}

impl OperationEffect {
    /// Returns whether this action needs a private workspace for durable
    /// staging, custody, or discard artifacts.
    #[must_use]
    pub const fn requires_private_workspace(&self) -> bool {
        matches!(
            self,
            Self::Write(_) | Self::Delete(_) | Self::CreateDirectory(_)
        )
    }

    /// Returns the action's endpoint records.
    #[must_use]
    pub fn endpoints(&self) -> Vec<&OperationEndpoint> {
        match self {
            Self::Write(effect) => vec![&effect.endpoint],
            Self::Delete(effect) => vec![&effect.endpoint],
            Self::Verify(effect) => vec![&effect.endpoint],
            Self::Relocate(effect) => vec![&effect.source, &effect.destination],
            Self::CreateDirectory(effect) => vec![&effect.endpoint],
            Self::PostCommitRemoveDirectory(effect) => vec![&effect.endpoint],
        }
    }

    /// Returns whether this action is terminal for the pre-commit cursor.
    #[must_use]
    pub fn is_prepared_terminal(&self) -> bool {
        match self {
            Self::Write(effect) => {
                matches!(
                    effect.state,
                    WriteState::Applied { .. } | WriteState::Preserved
                )
            }
            Self::Delete(effect) => {
                matches!(
                    effect.state,
                    DeleteState::Applied { .. } | DeleteState::Preserved
                )
            }
            Self::Verify(effect) => {
                matches!(
                    effect.state,
                    VerifyState::Applied { .. } | VerifyState::Preserved
                )
            }
            Self::Relocate(effect) => matches!(
                effect.state,
                RelocateState::Applied { .. } | RelocateState::Preserved
            ),
            Self::CreateDirectory(effect) => matches!(
                effect.state,
                CreateDirectoryState::Applied { .. } | CreateDirectoryState::Preserved
            ),
            Self::PostCommitRemoveDirectory(_) => false,
        }
    }

    /// Returns whether this effect belongs to the pre-commit action program.
    #[must_use]
    pub fn is_precommit(&self) -> bool {
        !matches!(self, Self::PostCommitRemoveDirectory(_))
    }

    /// Returns whether this effect has reached its applied terminal state.
    #[must_use]
    pub fn is_applied(&self) -> bool {
        match self {
            Self::Write(effect) => matches!(effect.state, WriteState::Applied { .. }),
            Self::Delete(effect) => matches!(effect.state, DeleteState::Applied { .. }),
            Self::Verify(effect) => matches!(effect.state, VerifyState::Applied { .. }),
            Self::Relocate(effect) => matches!(effect.state, RelocateState::Applied { .. }),
            Self::CreateDirectory(effect) => {
                matches!(effect.state, CreateDirectoryState::Applied { .. })
            }
            Self::PostCommitRemoveDirectory(_) => false,
        }
    }

    /// Returns whether this effect has reached its rollback-preserved state.
    #[must_use]
    pub fn is_preserved(&self) -> bool {
        match self {
            Self::Write(effect) => matches!(effect.state, WriteState::Preserved),
            Self::Delete(effect) => matches!(effect.state, DeleteState::Preserved),
            Self::Verify(effect) => matches!(effect.state, VerifyState::Preserved),
            Self::Relocate(effect) => matches!(effect.state, RelocateState::Preserved),
            Self::CreateDirectory(effect) => {
                matches!(effect.state, CreateDirectoryState::Preserved)
            }
            Self::PostCommitRemoveDirectory(_) => false,
        }
    }

    /// Returns whether this effect is on the rollback side of its graph.
    #[must_use]
    pub fn is_reverse_or_preserved(&self) -> bool {
        match self {
            Self::Write(effect) => matches!(
                effect.state,
                WriteState::RestoreIntent { .. }
                    | WriteState::PostimageDiscarded { .. }
                    | WriteState::Preserved
            ),
            Self::Delete(effect) => matches!(
                effect.state,
                DeleteState::RestoreIntent { .. } | DeleteState::Preserved
            ),
            Self::Relocate(effect) => matches!(
                effect.state,
                RelocateState::ReverseIntent | RelocateState::Preserved
            ),
            Self::Verify(effect) => matches!(effect.state, VerifyState::Preserved),
            Self::CreateDirectory(effect) => matches!(
                effect.state,
                CreateDirectoryState::DiscardIntent { .. }
                    | CreateDirectoryState::PostimageDiscarded { .. }
                    | CreateDirectoryState::Preserved
            ),
            Self::PostCommitRemoveDirectory(_) => false,
        }
    }

    /// Returns whether this post-commit cleanup effect is terminal.
    #[must_use]
    pub fn is_postcommit_terminal(&self) -> bool {
        matches!(
            self,
            Self::PostCommitRemoveDirectory(RemoveDirectoryEffect {
                state: RemoveDirectoryState::Applied { .. },
                ..
            })
        )
    }

    /// Returns whether this effect is still at its planned state.
    #[must_use]
    pub fn is_planned(&self) -> bool {
        match self {
            Self::Write(effect) => matches!(effect.state, WriteState::Planned),
            Self::Delete(effect) => matches!(effect.state, DeleteState::Planned),
            Self::Verify(effect) => matches!(effect.state, VerifyState::Planned),
            Self::Relocate(effect) => matches!(effect.state, RelocateState::Planned),
            Self::CreateDirectory(effect) => matches!(effect.state, CreateDirectoryState::Planned),
            Self::PostCommitRemoveDirectory(effect) => {
                matches!(effect.state, RemoveDirectoryState::Planned { .. })
            }
        }
    }

    /// Validates endpoint and state observations.
    pub fn validate(&self) -> Result<(), OptiScalerJournalError> {
        let mut paths = HashSet::new();
        for endpoint in self.endpoints() {
            endpoint.validate()?;
            if !paths.insert(normalized_path_key(endpoint.path())) {
                return Err(OptiScalerJournalError::Invalid(
                    "one operation must not touch one normalized path twice",
                ));
            }
        }
        validate_state(self)
    }
}
