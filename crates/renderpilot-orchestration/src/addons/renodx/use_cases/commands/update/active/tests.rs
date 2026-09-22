use std::path::{Path, PathBuf};

use renderpilot_domain::{
    AddonKind, FileReceipt, GameId, GameProxyTopology, InstalledAddon, InstalledAddonHostKind,
    ManagedAddonFile, ManagedFileBaseline, ManagedFileMode, PathRef, ProxyImplementation,
    ProxyLink, ProxyRootPrestate, Sha256Hash, TrackedSource, TrackedSourceRole,
};
use sha2::{Digest, Sha256};
use tempfile::{TempDir, tempdir};

use crate::addons::file_update::Replacement;
use crate::addons::peer_lifecycle::PeerRoots;
use crate::addons::renodx::peer::{RenoDxConfigSourceSeal, RenoDxRootSeal};
use crate::addons::renodx::types::RenoDxProcessingPath;
use crate::addons::renodx::use_cases::commands::update::prepare::{
    HostInstall, PreparedUpdateArtifacts,
};
use crate::addons::renodx::use_cases::commands::update::snapshot::UpdateSnapshot;
use crate::peer_mutation_executor::{PeerPathSnapshot, observe_peer_path_snapshot};

use super::super::route::{UpdatePhase1, ensure_update_route_matches};
use super::lowering::lower_active_update;
use super::snapshot::ActiveUpdatePhase1;

fn game_id() -> GameId {
    GameId::new("manual:renodx-active-update").expect("game id")
}

fn digest(bytes: &[u8]) -> Sha256Hash {
    Sha256Hash::new(hex::encode(Sha256::digest(bytes))).expect("digest")
}

fn path(path: &Path) -> PathRef {
    PathRef::new(path.to_string_lossy().into_owned()).expect("path")
}

fn source(role: TrackedSourceRole, bytes: &[u8], url: &str) -> TrackedSource {
    TrackedSource::new(role, url, None, digest(bytes).as_str())
}

struct Fixture {
    _root: TempDir,
    addon_path: PathRef,
    host_path: PathRef,
    addon_old: Vec<u8>,
}

impl Fixture {
    fn new() -> Self {
        let root = tempdir().expect("root");
        let addon_old = b"old-addon".to_vec();
        let host_old = b"old-host".to_vec();
        let addon = root.path().join("renodx.addon64");
        let host = root.path().join("ReShade64.dll");
        std::fs::write(&addon, &addon_old).expect("addon");
        std::fs::write(&host, &host_old).expect("host");
        Self {
            _root: root,
            addon_path: path(&addon),
            host_path: path(&host),
            addon_old,
        }
    }

    fn root(&self) -> &Path {
        self._root.path()
    }

    fn root_ref(&self) -> PathRef {
        path(self.root())
    }

    fn seal(&self, proxy: bool) -> RenoDxRootSeal {
        let root = crate::paths::canonicalize_existing(self.root()).expect("canonical root");
        let root_ref = path(&root);
        let ini = root.join("ReShade.ini");
        RenoDxRootSeal {
            canonical_game_root: root.clone(),
            canonical_game_root_ref: root_ref,
            config_source: RenoDxConfigSourceSeal::Absent {
                exact_ini_path: ini.clone(),
            },
            effective_addon_root: root.clone(),
            payload_root: None,
            payload_root_ref: None,
            exact_ini_path: ini,
            exact_proxy_host: proxy.then(|| root.join("ReShade64.dll")),
            canonical_registered_exe: (!proxy).then(|| root.join("game.exe")),
            roots: PeerRoots::new(root, None).expect("roots"),
        }
    }

    fn addon_snapshot(&self) -> PeerPathSnapshot {
        observe_peer_path_snapshot(&self.addon_path, &self.root_ref()).expect("addon snapshot")
    }

    fn host_snapshot(&self) -> PeerPathSnapshot {
        observe_peer_path_snapshot(&self.host_path, &self.root_ref()).expect("host snapshot")
    }

    fn vulkan_phase(&self, source: Option<TrackedSource>) -> ActiveUpdatePhase1 {
        let mut record = InstalledAddon::new(game_id(), AddonKind::RenoDx, self.addon_path.clone())
            .with_host_kind(InstalledAddonHostKind::SharedVulkanLayer)
            .with_registered_exe_path(path(&self.root().join("game.exe")));
        if let Some(source) = source {
            record = record.with_tracked_source(source);
        }
        let base = UpdateSnapshot {
            record,
            game_dir: self.root().to_path_buf(),
            processing_path: RenoDxProcessingPath::Unmanaged,
            renodx_config: None,
            shared_vulkan_channel: None,
            addon: None,
            host: None,
            host_target: None,
        };
        ActiveUpdatePhase1 {
            base,
            topology: self.topology(None),
            root_seal: self.seal(false),
            addon_path: self.addon_path.clone(),
            addon_snapshot: self.addon_snapshot(),
            host_path: None,
            host_snapshot: None,
        }
    }

    fn proxy_phase(&self, host_mode: ManagedFileMode) -> ActiveUpdatePhase1 {
        let host_snapshot = self.host_snapshot();
        let host_file = host_snapshot.file().expect("host file");
        let host_claim = match host_mode {
            ManagedFileMode::Owned => ManagedAddonFile::owned(
                self.host_path.clone(),
                ManagedFileBaseline::Absent,
                host_file.digest().clone(),
            ),
            ManagedFileMode::Reused => {
                ManagedAddonFile::reused(self.host_path.clone(), host_file.digest().clone())
            }
        };
        let record = InstalledAddon::new(game_id(), AddonKind::RenoDx, self.addon_path.clone())
            .with_host_kind(InstalledAddonHostKind::Proxy)
            .try_with_managed_files(vec![host_claim])
            .expect("managed host");
        ActiveUpdatePhase1 {
            base: UpdateSnapshot {
                record,
                game_dir: self.root().to_path_buf(),
                processing_path: RenoDxProcessingPath::Unmanaged,
                renodx_config: None,
                shared_vulkan_channel: None,
                addon: None,
                host: None,
                host_target: None,
            },
            topology: self.topology(Some(host_file.digest().clone())),
            root_seal: self.seal(true),
            addon_path: self.addon_path.clone(),
            addon_snapshot: self.addon_snapshot(),
            host_path: Some(self.host_path.clone()),
            host_snapshot: Some(self.host_snapshot()),
        }
    }

    fn topology(&self, host_digest: Option<Sha256Hash>) -> GameProxyTopology {
        let root_slot = path(&self.root().join("dxgi.dll"));
        let outer = ProxyLink {
            implementation: ProxyImplementation::OptiScaler,
            path: root_slot.clone(),
            receipt: FileReceipt::owned("outer", digest(b"outer")).expect("outer receipt"),
        };
        let (downstream, origin, prestate) = match host_digest {
            Some(digest) => (
                Some(ProxyLink {
                    implementation: ProxyImplementation::ReShade,
                    path: self.host_path.clone(),
                    receipt: FileReceipt::reused("host", digest).expect("host receipt"),
                }),
                Some(root_slot.clone()),
                ProxyRootPrestate::RelocatedDownstream,
            ),
            None => (None, None, ProxyRootPrestate::Absent),
        };
        GameProxyTopology {
            id: "optiscaler:renodx-active-update".to_owned(),
            game_id: game_id(),
            root_slot,
            outer,
            downstream,
            downstream_origin: origin,
            root_prestate: prestate,
        }
    }

    fn prepared(
        &self,
        refreshed_sources: Vec<TrackedSource>,
        replacements: Vec<Replacement>,
    ) -> PreparedUpdateArtifacts {
        PreparedUpdateArtifacts {
            refreshed_sources,
            replacements,
            host_install: None,
            config: None,
        }
    }
}

#[test]
fn addon_only_vulkan_lowering_preserves_exact_topology_and_mtime() {
    let fixture = Fixture::new();
    let new_addon = b"new-addon".to_vec();
    let phase1 = fixture.vulkan_phase(Some(source(
        TrackedSourceRole::AddonPayload,
        &fixture.addon_old,
        "https://old/addon",
    )));
    let phase3 = fixture.vulkan_phase(Some(source(
        TrackedSourceRole::AddonPayload,
        &fixture.addon_old,
        "https://old/addon",
    )));
    let prepared = fixture.prepared(
        vec![source(
            TrackedSourceRole::AddonPayload,
            &new_addon,
            "https://new/addon",
        )],
        vec![Replacement {
            path: PathBuf::from(fixture.addon_path.as_str()),
            bytes: new_addon,
            mtime: Some("Wed, 01 Jan 2025 00:00:00 GMT".to_owned()),
        }],
    );

    let lowered = lower_active_update(&phase1, &phase3, &prepared).expect("lowering");
    assert!(lowered.composition.physical().is_some());
    assert_eq!(
        lowered.addon_mtime.as_deref(),
        Some("Wed, 01 Jan 2025 00:00:00 GMT")
    );
    let physical = lowered.composition.physical().expect("physical");
    assert_eq!(
        physical.planned_topology(),
        &renderpilot_domain::PlannedGameProxyTopology::Exact(phase3.topology.clone())
    );
    assert!(phase3.host_path.is_none());
}

#[test]
fn owned_host_lowering_updates_only_managed_digest() {
    let fixture = Fixture::new();
    let phase1 = fixture.proxy_phase(ManagedFileMode::Owned);
    let phase3 = fixture.proxy_phase(ManagedFileMode::Owned);
    let new_host = b"new-host".to_vec();
    let before = phase1.base.record().managed_files()[0].baseline().clone();
    let prepared = fixture.prepared(
        vec![source(
            TrackedSourceRole::HostBinary,
            &new_host,
            "https://new/host",
        )],
        vec![Replacement {
            path: PathBuf::from(fixture.host_path.as_str()),
            bytes: new_host.clone(),
            mtime: Some("must-not-be-used".to_owned()),
        }],
    );

    let lowered = lower_active_update(&phase1, &phase3, &prepared).expect("lowering");
    assert!(lowered.composition.physical().is_some());
    assert!(lowered.addon_mtime.is_none());
    let physical = lowered.composition.physical().expect("physical");
    let claim = &physical.after_peer().managed_files()[0];
    assert_eq!(claim.mode(), ManagedFileMode::Owned);
    assert_eq!(claim.baseline(), &before);
    assert_eq!(claim.installed_sha256(), &digest(&new_host));
}

#[test]
fn reused_host_and_relocation_are_rejected_before_composition() {
    let fixture = Fixture::new();
    let phase = fixture.proxy_phase(ManagedFileMode::Reused);
    let new_host = b"new-host".to_vec();
    let reused = fixture.prepared(
        vec![source(
            TrackedSourceRole::HostBinary,
            &new_host,
            "https://new/host",
        )],
        vec![Replacement {
            path: PathBuf::from(fixture.host_path.as_str()),
            bytes: new_host,
            mtime: None,
        }],
    );
    assert!(lower_active_update(&phase, &phase, &reused).is_err());

    let relocation = PreparedUpdateArtifacts {
        refreshed_sources: Vec::new(),
        replacements: Vec::new(),
        host_install: Some(HostInstall {
            game_dir: fixture.root().to_path_buf(),
            name: "other.dll".to_owned(),
            bytes: b"host".to_vec(),
        }),
        config: None,
    };
    assert!(lower_active_update(&phase, &phase, &relocation).is_err());
}

#[test]
fn unknown_duplicate_and_source_mismatch_replacements_fail_closed() {
    let fixture = Fixture::new();
    let phase1 = fixture.vulkan_phase(None);
    let phase3 = fixture.vulkan_phase(None);
    let unknown = fixture.prepared(
        Vec::new(),
        vec![Replacement {
            path: fixture.root().join("unknown.dll"),
            bytes: b"unknown".to_vec(),
            mtime: None,
        }],
    );
    assert!(lower_active_update(&phase1, &phase3, &unknown).is_err());

    let duplicate = fixture.prepared(
        Vec::new(),
        vec![
            Replacement {
                path: PathBuf::from(fixture.addon_path.as_str()),
                bytes: b"one".to_vec(),
                mtime: None,
            },
            Replacement {
                path: PathBuf::from(fixture.addon_path.as_str()),
                bytes: b"two".to_vec(),
                mtime: None,
            },
        ],
    );
    assert!(lower_active_update(&phase1, &phase3, &duplicate).is_err());

    let mismatch = fixture.prepared(
        vec![source(
            TrackedSourceRole::AddonPayload,
            b"not-the-postimage",
            "https://bad/addon",
        )],
        vec![Replacement {
            path: PathBuf::from(fixture.addon_path.as_str()),
            bytes: b"postimage".to_vec(),
            mtime: None,
        }],
    );
    assert!(lower_active_update(&phase1, &phase3, &mismatch).is_err());
}

#[test]
fn retained_source_update_yields_metadata_and_unchanged_update_yields_noop() {
    let fixture = Fixture::new();
    let old_source = source(
        TrackedSourceRole::AddonPayload,
        &fixture.addon_old,
        "https://old/addon",
    );
    let phase1 = fixture.vulkan_phase(Some(old_source.clone()));
    let phase3 = fixture.vulkan_phase(Some(old_source.clone()));
    let metadata = fixture.prepared(
        vec![source(
            TrackedSourceRole::AddonPayload,
            &fixture.addon_old,
            "https://new/metadata",
        )],
        Vec::new(),
    );
    let lowered = lower_active_update(&phase1, &phase3, &metadata).expect("metadata");
    assert!(lowered.composition.metadata().is_some());

    let noop = fixture.prepared(vec![old_source], Vec::new());
    let lowered = lower_active_update(&phase1, &phase3, &noop).expect("noop");
    assert!(matches!(
        lowered.composition,
        crate::addons::renodx::peer::RenoDxActiveUpdateComposition::Noop
    ));
}

#[test]
fn phase_three_requires_the_same_active_route_and_exact_evidence() {
    let fixture = Fixture::new();
    let phase1 = fixture.vulkan_phase(None);
    let phase3 = fixture.vulkan_phase(None);
    assert!(
        ensure_update_route_matches(
            &UpdatePhase1::Active(Box::new(phase1)),
            &UpdatePhase1::Active(Box::new(phase3)),
        )
        .is_ok()
    );

    let active = fixture.vulkan_phase(None);
    let inactive = UpdatePhase1::Inactive(Box::new(active.base.clone()));
    assert!(
        ensure_update_route_matches(&UpdatePhase1::Active(Box::new(active)), &inactive).is_err()
    );

    let active = fixture.vulkan_phase(None);
    let mut drifted = fixture.vulkan_phase(None);
    drifted.addon_path = path(&fixture.root().join("other.addon64"));
    assert!(
        ensure_update_route_matches(
            &UpdatePhase1::Active(Box::new(active)),
            &UpdatePhase1::Active(Box::new(drifted)),
        )
        .is_err()
    );
}
