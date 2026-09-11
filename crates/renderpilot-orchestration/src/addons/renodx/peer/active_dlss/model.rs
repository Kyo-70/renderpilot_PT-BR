use renderpilot_domain::{
    GameProxyTopology, InstalledAddon, PathRef, PlannedGameProxyTopology, RenoDxDlssProjection,
    RenoDxReshadeIniAuthority, RenoDxReshadeIniFeature,
};

use crate::addons::renodx::peer::RenoDxRootSeal;
use crate::addons::shared_vulkan_mutation::FileIntent;
use crate::peer_mutation_executor::{ExactEndpointProgram, PeerPathSnapshot};

/// The sealed effect requested for one DLSS-Fix endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ActiveDlssEffect {
    /// The endpoint must not be changed.
    Unchanged,
    /// Replace or create the endpoint with these exact bytes.
    Write(Vec<u8>),
    /// Remove the endpoint when it is present and owned by the projection.
    Remove,
}

/// One endpoint path, immutable preimage, and prepared effect.
#[derive(Debug)]
pub(crate) struct ActiveDlssEndpointInput<'a> {
    pub(crate) path: PathRef,
    pub(crate) snapshot: &'a PeerPathSnapshot,
    pub(crate) effect: ActiveDlssEffect,
}

impl<'a> ActiveDlssEndpointInput<'a> {
    pub(crate) fn new(
        path: PathRef,
        snapshot: &'a PeerPathSnapshot,
        effect: ActiveDlssEffect,
    ) -> Self {
        Self {
            path,
            snapshot,
            effect,
        }
    }
}

/// Complete immutable input to the filesystem-free active DLSS composer.
#[derive(Debug)]
pub(crate) struct ActiveDlssInput<'a> {
    pub(crate) before_peer: &'a InstalledAddon,
    pub(crate) after_peer: &'a InstalledAddon,
    pub(crate) topology: &'a GameProxyTopology,
    pub(crate) root: &'a RenoDxRootSeal,
    pub(crate) companion: ActiveDlssEndpointInput<'a>,
    pub(crate) ini: Option<ActiveDlssEndpointInput<'a>>,
    pub(crate) ini_feature: Option<RenoDxReshadeIniFeature>,
}

/// Pure result of one active DLSS transition.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ActiveDlssComposition {
    /// Neither physical bytes nor the DLSS claim projection changed.
    Noop,
    /// The exact companion claim changed without a physical endpoint write.
    ClaimOnly {
        after_peer: InstalledAddon,
        projection: RenoDxDlssProjection,
        planned_topology: PlannedGameProxyTopology,
    },
    /// One exact physical program and its corresponding catalog projection.
    Physical {
        after_peer: InstalledAddon,
        projection: RenoDxDlssProjection,
        program: ExactEndpointProgram,
        payloads: Vec<Option<Vec<u8>>>,
        game_intents: Vec<FileIntent>,
        planned_topology: PlannedGameProxyTopology,
        reshade_ini_authority: Option<RenoDxReshadeIniAuthority>,
    },
}
