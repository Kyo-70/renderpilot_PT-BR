/// A validated observation that can safely cross the orchestration boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DurableObservation {
    /// The endpoint or artifact slot is absent.
    Absent,
    /// A regular file with opaque native identity and exact content digest.
    File {
        /// Opaque native filesystem identity for identity-preserving checks.
        identity: String,
        /// Exact cryptographic digest of the file contents.
        digest: Sha256Hash,
    },
    /// A directory with opaque native identity.
    Directory {
        /// Opaque native filesystem identity of the directory.
        identity: String,
    },
    /// An object exists but is not a regular file or directory.
    NonRegular,
    /// The authority could not obtain an exact observation.
    Unreadable,
}

impl<'de> Deserialize<'de> for DurableObservation {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
        enum Wire {
            Absent,
            File {
                #[serde(deserialize_with = "deserialize_nonempty_identity")]
                identity: String,
                digest: Sha256Hash,
            },
            Directory {
                #[serde(deserialize_with = "deserialize_nonempty_identity")]
                identity: String,
            },
            NonRegular,
            Unreadable,
        }

        match Wire::deserialize(deserializer)? {
            Wire::Absent => Ok(Self::Absent),
            Wire::File { identity, digest } => Ok(Self::File { identity, digest }),
            Wire::Directory { identity } => Ok(Self::Directory { identity }),
            Wire::NonRegular => Ok(Self::NonRegular),
            Wire::Unreadable => Ok(Self::Unreadable),
        }
    }
}

impl DurableObservation {
    /// Creates an exact file observation.
    pub fn file(
        identity: impl Into<String>,
        digest: Sha256Hash,
    ) -> Result<Self, OptiScalerJournalError> {
        let identity = into_nonempty("file observation identity", identity.into())?;
        Ok(Self::File { identity, digest })
    }

    /// Creates an exact directory observation.
    pub fn directory(identity: impl Into<String>) -> Result<Self, OptiScalerJournalError> {
        Ok(Self::Directory {
            identity: into_nonempty("directory observation identity", identity.into())?,
        })
    }

    /// Returns the opaque identity for an exact file or directory.
    #[must_use]
    pub fn identity(&self) -> Option<&str> {
        match self {
            Self::File { identity, .. } | Self::Directory { identity } => Some(identity),
            Self::Absent | Self::NonRegular | Self::Unreadable => None,
        }
    }

    /// Returns the file digest for an exact file observation.
    #[must_use]
    pub fn digest(&self) -> Option<&Sha256Hash> {
        match self {
            Self::File { digest, .. } => Some(digest),
            Self::Absent | Self::Directory { .. } | Self::NonRegular | Self::Unreadable => None,
        }
    }

    /// Returns whether this is an exact, usable observation.
    #[must_use]
    pub fn is_exact(&self) -> bool {
        matches!(
            self,
            Self::Absent | Self::File { .. } | Self::Directory { .. }
        )
    }

    /// Validates invariants independently of serde.
    pub fn validate(&self) -> Result<(), OptiScalerJournalError> {
        if let Self::File { identity, .. } | Self::Directory { identity } = self {
            validate_nonempty("observation identity", identity)?;
        }
        Ok(())
    }
}
