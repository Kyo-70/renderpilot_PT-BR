use std::path::Path;

use renderpilot_domain::{FileOwnership, GameId, GameProxyTopology, PathRef, Sha256Hash};

/// Filesystem intent for the ReShade host behind an outer proxy.
pub(crate) enum DownstreamInstallPlan<'a> {
    /// Retain an already committed downstream while refreshing the outer
    /// proxy in the same root slot. No source bytes need to be rediscovered.
    Existing { destination_path: &'a Path },
    /// Transfer a verified host into a new downstream slot during initial
    /// installation or topology relocation.
    Transfer {
        source_path: &'a Path,
        expected_source_sha256: &'a Sha256Hash,
        destination_path: &'a Path,
        destination_ownership: FileOwnership,
    },
}

impl DownstreamInstallPlan<'_> {
    pub(super) fn destination_path(&self) -> &Path {
        match self {
            Self::Existing { destination_path }
            | Self::Transfer {
                destination_path, ..
            } => destination_path,
        }
    }
}

/// Complete topology intent consumed by the shared proxy executor.
pub(crate) struct ProxyInstallPlan<'a> {
    pub(crate) game_id: &'a GameId,
    pub(crate) root_slot: &'a Path,
    pub(crate) outer_sha256: Sha256Hash,
    pub(crate) updating: bool,
    pub(crate) downstream: Option<DownstreamInstallPlan<'a>>,
}

/// A peer receipt path transition produced while releasing a topology.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PeerHostRelocation {
    pub(crate) from: PathRef,
    pub(crate) to: PathRef,
}

/// Returns the exact host path transition that a topology removal will apply.
/// This is pure topology projection used by callers to preflight peer receipt
/// ownership before the custody journal is prepared.
pub(crate) fn planned_peer_host_relocation(
    topology: &GameProxyTopology,
) -> Option<PeerHostRelocation> {
    let downstream = topology.downstream.as_ref()?;
    let to = topology.downstream_origin.clone()?;
    Some(PeerHostRelocation {
        from: downstream.path.clone(),
        to,
    })
}
