/// The opaque capability binding one private namespace leaf to its owner.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct NamespaceCapability(String);

impl<'de> Deserialize<'de> for NamespaceCapability {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

impl NamespaceCapability {
    /// Creates a canonical lower-case hexadecimal capability.
    pub fn new(value: impl Into<String>) -> Result<Self, OptiScalerJournalError> {
        let value = into_nonempty("namespace capability", value.into())?;
        if value.len() != 64
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        {
            return Err(OptiScalerJournalError::Invalid(
                "namespace capability must be 64 lower-case hexadecimal characters",
            ));
        }
        Ok(Self(value))
    }

    /// Returns the canonical capability text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for NamespaceCapability {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for NamespaceCapability {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// The comparison-only control namespace binding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ControlNamespaceBinding {
    path: String,
    identity: Option<String>,
    capability: NamespaceCapability,
}

impl<'de> Deserialize<'de> for ControlNamespaceBinding {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            path: String,
            identity: Option<String>,
            capability: NamespaceCapability,
        }
        let wire = Wire::deserialize(deserializer)?;
        Self::from_wire_exact(wire.path, wire.identity, wire.capability)
            .map_err(serde::de::Error::custom)
    }
}

impl ControlNamespaceBinding {
    /// Creates a control binding before the native object is materialized.
    pub fn new(
        path: impl Into<String>,
        identity: Option<String>,
        capability: NamespaceCapability,
    ) -> Result<Self, OptiScalerJournalError> {
        let binding = Self {
            path: canonical_namespace_path("control namespace path", path.into())?,
            identity: identity
                .map(|value| into_nonempty("control namespace identity", value))
                .transpose()?,
            capability,
        };
        binding.validate()?;
        Ok(binding)
    }

    fn from_wire_exact(
        path: String,
        identity: Option<String>,
        capability: NamespaceCapability,
    ) -> Result<Self, OptiScalerJournalError> {
        validate_canonical_namespace_path("control namespace path", &path)?;
        let binding = Self {
            path,
            identity,
            capability,
        };
        binding.validate()?;
        Ok(binding)
    }

    /// Returns the comparison-only control path.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Returns the native control identity, if materialized.
    #[must_use]
    pub fn identity(&self) -> Option<&str> {
        self.identity.as_deref()
    }

    /// Returns the namespace capability.
    #[must_use]
    pub fn capability(&self) -> &NamespaceCapability {
        &self.capability
    }

    /// Records the exact native control identity.
    pub fn set_identity(
        &mut self,
        identity: impl Into<String>,
    ) -> Result<(), OptiScalerJournalError> {
        let identity = into_nonempty("control namespace identity", identity.into())?;
        if self
            .identity
            .as_deref()
            .is_some_and(|current| current != identity)
        {
            return Err(OptiScalerJournalError::Invalid(
                "control namespace identity cannot be replaced",
            ));
        }
        self.identity = Some(identity);
        Ok(())
    }

    /// Validates path, identity, and capability shape.
    pub fn validate(&self) -> Result<(), OptiScalerJournalError> {
        validate_canonical_namespace_path("control namespace path", &self.path)?;
        if let Some(identity) = &self.identity {
            validate_nonempty("control namespace identity", identity)?;
        }
        Ok(())
    }
}

/// One private workspace shared by all artifact-bearing operations in one
/// physical mutation group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PrivateWorkspaceBinding {
    workspace_id: u32,
    root_index: u32,
    path: String,
    identity: Option<String>,
    capability: NamespaceCapability,
}

impl<'de> Deserialize<'de> for PrivateWorkspaceBinding {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            workspace_id: u32,
            root_index: u32,
            path: String,
            identity: Option<String>,
            capability: NamespaceCapability,
        }

        let wire = Wire::deserialize(deserializer)?;
        Self::from_wire_exact(
            wire.workspace_id,
            wire.root_index,
            wire.path,
            wire.identity,
            wire.capability,
        )
        .map_err(serde::de::Error::custom)
    }
}

impl PrivateWorkspaceBinding {
    /// Creates a private workspace binding before its native directory is
    /// materialized.
    pub fn new(
        workspace_id: u32,
        root_index: u32,
        path: impl Into<String>,
        identity: Option<String>,
        capability: NamespaceCapability,
    ) -> Result<Self, OptiScalerJournalError> {
        let binding = Self {
            workspace_id,
            root_index,
            path: canonical_namespace_path("private namespace path", path.into())?,
            identity: identity
                .map(|value| into_nonempty("private workspace identity", value))
                .transpose()?,
            capability,
        };
        binding.validate()?;
        Ok(binding)
    }

    fn from_wire_exact(
        workspace_id: u32,
        root_index: u32,
        path: String,
        identity: Option<String>,
        capability: NamespaceCapability,
    ) -> Result<Self, OptiScalerJournalError> {
        validate_canonical_namespace_path("private namespace path", &path)?;
        let binding = Self {
            workspace_id,
            root_index,
            path,
            identity,
            capability,
        };
        binding.validate()?;
        Ok(binding)
    }

    /// Returns the contiguous workspace ordinal.
    #[must_use]
    pub const fn workspace_id(&self) -> u32 {
        self.workspace_id
    }

    /// Returns the index of the declared root selected for this workspace.
    #[must_use]
    pub const fn root_index(&self) -> u32 {
        self.root_index
    }

    /// Returns the comparison-only workspace path.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Returns the native identity, if materialized.
    #[must_use]
    pub fn identity(&self) -> Option<&str> {
        self.identity.as_deref()
    }

    /// Returns the namespace capability.
    #[must_use]
    pub fn capability(&self) -> &NamespaceCapability {
        &self.capability
    }

    /// Records the exact native private workspace identity.
    pub fn set_identity(
        &mut self,
        identity: impl Into<String>,
    ) -> Result<(), OptiScalerJournalError> {
        let identity = into_nonempty("private workspace identity", identity.into())?;
        if self
            .identity
            .as_deref()
            .is_some_and(|current| current != identity)
        {
            return Err(OptiScalerJournalError::Invalid(
                "private workspace identity cannot be replaced",
            ));
        }
        self.identity = Some(identity);
        Ok(())
    }

    /// Validates immutable workspace fields and optional identity shape.
    pub fn validate(&self) -> Result<(), OptiScalerJournalError> {
        validate_canonical_namespace_path("private namespace path", &self.path)?;
        if let Some(identity) = &self.identity {
            validate_nonempty("private workspace identity", identity)?;
        }
        Ok(())
    }
}
