use renderpilot_domain::{
    GameProxyTopology, InstalledAddon, PathRef, PlannedGameProxyTopology, RenoDxReshadeIniAuthority,
};

use crate::addons::renodx::peer::RenoDxRootSeal;
use crate::addons::shared_vulkan_mutation::FileIntent;
use crate::peer_mutation_executor::{ExactEndpointProgram, PeerPathSnapshot};

/// One immutable endpoint observation retained by the active uninstall route.
/// A backup is retained beside the live preimage so a restore can be lowered
/// without rediscovering a sidecar during or after the transaction.
#[derive(Debug)]
pub(crate) struct ActiveUninstallEndpointOwned {
    path: PathRef,
    snapshot: PeerPathSnapshot,
    backup: Option<ActiveUninstallBackupOwned>,
}

#[derive(Debug)]
pub(crate) struct ActiveUninstallBackupOwned {
    path: PathRef,
    snapshot: PeerPathSnapshot,
}

impl ActiveUninstallEndpointOwned {
    pub(crate) fn new(
        path: PathRef,
        snapshot: PeerPathSnapshot,
        backup: Option<ActiveUninstallBackupOwned>,
    ) -> Self {
        Self {
            path,
            snapshot,
            backup,
        }
    }

    pub(crate) fn path(&self) -> &PathRef {
        &self.path
    }

    pub(crate) fn snapshot(&self) -> &PeerPathSnapshot {
        &self.snapshot
    }

    pub(crate) fn backup(&self) -> Option<&ActiveUninstallBackupOwned> {
        self.backup.as_ref()
    }
}

impl ActiveUninstallBackupOwned {
    pub(crate) fn new(path: PathRef, snapshot: PeerPathSnapshot) -> Self {
        Self { path, snapshot }
    }

    pub(crate) fn path(&self) -> &PathRef {
        &self.path
    }

    pub(crate) fn snapshot(&self) -> &PeerPathSnapshot {
        &self.snapshot
    }
}

/// Owned hand-off from the command's sealed snapshot builder to the pure
/// active-uninstall composer.
#[derive(Debug)]
pub(crate) struct ActiveUninstallInputOwned {
    record: InstalledAddon,
    topology: GameProxyTopology,
    root: RenoDxRootSeal,
    endpoints: Vec<ActiveUninstallEndpointOwned>,
    ini_snapshot: PeerPathSnapshot,
}

impl ActiveUninstallInputOwned {
    pub(crate) fn new(
        record: InstalledAddon,
        topology: GameProxyTopology,
        root: RenoDxRootSeal,
        endpoints: Vec<ActiveUninstallEndpointOwned>,
        ini_snapshot: PeerPathSnapshot,
    ) -> Self {
        Self {
            record,
            topology,
            root,
            endpoints,
            ini_snapshot,
        }
    }

    pub(crate) fn input(&self) -> ActiveUninstallInput<'_> {
        ActiveUninstallInput {
            record: &self.record,
            topology: &self.topology,
            root: &self.root,
            endpoints: &self.endpoints,
            ini_snapshot: &self.ini_snapshot,
        }
    }

    pub(crate) fn root(&self) -> &RenoDxRootSeal {
        &self.root
    }
}

/// Borrowed pure-composer input. It cannot read storage or the filesystem.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ActiveUninstallInput<'a> {
    pub(crate) record: &'a InstalledAddon,
    pub(crate) topology: &'a GameProxyTopology,
    pub(crate) root: &'a RenoDxRootSeal,
    pub(crate) endpoints: &'a [ActiveUninstallEndpointOwned],
    pub(crate) ini_snapshot: &'a PeerPathSnapshot,
}

/// Exact physical and catalog projection for one active uninstall.
#[derive(Debug)]
pub(crate) struct ActiveUninstallComposition {
    pub(crate) program: ExactEndpointProgram,
    pub(crate) payloads: Vec<Option<Vec<u8>>>,
    pub(crate) game_intents: Vec<FileIntent>,
    pub(crate) planned_topology: PlannedGameProxyTopology,
    pub(crate) reshade_ini_authority: Option<RenoDxReshadeIniAuthority>,
}
