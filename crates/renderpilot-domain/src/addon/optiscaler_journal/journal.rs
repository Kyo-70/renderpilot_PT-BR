/// The only kind currently emitted by this contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OptiScalerJournalKind {
    /// Canonical OptiScaler mutation journal format.
    #[serde(rename = "optiscaler")]
    OptiScaler,
}

/// Top-level canonical OptiScaler journal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OptiScalerJournal {
    kind: OptiScalerJournalKind,
    threat_model: ThreatModel,
    materialization: MaterializationState,
    cleanup: CleanupState,
    roots: Vec<String>,
    control_namespace: ControlNamespaceBinding,
    private_workspaces: Vec<PrivateWorkspaceBinding>,
    operations: Vec<OperationRecord>,
}

impl<'de> Deserialize<'de> for OptiScalerJournal {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            kind: OptiScalerJournalKind,
            threat_model: ThreatModel,
            materialization: MaterializationState,
            cleanup: CleanupState,
            roots: Vec<String>,
            control_namespace: ControlNamespaceBinding,
            private_workspaces: Vec<PrivateWorkspaceBinding>,
            operations: Vec<OperationRecord>,
        }

        let wire = Wire::deserialize(deserializer)?;
        let journal = Self {
            kind: wire.kind,
            threat_model: wire.threat_model,
            materialization: wire.materialization,
            cleanup: wire.cleanup,
            roots: wire.roots,
            control_namespace: wire.control_namespace,
            private_workspaces: wire.private_workspaces,
            operations: wire.operations,
        };
        journal.validate().map_err(serde::de::Error::custom)?;
        Ok(journal)
    }
}

impl OptiScalerJournal {
    /// Constructs an unversioned canonical OptiScaler journal.
    pub fn new(
        roots: Vec<String>,
        control_namespace: ControlNamespaceBinding,
        private_workspaces: Vec<PrivateWorkspaceBinding>,
        operations: Vec<OperationRecord>,
    ) -> Result<Self, OptiScalerJournalError> {
        Self::new_with_threat_model(
            ThreatModel::CooperativeSameUid,
            roots,
            control_namespace,
            private_workspaces,
            operations,
        )
    }

    /// Constructs a journal with an explicit pre-native threat model.
    pub fn new_with_threat_model(
        threat_model: ThreatModel,
        roots: Vec<String>,
        control_namespace: ControlNamespaceBinding,
        private_workspaces: Vec<PrivateWorkspaceBinding>,
        operations: Vec<OperationRecord>,
    ) -> Result<Self, OptiScalerJournalError> {
        let journal = Self {
            kind: OptiScalerJournalKind::OptiScaler,
            threat_model,
            materialization: MaterializationState::Planned,
            cleanup: CleanupState::Inactive,
            roots,
            control_namespace,
            private_workspaces,
            operations,
        };
        journal.validate()?;
        Ok(journal)
    }

    /// Returns the fixed journal kind.
    #[must_use]
    pub const fn kind(&self) -> OptiScalerJournalKind {
        self.kind
    }

    /// Returns the threat model selected before native mutation.
    #[must_use]
    pub const fn threat_model(&self) -> ThreatModel {
        self.threat_model
    }

    /// Returns namespace materialization state.
    #[must_use]
    pub fn materialization(&self) -> &MaterializationState {
        &self.materialization
    }

    /// Changes the namespace state after the storage CAS layer has checked its
    /// transition table.
    pub fn set_materialization(&mut self, state: MaterializationState) {
        self.materialization = state;
    }

    /// Returns the intent-first cleanup cursor.
    #[must_use]
    pub fn cleanup(&self) -> &CleanupState {
        &self.cleanup
    }

    /// Changes the cleanup cursor after the storage CAS layer has checked it.
    pub fn set_cleanup(&mut self, state: CleanupState) {
        self.cleanup = state;
    }

    /// Returns protected root paths as domain-neutral text.
    #[must_use]
    pub fn roots(&self) -> &[String] {
        &self.roots
    }

    /// Returns the comparison-only control namespace binding.
    #[must_use]
    pub fn control_namespace(&self) -> &ControlNamespaceBinding {
        &self.control_namespace
    }

    /// Returns mutable control binding access for a durable native identity.
    pub fn control_namespace_mut(&mut self) -> &mut ControlNamespaceBinding {
        &mut self.control_namespace
    }

    /// Returns the ordered private workspace program.
    #[must_use]
    pub fn private_workspaces(&self) -> &[PrivateWorkspaceBinding] {
        &self.private_workspaces
    }

    /// Returns mutable workspace access for a single identity-recording CAS.
    pub fn private_workspaces_mut(&mut self) -> &mut [PrivateWorkspaceBinding] {
        &mut self.private_workspaces
    }

    /// Returns the complete ordered operation program.
    #[must_use]
    pub fn operations(&self) -> &[OperationRecord] {
        &self.operations
    }

    /// Returns mutable access for a single CAS transition.
    pub fn operations_mut(&mut self) -> &mut [OperationRecord] {
        &mut self.operations
    }

    /// Returns whether the journal is at the finish-preparing boundary.
    ///
    /// Only completed pre-commit actions may be committed; post-commit
    /// cleanup remains untouched and the cleanup cursor must be inactive.
    #[must_use]
    pub fn is_finish_boundary_ready(&self) -> bool {
        self.operations.iter().all(|operation| {
            if operation.effect().is_precommit() {
                operation.effect().is_applied()
            } else {
                matches!(
                    operation.effect(),
                    OperationEffect::PostCommitRemoveDirectory(RemoveDirectoryEffect {
                        state: RemoveDirectoryState::Planned { .. },
                        ..
                    })
                )
            }
        }) && matches!(self.cleanup, CleanupState::Inactive)
    }

    /// Returns whether a reverse action or preserved operation has latched
    /// the journal into rollback-only mode.
    #[must_use]
    pub fn is_rollback_latched(&self) -> bool {
        self.operations
            .iter()
            .filter(|operation| operation.effect().is_precommit())
            .any(|operation| operation.effect().is_reverse_or_preserved())
    }

    /// Returns the greatest non-preserved pre-commit ordinal to reverse.
    #[must_use]
    pub fn rollback_cursor(&self) -> Option<u32> {
        self.operations
            .iter()
            .filter(|operation| operation.effect().is_precommit())
            .filter(|operation| !operation.effect().is_preserved())
            .map(OperationRecord::operation_id)
            .max()
    }

    /// Returns whether rollback cleanup may delete the journal and namespace.
    #[must_use]
    pub fn can_delete_after_rollback(&self) -> bool {
        self.operations.iter().all(|operation| {
            if operation.effect().is_precommit() {
                operation.effect().is_preserved()
            } else {
                matches!(
                    operation.effect(),
                    OperationEffect::PostCommitRemoveDirectory(RemoveDirectoryEffect {
                        state: RemoveDirectoryState::Planned { .. },
                        ..
                    })
                )
            }
        }) && self.slots_are_empty()
            && matches!(self.cleanup, CleanupState::Complete)
    }

    /// Returns whether rollback artifact and namespace cleanup may advance.
    /// Every pre-commit effect must be preserved, every post-commit directory
    /// effect must remain planned, and workspace identities must be ready.
    #[must_use]
    pub fn is_rollback_cleanup_ready(&self) -> bool {
        self.materialization == MaterializationState::Ready
            && self.operations.iter().all(|operation| {
                if operation.effect().is_precommit() {
                    operation.effect().is_preserved()
                } else {
                    operation.effect().is_planned()
                }
            })
    }

    /// Returns whether committed cleanup may delete the journal and namespace.
    #[must_use]
    pub fn can_delete_after_commit(&self) -> bool {
        self.operations.iter().all(|operation| {
            if operation.effect().is_precommit() {
                operation.effect().is_applied()
            } else {
                operation.effect().is_postcommit_terminal()
            }
        }) && self.slots_are_empty()
            && matches!(self.cleanup, CleanupState::Complete)
    }

    fn slots_are_empty(&self) -> bool {
        self.operations.iter().all(|operation| {
            [
                &operation.slots().custody(),
                &operation.slots().stage(),
                &operation.slots().discard(),
            ]
            .into_iter()
            .all(|observation| matches!(observation, DurableObservation::Absent))
        })
    }

    /// Validates the complete ordered program and all domain invariants.
    pub fn validate(&self) -> Result<(), OptiScalerJournalError> {
        if self.kind != OptiScalerJournalKind::OptiScaler {
            return Err(OptiScalerJournalError::Invalid("unknown journal kind"));
        }
        if self.roots.is_empty() || self.roots.iter().any(|root| root.trim().is_empty()) {
            return Err(OptiScalerJournalError::Invalid(
                "journal roots must be non-empty",
            ));
        }
        let mut roots = HashSet::new();
        for root in &self.roots {
            if !roots.insert(normalized_component_key(root)) {
                return Err(OptiScalerJournalError::Invalid(
                    "journal roots must be normalized-unique",
                ));
            }
        }
        self.materialization.validate()?;
        self.cleanup.validate()?;
        self.control_namespace.validate()?;
        if !matches!(self.cleanup, CleanupState::Inactive)
            && !matches!(self.materialization, MaterializationState::Ready)
        {
            return Err(OptiScalerJournalError::Invalid(
                "cleanup requires a fully materialized workspace frontier",
            ));
        }
        if self.operations.is_empty() {
            return Err(OptiScalerJournalError::Invalid(
                "journal operation program must not be empty",
            ));
        }
        for (index, operation) in self.operations.iter().enumerate() {
            let expected = u32::try_from(index)
                .map_err(|_| OptiScalerJournalError::Invalid("operation program is too large"))?;
            if operation.operation_id != expected {
                return Err(OptiScalerJournalError::Invalid(
                    "operation ids must be contiguous and ordered",
                ));
            }
            operation.validate()?;
            if let Some(workspace_id) = operation.workspace_id() {
                let workspace_index = usize::try_from(workspace_id).map_err(|_| {
                    OptiScalerJournalError::Invalid("operation workspace id overflows usize")
                })?;
                if self.private_workspaces.get(workspace_index).is_none() {
                    return Err(OptiScalerJournalError::Invalid(
                        "operation references a missing private workspace",
                    ));
                }
            }
        }
        validate_operation_program(&self.operations)?;
        validate_parent_dependencies(&self.operations)?;
        validate_materialization_frontier(self)?;
        validate_namespace_bindings(self)?;
        validate_cleanup_cursor(self)?;
        validate_rollback_latch(&self.operations)?;
        Ok(())
    }
}

/// Domain-level validation failures for the OptiScaler journal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptiScalerJournalError {
    /// A required text or identity field was empty.
    Empty(&'static str),
    /// A structural contract invariant was violated.
    Invalid(&'static str),
}

impl fmt::Display for OptiScalerJournalError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty(field) => write!(formatter, "{field} must not be empty"),
            Self::Invalid(message) => formatter.write_str(message),
        }
    }
}

impl Error for OptiScalerJournalError {}

fn validate_nonempty(field: &'static str, value: &str) -> Result<(), OptiScalerJournalError> {
    if value.trim().is_empty() {
        Err(OptiScalerJournalError::Empty(field))
    } else {
        Ok(())
    }
}

fn into_nonempty(field: &'static str, value: String) -> Result<String, OptiScalerJournalError> {
    validate_nonempty(field, &value)?;
    Ok(value)
}

fn deserialize_nonempty_identity<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let identity = String::deserialize(deserializer)?;
    if identity.trim().is_empty() {
        return Err(serde::de::Error::custom("identity cannot be empty"));
    }
    Ok(identity)
}
