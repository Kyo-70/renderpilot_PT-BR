use std::path::Path;

use renderpilot_domain::{GameProxyTopology, InstalledAddon, PathRef, PlannedGameProxyTopology};

use crate::addons::shared_vulkan_mutation::FileIntent;
use crate::peer_mutation_executor::{ExactEndpointProgram, PeerPathSnapshot};

/// Exact Set_Path transition lowered from the sealed ReShade.ini source.
pub(super) type RenoDxActiveUpdateConfigParts<'a> = (
    &'a PathRef,
    Option<&'a crate::peer_mutation_executor::VerifiedPeerFile>,
    Option<&'a [u8]>,
    Option<&'a [u8]>,
);

pub(super) type RenoDxActiveUpdateConfigOwnedParts = (
    PathRef,
    Option<crate::peer_mutation_executor::VerifiedPeerFile>,
    Option<Vec<u8>>,
    Option<Vec<u8>>,
);

#[derive(Debug)]
pub(crate) struct RenoDxActiveUpdateConfigInput {
    path: PathRef,
    before: Option<crate::peer_mutation_executor::VerifiedPeerFile>,
    before_bytes: Option<Vec<u8>>,
    after: Option<Vec<u8>>,
}

impl RenoDxActiveUpdateConfigInput {
    pub(crate) fn new(
        path: PathRef,
        before: Option<crate::peer_mutation_executor::VerifiedPeerFile>,
        before_bytes: Option<Vec<u8>>,
        after: Option<Vec<u8>>,
    ) -> Self {
        Self {
            path,
            before,
            before_bytes,
            after,
        }
    }

    pub(super) fn parts(&self) -> RenoDxActiveUpdateConfigParts<'_> {
        (
            &self.path,
            self.before.as_ref(),
            self.before_bytes.as_deref(),
            self.after.as_deref(),
        )
    }

    pub(super) fn into_parts(self) -> RenoDxActiveUpdateConfigOwnedParts {
        (self.path, self.before, self.before_bytes, self.after)
    }
}

/// Prepared bytes and the exact sealed preimage for an owned proxy host.
#[derive(Debug)]
pub(crate) struct RenoDxActiveUpdateHostInput<'a> {
    path: PathRef,
    snapshot: &'a PeerPathSnapshot,
    bytes: Vec<u8>,
}

impl<'a> RenoDxActiveUpdateHostInput<'a> {
    /// Binds prepared host bytes to the retained sealed preimage.
    pub(crate) fn new(path: PathRef, snapshot: &'a PeerPathSnapshot, bytes: Vec<u8>) -> Self {
        Self {
            path,
            snapshot,
            bytes,
        }
    }

    pub(super) fn into_parts(self) -> (PathRef, &'a PeerPathSnapshot, Vec<u8>) {
        (self.path, self.snapshot, self.bytes)
    }
}

/// Complete immutable input to the active RenoDX update composer.
#[derive(Debug)]
pub(crate) struct RenoDxActiveUpdateInput<'a> {
    pub(crate) before_peer: &'a InstalledAddon,
    pub(crate) after_peer: &'a InstalledAddon,
    pub(crate) topology: &'a GameProxyTopology,
    pub(crate) canonical_game_root: &'a Path,
    pub(crate) payload_root: Option<&'a Path>,
    pub(crate) addon_path: PathRef,
    pub(crate) addon_snapshot: &'a PeerPathSnapshot,
    pub(crate) addon_bytes: Vec<u8>,
    pub(crate) host: Option<RenoDxActiveUpdateHostInput<'a>>,
    pub(crate) config: Option<RenoDxActiveUpdateConfigInput>,
}

/// Metadata-only result when no physical endpoint needs replacement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RenoDxActiveUpdateMetadata {
    after_peer: InstalledAddon,
}

impl RenoDxActiveUpdateMetadata {
    pub(super) fn new(after_peer: InstalledAddon) -> Self {
        Self { after_peer }
    }

    pub(crate) fn after_peer(&self) -> &InstalledAddon {
        &self.after_peer
    }
}

/// Physical result consumed by the later peer runtime route.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RenoDxActiveUpdatePhysical {
    after_peer: InstalledAddon,
    program: ExactEndpointProgram,
    payloads: Vec<Option<Vec<u8>>>,
    game_intents: Vec<FileIntent>,
    planned_topology: PlannedGameProxyTopology,
}

impl RenoDxActiveUpdatePhysical {
    pub(super) fn new(
        after_peer: InstalledAddon,
        program: ExactEndpointProgram,
        payloads: Vec<Option<Vec<u8>>>,
        game_intents: Vec<FileIntent>,
        planned_topology: PlannedGameProxyTopology,
    ) -> Self {
        Self {
            after_peer,
            program,
            payloads,
            game_intents,
            planned_topology,
        }
    }

    pub(crate) fn after_peer(&self) -> &InstalledAddon {
        &self.after_peer
    }

    pub(crate) fn program(&self) -> &ExactEndpointProgram {
        &self.program
    }

    pub(crate) fn payloads(&self) -> &[Option<Vec<u8>>] {
        &self.payloads
    }

    pub(crate) fn game_intents(&self) -> &[FileIntent] {
        &self.game_intents
    }

    pub(crate) fn planned_topology(&self) -> &PlannedGameProxyTopology {
        &self.planned_topology
    }
}

/// Result of composing one active RenoDX update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RenoDxActiveUpdateComposition {
    /// No record or physical state changed.
    Noop,
    /// Only record provenance or metadata changed.
    Metadata(Box<RenoDxActiveUpdateMetadata>),
    /// One exact endpoint program must be executed.
    Physical(Box<RenoDxActiveUpdatePhysical>),
}

impl RenoDxActiveUpdateComposition {
    #[cfg(test)]
    pub(crate) fn metadata(&self) -> Option<&RenoDxActiveUpdateMetadata> {
        match self {
            Self::Metadata(metadata) => Some(metadata),
            Self::Noop | Self::Physical(_) => None,
        }
    }

    #[cfg(test)]
    pub(crate) fn physical(&self) -> Option<&RenoDxActiveUpdatePhysical> {
        match self {
            Self::Physical(physical) => Some(physical),
            Self::Noop | Self::Metadata(_) => None,
        }
    }
}
