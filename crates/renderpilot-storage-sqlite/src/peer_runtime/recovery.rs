//! Storage-owned, typed projection of a file peer recovery program.
//!
//! The projection is deliberately not a serde model.  It is minted only
//! after the durable manifest and its peer program have been bound together at
//! the storage boundary, so recovery consumers cannot select a different
//! endpoint list or silently fall back to a generic restore path.

use renderpilot_application::{AppError, AppResult};
use renderpilot_domain::{
    PathRef, PeerEndpointOperation, PeerEndpointRole, RenoDxReshadeIniAuthority, Sha256Hash,
};

use super::file_manifest_binding::ValidatedFilePeerManifest;
use super::manifest::ParsedPeerProgram;

/// Durable execution family for a file peer recovery program.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeerRecoveryExecutionClass(ExecutionClass);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExecutionClass {
    Ordinary,
    Retryable,
}

impl PeerRecoveryExecutionClass {
    pub(super) const fn ordinary() -> Self {
        Self(ExecutionClass::Ordinary)
    }

    pub(super) const fn retryable() -> Self {
        Self(ExecutionClass::Retryable)
    }

    /// Returns whether this is the ordinary transaction class.
    #[must_use]
    pub const fn is_ordinary(self) -> bool {
        matches!(self.0, ExecutionClass::Ordinary)
    }

    /// Returns whether this is the retryable transaction class.
    #[must_use]
    pub const fn is_retryable(self) -> bool {
        matches!(self.0, ExecutionClass::Retryable)
    }
}

/// Exact image needed to classify one durable endpoint during recovery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerRecoveryImage {
    state: PeerRecoveryImageState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PeerRecoveryImageState {
    Absent,
    File {
        /// Native identity is retained only for a durable before image.
        /// Planned after images intentionally have no identity assertion:
        /// publication may create a new native object.
        identity: Option<String>,
        sha256: Sha256Hash,
        length: u64,
    },
}

impl PeerRecoveryImage {
    pub(super) fn from_before(image: Option<&renderpilot_domain::PeerFileImage>) -> Self {
        match image {
            Some(image) => Self {
                state: PeerRecoveryImageState::File {
                    identity: Some(image.identity().to_owned()),
                    sha256: image.sha256().clone(),
                    length: image.length(),
                },
            },
            None => Self {
                state: PeerRecoveryImageState::Absent,
            },
        }
    }

    pub(super) fn try_from_after(
        intent: &renderpilot_domain::PeerEndpointIntent,
    ) -> AppResult<Self> {
        match intent.operation() {
            PeerEndpointOperation::Remove => Ok(Self {
                state: PeerRecoveryImageState::Absent,
            }),
            PeerEndpointOperation::Create | PeerEndpointOperation::Replace => {
                let sha256 = intent.planned_sha256().cloned().ok_or_else(|| {
                    AppError::storage_failed(
                        "validated peer recovery endpoint is missing its planned digest",
                    )
                })?;
                let length = intent.planned_length().ok_or_else(|| {
                    AppError::storage_failed(
                        "validated peer recovery endpoint is missing its planned length",
                    )
                })?;
                Ok(Self {
                    state: PeerRecoveryImageState::File {
                        identity: None,
                        sha256,
                        length,
                    },
                })
            }
        }
    }

    /// Returns whether the endpoint must be absent.
    #[must_use]
    pub const fn is_absent(&self) -> bool {
        matches!(&self.state, PeerRecoveryImageState::Absent)
    }

    /// Returns whether the endpoint must be a regular file.
    #[must_use]
    pub const fn is_file(&self) -> bool {
        matches!(&self.state, PeerRecoveryImageState::File { .. })
    }

    /// Returns the native identity when this image is a durable before image.
    /// Planned after images return `None` because their native identity is
    /// established only by the filesystem publication.
    #[must_use]
    pub fn identity(&self) -> Option<&str> {
        match &self.state {
            PeerRecoveryImageState::Absent => None,
            PeerRecoveryImageState::File { identity, .. } => identity.as_deref(),
        }
    }

    /// Returns the exact post/preimage digest for a regular-file image.
    #[must_use]
    pub fn sha256(&self) -> Option<&Sha256Hash> {
        match &self.state {
            PeerRecoveryImageState::Absent => None,
            PeerRecoveryImageState::File { sha256, .. } => Some(sha256),
        }
    }

    /// Returns the exact byte length for a regular-file image.
    #[must_use]
    pub fn length(&self) -> Option<u64> {
        match &self.state {
            PeerRecoveryImageState::Absent => None,
            PeerRecoveryImageState::File { length, .. } => Some(*length),
        }
    }
}

/// One immutable physical endpoint in a storage-issued recovery program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerRecoveryEndpoint {
    ordinal: usize,
    path: PathRef,
    role: PeerEndpointRole,
    operation: PeerEndpointOperation,
    before: PeerRecoveryImage,
    after: PeerRecoveryImage,
    snapshot_path: Option<String>,
}

impl PeerRecoveryEndpoint {
    pub(super) fn from_validated(
        ordinal: usize,
        intent: &renderpilot_domain::PeerEndpointIntent,
        before: &PeerRecoveryImage,
        after: &PeerRecoveryImage,
        snapshot_path: Option<String>,
    ) -> Self {
        Self {
            ordinal,
            path: intent.path().clone(),
            role: intent.role(),
            operation: intent.operation(),
            before: before.clone(),
            after: after.clone(),
            snapshot_path,
        }
    }

    /// Returns the contiguous endpoint ordinal bound by storage.
    #[must_use]
    pub const fn ordinal(&self) -> usize {
        self.ordinal
    }

    /// Returns the exact endpoint path.
    #[must_use]
    pub fn path(&self) -> &PathRef {
        &self.path
    }

    /// Returns the sealed physical endpoint role.
    #[must_use]
    pub const fn role(&self) -> PeerEndpointRole {
        self.role
    }

    /// Returns the physical operation selected by the sealed peer program.
    #[must_use]
    pub const fn operation(&self) -> PeerEndpointOperation {
        self.operation
    }

    /// Returns the exact captured preimage.
    #[must_use]
    pub fn before(&self) -> &PeerRecoveryImage {
        &self.before
    }

    /// Returns the exact planned postimage.
    #[must_use]
    pub fn after(&self) -> &PeerRecoveryImage {
        &self.after
    }

    /// Returns the durable before-snapshot path, when this endpoint had one.
    #[must_use]
    pub fn snapshot_path(&self) -> Option<&str> {
        self.snapshot_path.as_deref()
    }
}

/// One exact directory ancestor retained for peer recovery cleanup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerRecoveryAncestor {
    path: String,
    consumer_ordinals: Vec<usize>,
}

impl PeerRecoveryAncestor {
    pub(super) fn from_validated(path: String, consumer_ordinals: Vec<usize>) -> Self {
        Self {
            path,
            consumer_ordinals,
        }
    }

    /// Returns the exact ancestor path retained by the durable manifest.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Returns the endpoint ordinals that caused this ancestor to be created.
    #[must_use]
    pub fn consumer_ordinals(&self) -> &[usize] {
        &self.consumer_ordinals
    }
}

/// Closed storage-issued recovery projection for one file peer mutation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerRecoveryProgram {
    execution_class: PeerRecoveryExecutionClass,
    roots: Vec<String>,
    transaction_dir: String,
    endpoints: Vec<PeerRecoveryEndpoint>,
    declared_ancestors: Vec<PeerRecoveryAncestor>,
    renodx_reshade_ini: Option<RenoDxReshadeIniAuthority>,
}

impl PeerRecoveryProgram {
    pub(super) fn from_validated(
        execution_class: PeerRecoveryExecutionClass,
        roots: Vec<String>,
        transaction_dir: String,
        program: &ParsedPeerProgram,
        binding: ValidatedFilePeerManifest,
        renodx_reshade_ini: Option<RenoDxReshadeIniAuthority>,
    ) -> AppResult<Self> {
        if binding.snapshot_paths.len() != program.intents().len() {
            return Err(AppError::storage_failed(
                "peer recovery snapshot projection cardinality differs from endpoints",
            ));
        }
        let mut endpoints = Vec::with_capacity(program.intents().len());
        for (ordinal, intent) in program.intents().iter().enumerate() {
            let before = PeerRecoveryImage::from_before(program.before()[ordinal].as_ref());
            let after = PeerRecoveryImage::try_from_after(intent)?;
            let snapshot_path = binding.snapshot_paths[ordinal].clone();
            endpoints.push(PeerRecoveryEndpoint::from_validated(
                ordinal,
                intent,
                &before,
                &after,
                snapshot_path,
            ));
        }

        Ok(Self {
            execution_class,
            roots,
            transaction_dir,
            endpoints,
            declared_ancestors: binding.ancestors,
            renodx_reshade_ini,
        })
    }

    /// Returns the sealed execution family.
    #[must_use]
    pub const fn execution_class(&self) -> PeerRecoveryExecutionClass {
        self.execution_class
    }

    /// Returns the exact authorized roots.
    #[must_use]
    pub fn roots(&self) -> &[String] {
        &self.roots
    }

    /// Returns the durable transaction directory.
    #[must_use]
    pub fn transaction_dir(&self) -> &str {
        &self.transaction_dir
    }

    /// Returns endpoints in their storage-sealed ordinal order.
    #[must_use]
    pub fn endpoints(&self) -> &[PeerRecoveryEndpoint] {
        &self.endpoints
    }

    /// Returns the exact declared ancestor projection.
    #[must_use]
    pub fn declared_ancestors(&self) -> &[PeerRecoveryAncestor] {
        &self.declared_ancestors
    }

    /// Returns the storage-derived RenoDX ReShade.ini authority, when present.
    #[must_use]
    pub fn renodx_reshade_ini_authority(&self) -> Option<&RenoDxReshadeIniAuthority> {
        self.renodx_reshade_ini.as_ref()
    }
}
