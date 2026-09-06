/// One immutable ordered operation record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OperationRecord {
    operation_id: u32,
    parent_dependencies: Vec<u32>,
    workspace_id: Option<u32>,
    slots: PrivateArtifactSlots,
    effect: OperationEffect,
}

impl<'de> Deserialize<'de> for OperationRecord {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            operation_id: u32,
            parent_dependencies: Vec<u32>,
            workspace_id: Option<u32>,
            slots: PrivateArtifactSlots,
            effect: OperationEffect,
        }

        let wire = Wire::deserialize(deserializer)?;
        Self::new(
            wire.operation_id,
            wire.parent_dependencies,
            wire.workspace_id,
            wire.slots,
            wire.effect,
        )
        .map_err(serde::de::Error::custom)
    }
}

impl OperationRecord {
    /// Constructs an operation record and checks local invariants.
    pub fn new(
        operation_id: u32,
        parent_dependencies: Vec<u32>,
        workspace_id: Option<u32>,
        slots: PrivateArtifactSlots,
        effect: OperationEffect,
    ) -> Result<Self, OptiScalerJournalError> {
        let operation = Self {
            operation_id,
            parent_dependencies,
            workspace_id,
            slots,
            effect,
        };
        operation.validate()?;
        Ok(operation)
    }

    /// Returns the stable ordered operation ordinal.
    #[must_use]
    pub const fn operation_id(&self) -> u32 {
        self.operation_id
    }

    /// Returns explicit parent-directory dependencies.
    #[must_use]
    pub fn parent_dependencies(&self) -> &[u32] {
        &self.parent_dependencies
    }

    /// Returns the workspace selected for this operation's private artifacts.
    #[must_use]
    pub const fn workspace_id(&self) -> Option<u32> {
        self.workspace_id
    }

    /// Returns current private artifact slot occupation.
    #[must_use]
    pub fn slots(&self) -> &PrivateArtifactSlots {
        &self.slots
    }

    /// Returns mutable slot access for a legal CAS transition.
    pub fn slots_mut(&mut self) -> &mut PrivateArtifactSlots {
        &mut self.slots
    }

    /// Returns the action effect.
    #[must_use]
    pub fn effect(&self) -> &OperationEffect {
        &self.effect
    }

    /// Returns a mutable action effect for a legal CAS transition.
    pub fn effect_mut(&mut self) -> &mut OperationEffect {
        &mut self.effect
    }

    /// Validates local operation invariants.
    pub fn validate(&self) -> Result<(), OptiScalerJournalError> {
        if self
            .parent_dependencies
            .windows(2)
            .any(|pair| pair[0] >= pair[1])
        {
            return Err(OptiScalerJournalError::Invalid(
                "parent dependencies must be strictly sorted",
            ));
        }
        if self
            .parent_dependencies
            .iter()
            .any(|dependency| *dependency >= self.operation_id)
        {
            return Err(OptiScalerJournalError::Invalid(
                "parent dependency must precede its operation",
            ));
        }
        self.slots.validate()?;
        self.effect.validate()?;
        if self.effect.requires_private_workspace() != self.workspace_id.is_some() {
            return Err(OptiScalerJournalError::Invalid(
                "operation workspace assignment does not match its artifact requirement",
            ));
        }
        validate_action_slots(&self.effect, &self.slots)
    }
}
