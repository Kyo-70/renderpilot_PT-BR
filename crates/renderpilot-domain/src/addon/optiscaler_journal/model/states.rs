/// Durable state of namespace materialization before a mutation is prepared.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum MaterializationState {
    /// Namespace materialization is planned but not yet started.
    Planned,
    /// Durable intent to create the private control root directory.
    ControlCreateIntent,
    /// Private workspace directory creation is in progress.
    Workspaces {
        /// Cursor indicating the next workspace ID to materialize.
        next_workspace_id: u32,
    },
    /// Durable intent to create a specific private workspace directory.
    WorkspaceCreateIntent {
        /// Workspace ID whose directory is being created.
        workspace_id: u32,
    },
    /// All control and private workspace directories are materialized and verified.
    Ready,
}

impl MaterializationState {
    /// Validates identities and operation cursors independent of operation count.
    pub fn validate(&self) -> Result<(), OptiScalerJournalError> {
        match self {
            Self::WorkspaceCreateIntent { .. }
            | Self::Workspaces { .. }
            | Self::Planned
            | Self::ControlCreateIntent
            | Self::Ready => {}
        }
        Ok(())
    }
}

/// Threat model selected before any native filesystem operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ThreatModel {
    /// All participants are expected to be controlled by the same user.
    CooperativeSameUid,
    /// The host may contain same-UID interference; OptiScaler refuses this
    /// mutation because its private namespace cannot establish authority.
    HostileSameUid,
}

/// Durable intent-first private-artifact and namespace cleanup cursor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum CleanupState {
    /// Cleanup has not been started.
    Inactive,
    /// Durable intent to remove an artifact from a private operation slot.
    ArtifactRemoveIntent {
        /// Operation ID owning the private artifact slot.
        operation_id: u32,
        /// Target private slot (custody, stage, or discard).
        artifact: ArtifactSlot,
        /// Expected exact observation of the artifact prior to removal.
        expected: DurableObservation,
    },
    /// Durable intent to remove an empty private workspace directory.
    WorkspaceRemoveIntent {
        /// Workspace ID whose directory is being removed.
        workspace_id: u32,
        /// Expected native filesystem identity of the workspace directory.
        expected_identity: String,
    },
    /// Durable intent to remove the private control root directory.
    ControlRemoveIntent {
        /// Expected native filesystem identity of the control directory.
        expected_identity: String,
    },
    /// All cleanup operations have completed successfully.
    Complete,
}

impl CleanupState {
    /// Validates exact cleanup observations and identities.
    pub fn validate(&self) -> Result<(), OptiScalerJournalError> {
        match self {
            Self::ArtifactRemoveIntent { expected, .. } => {
                expected.validate()?;
                if !expected.is_exact() || matches!(expected, DurableObservation::Absent) {
                    return Err(OptiScalerJournalError::Invalid(
                        "artifact cleanup requires a present exact observation",
                    ));
                }
            }
            Self::WorkspaceRemoveIntent {
                expected_identity, ..
            }
            | Self::ControlRemoveIntent { expected_identity } => {
                validate_nonempty("cleanup identity", expected_identity)?;
            }
            Self::Inactive | Self::Complete => {}
        }
        Ok(())
    }
}

/// Private artifact slot named by a cleanup cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ArtifactSlot {
    /// Custody slot holding the pre-mutation file copy for safe rollback.
    Custody,
    /// Staging slot holding new content before atomic publication.
    Stage,
    /// Discard slot holding superseded content awaiting post-commit cleanup.
    Discard,
}

/// State for an exact write action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum WriteState {
    /// Write action is planned but not yet staged.
    Planned,
    /// Intent to stage target content into the private staging slot.
    StageIntent {
        /// Expected cryptographic hash of the content to stage.
        target_digest: Sha256Hash,
    },
    /// Content successfully written and verified in the staging slot.
    Staged {
        /// Observation of the staged file.
        stage: DurableObservation,
    },
    /// Intent to capture the pre-existing live file into the custody slot.
    CaptureIntent {
        /// Staged file observation carried forward.
        stage: DurableObservation,
    },
    /// Pre-existing live file successfully captured into custody.
    Captured {
        /// Staged file observation awaiting publication.
        stage: DurableObservation,
        /// Captured pre-existing file observation in custody.
        custody: DurableObservation,
    },
    /// Intent to publish the staged file to the live destination path.
    PublishIntent {
        /// Staged file observation awaiting rename.
        stage: DurableObservation,
        /// Custody file observation retained for rollback safety.
        custody: DurableObservation,
    },
    /// Content successfully applied to the live endpoint.
    Applied {
        /// Observation of the live file.
        live: DurableObservation,
        /// Retained custody observation for post-commit cleanup or rollback.
        custody: DurableObservation,
    },
    /// Intent to move the live file into the discard slot during rollback.
    DiscardIntent {
        /// Observation of the live file prior to discard.
        postimage: DurableObservation,
        /// Custody file observation retained for restoration.
        custody: DurableObservation,
    },
    /// Postimage successfully moved to the discard slot during rollback.
    PostimageDiscarded {
        /// Observation of the discarded postimage.
        discard: DurableObservation,
        /// Custody file observation ready for restore.
        custody: DurableObservation,
    },
    /// Intent to restore the original file from the custody slot.
    RestoreIntent {
        /// Preimage observation being restored to the live path.
        preimage: DurableObservation,
        /// Discard slot observation retained until restore completes.
        discard: DurableObservation,
    },
    /// Terminal rollback state with historical evidence preserved.
    Preserved,
}

/// State for an exact destructive delete action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum DeleteState {
    /// Deletion is planned but not yet executed.
    Planned,
    /// Intent to capture the target file into custody before deleting from live.
    CaptureIntent,
    /// File successfully captured into custody.
    Captured {
        /// Observation of the captured file in the custody slot.
        custody: DurableObservation,
    },
    /// File removed from live path, retained in custody for rollback safety.
    Applied {
        /// Observation of the custody file held until commit.
        custody: DurableObservation,
    },
    /// Intent to restore the file from custody during rollback.
    RestoreIntent {
        /// Preimage observation being restored to the live path.
        preimage: DurableObservation,
    },
    /// Terminal rollback state with historical evidence preserved.
    Preserved,
}

/// State for an identity-preserving no-replace relocation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum RelocateState {
    /// Relocation is planned between endpoints.
    Planned,
    /// Intent to move the file from source to destination.
    MoveIntent,
    /// File successfully moved to destination; source is now absent.
    Applied {
        /// Observation of the source endpoint after move (expected absent).
        source_after: DurableObservation,
        /// Observation of the destination endpoint after move.
        destination_after: DurableObservation,
    },
    /// Intent to reverse the relocation during rollback.
    ReverseIntent,
    /// Terminal rollback state with historical evidence preserved.
    Preserved,
}

/// State for a read-only exact verification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum VerifyState {
    /// Verification is planned.
    Planned,
    /// Verification completed with the recorded endpoint observation.
    Applied {
        /// Exact observation obtained during verification.
        observed: DurableObservation,
    },
    /// Terminal rollback state with historical evidence preserved.
    Preserved,
}
