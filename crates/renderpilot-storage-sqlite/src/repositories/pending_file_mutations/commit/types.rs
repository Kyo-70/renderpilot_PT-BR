use super::prelude::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::repositories) enum OptiScalerBoundTransition {
    ClaimedFile {
        installed: FileReceipt,
    },
    ReplaceClaimedFile {
        before: FileReceipt,
        after: FileReceipt,
    },
    AcquireReusedFile {
        prior: FileReceipt,
        installed: FileReceipt,
        mode: ReusedAcquisitionMode,
        configuration_baseline: Option<FileReceipt>,
    },
    RecreateOwnedFile {
        prior: FileReceipt,
        installed: FileReceipt,
    },
    ClaimedDirectory {
        identity: String,
    },
    RemoveOwnedFile {
        installed: FileReceipt,
        restoration: Option<FileReceipt>,
        allow_absent: bool,
    },
    RemoveReusedFile {
        installed: FileReceipt,
        restoration: Option<FileReceipt>,
        allow_absent: bool,
    },
    /// Restore the exact user configuration baseline while relinquishing an
    /// OptiScaler-owned file during aggregate uninstall.
    ///
    /// This is deliberately distinct from `RemoveOwnedFile`: the live entry
    /// remains the same native object and the terminal action is a typed
    /// in-place write from the installed digest to the retained Reused
    /// baseline digest.
    RestoreOwnedFile {
        installed: FileReceipt,
        restoration: FileReceipt,
    },
    VerifyOwnedFile {
        prior: FileReceipt,
        current: FileReceipt,
    },
    RelinquishReusedFile {
        installed: FileReceipt,
    },
    /// Observes the user-editable adopted configuration without claiming its
    /// current identity or content. The persisted receipt proves adoption;
    /// the journal separately seals the one live observation.
    ObserveReusedConfiguration {
        persisted: FileReceipt,
    },
    Relocate {
        source: FileReceipt,
        destination: FileReceipt,
    },
    /// A peer-owned managed baseline relocation, bound to the exact baseline
    /// digest recorded by the peer transition.
    RelocatePeerBaseline {
        digest: Sha256Hash,
    },
    /// A peer relocation vacates this path before OptiScaler claims it in a
    /// later operation of the same journal.
    RelocateThenClaim {
        source: FileReceipt,
        destination: FileReceipt,
        installed: FileReceipt,
    },
    PostCommitRemoveDirectory {
        identity: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::repositories) enum ReusedAcquisitionMode {
    /// An exact unchanged OptiScaler artifact may be acquired, or a missing
    /// artifact may be recreated.
    ExactOrAbsent,
    /// The canonical configuration may be merged from its exact live bytes,
    /// including same-identity user edits, or recreated when missing.
    Configuration,
}

/// One path and its aggregate-level transition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::repositories) struct OptiScalerBoundPath {
    pub(in crate::repositories) path: String,
    pub(in crate::repositories) transition: OptiScalerBoundTransition,
}

/// Typed custody proof assembled by the game mutation boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::repositories) struct OptiScalerAggregateBinding {
    pub(in crate::repositories) paths: BTreeMap<String, OptiScalerBoundPath>,
    pub(in crate::repositories) after_claims: BTreeMap<String, OptiScalerBoundPath>,
    pub(in crate::repositories) known_paths: BTreeSet<String>,
    /// Exact Reused originals which must remain sealed by one live Verify
    /// while OptiScaler updates the active replacement around them.
    pub(in crate::repositories) retained_fsr_custody: BTreeMap<String, FileReceipt>,
    pub(in crate::repositories) auxiliary: Vec<OptiScalerAuxiliaryPreservation>,
    pub(in crate::repositories) owned_preservations: Vec<OptiScalerOwnedPreservation>,
}

/// Typed app-owned copy participating in the aggregate proof.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::repositories) struct OptiScalerAuxiliaryPreservation {
    pub(in crate::repositories) source: String,
    pub(in crate::repositories) destination: String,
    pub(in crate::repositories) receipt: FileReceipt,
}

/// The immutable source side of an app-owned recovery copy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::repositories) struct OptiScalerOwnedPreservation {
    pub(in crate::repositories) source: String,
    pub(in crate::repositories) prior: FileReceipt,
    pub(in crate::repositories) current: FileReceipt,
}
