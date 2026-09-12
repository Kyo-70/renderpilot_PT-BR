#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PlannedPreimage {
    Exact {
        current: FileReceipt,
        prior_owned: Option<FileReceipt>,
    },
    ExactReused {
        current: FileReceipt,
        authority: ReusedMutationAuthority,
    },
    Absent,
    Verify,
}

/// Closed reasons for mutating an exact Reused participant. Storage derives
/// the same permission from the aggregate role, so this value cannot widen
/// durable authority by itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReusedMutationAuthority {
    /// Observe a Reused participant without changing it. This is the only
    /// authority for a user configuration which remains unmodified.
    ObservationOnly,
    /// Acquire an existing OptiScaler configuration through a bounded merge.
    ConfigurationWrite,
    /// Replace or remove a manifest-recognized OptiScaler artifact.
    OptiScalerArtifact,
    /// Move an exact downstream peer without claiming its ownership.
    RelocationSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PlannedParticipant {
    pub(crate) path: PathBuf,
    pub(crate) preimage: PlannedPreimage,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OptiScalerPlannedOperation {
    Write(PlannedParticipant),
    Delete(PlannedParticipant),
    Verify(PlannedParticipant),
    CreateDirectory(PlannedParticipant),
    Relocate {
        source: PlannedParticipant,
        destination: PlannedParticipant,
    },
    PostCommitRemoveDirectory(renderpilot_domain::OptiScalerDirectoryReceipt),
}

pub(crate) struct OptiScalerMutation<'a> {
    pub(crate) context: &'a Context,
    pub(crate) guard: &'a GameMutationGuard,
    pub(crate) scope: &'a MutationScope,
    pub(crate) feature: &'a str,
    pub(crate) subject_id: Option<&'a str>,
    pub(crate) operations: Vec<OptiScalerPlannedOperation>,
    /// Persisted OptiScaler release endpoints that may be absent below an
    /// existing managed root during repair/removal.  This is transient
    /// planning authority, never a journal protocol field.
    pub(crate) managed_endpoint_roots: Vec<(PathBuf, PathBuf)>,
    pub(crate) threat_model: ThreatModel,
}
