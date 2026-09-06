//! Immutable domain types for peer read-guard requirements and evidence.

use super::super::model::PeerFileImage;
use crate::{PathRef, Sha256Hash};

/// Domain source that requires a live read guard before a peer mutation.
///
/// The enum is intentionally closed: a caller cannot invent an unclassified
/// guard source or silently broaden a guard's authority at an adapter edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PeerReadGuardSource {
    /// The immutable outer receipt at the active topology root.
    TopologyOuter,
    /// An unchanged topology downstream receipt.
    TopologyDownstream,
    /// The sidecar preserving an owned managed-file baseline.
    ManagedOwnedBaseline,
    /// The live bytes accepted by a reused managed-file claim.
    ManagedReusedLive,
    /// The live baseline accepted while a catalog rollback consumes its sidecar.
    CatalogBaselineLive,
    /// The catalog rollback sidecar that is being consumed.
    CatalogBaselineSidecar,
    /// The exact RenoDX DLSS-Fix companion preimage used by a claim-only
    /// transition.
    RenoDxDlssCompanion,
}

/// Exact expectation for a read guard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PeerReadGuardExpectation {
    /// The guarded path must be absent.
    Absent,
    /// Only the guarded bytes' digest is authoritative.
    Digest {
        /// Expected SHA-256 digest.
        sha256: Sha256Hash,
    },
    /// Both stable file identity and digest are authoritative.
    Receipt {
        /// Expected native file identity.
        identity: String,
        /// Expected SHA-256 digest.
        sha256: Sha256Hash,
    },
}

impl PeerReadGuardExpectation {
    /// Returns the expected digest, when the guard requires bytes.
    #[must_use]
    pub fn sha256(&self) -> Option<&Sha256Hash> {
        match self {
            Self::Absent => None,
            Self::Digest { sha256 } | Self::Receipt { sha256, .. } => Some(sha256),
        }
    }

    /// Returns the expected native identity for a receipt guard.
    #[must_use]
    pub fn identity(&self) -> Option<&str> {
        match self {
            Self::Receipt { identity, .. } => Some(identity),
            Self::Absent | Self::Digest { .. } => None,
        }
    }
}

/// One canonical read-guard requirement derived from peer/topology state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerReadGuardRequirement {
    path: PathRef,
    sources: Vec<PeerReadGuardSource>,
    expectation: PeerReadGuardExpectation,
}

impl PeerReadGuardRequirement {
    pub(super) fn new(
        path: PathRef,
        mut sources: Vec<PeerReadGuardSource>,
        expectation: PeerReadGuardExpectation,
    ) -> Self {
        sources.sort_unstable();
        sources.dedup();
        Self {
            path,
            sources,
            expectation,
        }
    }

    /// Returns the normalized guarded path.
    #[must_use]
    pub fn path(&self) -> &PathRef {
        &self.path
    }

    /// Returns canonical source attribution in enum order.
    #[must_use]
    pub fn sources(&self) -> &[PeerReadGuardSource] {
        &self.sources
    }

    /// Returns whether this requirement has a topology-derived source.
    #[must_use]
    pub fn is_topology_sourced(&self) -> bool {
        self.sources.iter().any(|source| {
            matches!(
                source,
                PeerReadGuardSource::TopologyOuter | PeerReadGuardSource::TopologyDownstream
            )
        })
    }

    /// Returns the exact expected live state.
    #[must_use]
    pub fn expectation(&self) -> &PeerReadGuardExpectation {
        &self.expectation
    }
}

/// One process-local observation supplied by the filesystem executor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerReadGuardEvidence {
    path: PathRef,
    observed: Option<PeerFileImage>,
}

impl PeerReadGuardEvidence {
    /// Creates evidence for one guard path. Domain validation is performed by
    /// [`super::super::validate_read_guards`] at the authority boundary.
    #[must_use]
    pub fn new(path: PathRef, observed: Option<PeerFileImage>) -> Self {
        Self { path, observed }
    }

    /// Returns the observed path.
    #[must_use]
    pub fn path(&self) -> &PathRef {
        &self.path
    }

    /// Returns the live image, or `None` when the path was absent.
    #[must_use]
    pub fn observed(&self) -> Option<&PeerFileImage> {
        self.observed.as_ref()
    }
}
