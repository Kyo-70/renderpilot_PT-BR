use std::path::{Path, PathBuf};

use renderpilot_domain::{GameProxyTopology, InstalledAddon, PathRef, Version};

use crate::addons::luma::use_cases::update_target::ResolvedUpdateTarget;
use crate::peer_mutation_executor::PeerPathSnapshot;

/// Closed local dgVoodoo outcome carried from phase one to later preparation.
///
/// The phase-one route never invents dependency bytes.  It records only the
/// outcome needed by the later phase to choose preserve, replacement, or
/// removal while retaining configuration ownership when replacement is safe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DgVoodooLocalDecision {
    /// The current profile can be left untouched. `config_owned` tells the
    /// later phase whether a managed config may be refreshed if needed.
    Preserve { config_owned: bool },
    /// The current profile is managed and needs a complete local replacement.
    Replace { config_owned: bool },
    /// The managed runtime could not be identified safely. Repair may replace
    /// it after the later full-payload decision; normal Update must preserve it.
    ReplaceOnFull { config_owned: bool },
    /// The old managed dependency is no longer declared by the current profile.
    Remove,
}

/// Complete immutable phase-one input for an active Luma update.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ActiveUpdatePhase1 {
    pub(crate) record: InstalledAddon,
    pub(crate) topology: GameProxyTopology,
    pub(crate) target: ResolvedUpdateTarget,
    /// The install root as persisted by the game record, before target
    /// canonicalization.  It is retained for later exact drift checks.
    pub(crate) stored_game_install_path: PathRef,
    /// Canonical directory containing the resolved rendering executable.
    pub(crate) canonical_game_root: PathBuf,
    pub(crate) downstream_path: PathRef,
    pub(crate) downstream_snapshot: PeerPathSnapshot,
    pub(crate) minimum_reshade_version: Version,
    pub(crate) had_torn_marker: bool,
    pub(crate) payload_disk_intact: bool,
    pub(crate) dependency_paths: Vec<PathBuf>,
    pub(crate) dgvoodoo: DgVoodooLocalDecision,
    pub(crate) host_replacement_required: bool,
}

impl ActiveUpdatePhase1 {
    #[must_use]
    pub(crate) fn record(&self) -> &InstalledAddon {
        &self.record
    }

    #[must_use]
    pub(crate) fn topology(&self) -> &GameProxyTopology {
        &self.topology
    }

    #[must_use]
    pub(crate) fn target(&self) -> &ResolvedUpdateTarget {
        &self.target
    }

    #[must_use]
    #[cfg(test)]
    pub(crate) fn stored_game_install_path(&self) -> &PathRef {
        &self.stored_game_install_path
    }

    #[must_use]
    pub(crate) fn canonical_game_root(&self) -> &Path {
        &self.canonical_game_root
    }

    #[must_use]
    pub(crate) fn downstream_path(&self) -> &PathRef {
        &self.downstream_path
    }

    #[must_use]
    pub(crate) fn downstream_snapshot(&self) -> &PeerPathSnapshot {
        &self.downstream_snapshot
    }

    #[must_use]
    pub(crate) fn minimum_reshade_version(&self) -> &Version {
        &self.minimum_reshade_version
    }

    #[must_use]
    pub(crate) fn had_torn_marker(&self) -> bool {
        self.had_torn_marker
    }

    #[must_use]
    pub(crate) fn payload_disk_intact(&self) -> bool {
        self.payload_disk_intact
    }

    #[must_use]
    pub(crate) fn dependency_paths(&self) -> &[PathBuf] {
        &self.dependency_paths
    }

    #[must_use]
    pub(crate) fn dgvoodoo(&self) -> &DgVoodooLocalDecision {
        &self.dgvoodoo
    }

    #[must_use]
    pub(crate) fn host_replacement_required(&self) -> bool {
        self.host_replacement_required
    }
}
