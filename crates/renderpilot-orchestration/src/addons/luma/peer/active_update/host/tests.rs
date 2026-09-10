use std::path::Path;

use renderpilot_domain::{
    AddonKind, FileOwnership, FileReceipt, GameId, GameProxyTopology, InstalledAddon,
    ManagedAddonFile, ManagedFileBaseline, PathRef, PlannedGameProxyTopology, ProxyImplementation,
    ProxyLink, ProxyRootPrestate, Version,
};

use crate::addons::luma::peer::{
    active_update::{
        error::LumaActiveUpdateError,
        model::{LumaActiveUpdateHostInput, LumaActiveUpdateHostObservation},
    },
    effects::{LumaPeerEffectAccumulator, LumaPeerOperationOrder},
    root_authority::LumaPeerRootAuthority,
};
use crate::addons::reshade::host_policy::{
    HostLifecycle, TopologyHostAssessment, assess_topology_downstream_for_tool,
};
use crate::peer_mutation_executor::{
    EndpointExpectation, EndpointPostcondition, PeerPathSnapshot, observe_peer_path_snapshot,
};

fn path(root: &Path, relative: &str) -> PathRef {
    PathRef::new(root.join(relative).to_string_lossy().into_owned()).expect("path")
}

fn digest(bytes: &[u8]) -> renderpilot_domain::Sha256Hash {
    renderpilot_detection::sha256_bytes(bytes).expect("digest")
}

fn host_bytes() -> Vec<u8> {
    host_bytes_version([6, 7, 0, 0])
}

fn host_bytes_version(version: [u16; 4]) -> Vec<u8> {
    let exported = crate::addons::test_support::build_pe_with_exports(
        crate::addons::test_support::MACHINE_AMD64,
        crate::addons::test_support::PE32_PLUS_MAGIC,
        &[
            "ReShadeVersion",
            "ReShadeRegisterAddon",
            "ReShadeUnregisterAddon",
            "ReShadeRegisterEvent",
        ],
    );
    let versioned = crate::addons::luma::test_support::build_nvidia_dlss_pe(version);
    let first_section_offset = 0x188usize;
    let first_raw_end = exported.len();
    let second_raw = (first_raw_end + 0x1ff) & !0x1ff;
    let resource = &versioned[0x200..];
    let second_rva = 0x2000u32;
    let mut bytes = vec![0u8; second_raw + resource.len()];
    bytes[..exported.len()].copy_from_slice(&exported);
    bytes[0x86..0x88].copy_from_slice(&2u16.to_le_bytes());
    let second_section = first_section_offset + 40;
    bytes[second_section..second_section + 8].copy_from_slice(b".rsrc\0\0\0");
    bytes[second_section + 8..second_section + 12]
        .copy_from_slice(&(resource.len() as u32).to_le_bytes());
    bytes[second_section + 12..second_section + 16].copy_from_slice(&second_rva.to_le_bytes());
    bytes[second_section + 16..second_section + 20]
        .copy_from_slice(&(resource.len() as u32).to_le_bytes());
    bytes[second_section + 20..second_section + 24]
        .copy_from_slice(&(second_raw as u32).to_le_bytes());
    bytes[0x98 + 112 + 16..0x98 + 112 + 20].copy_from_slice(&second_rva.to_le_bytes());
    bytes[0x98 + 112 + 20..0x98 + 112 + 24].copy_from_slice(&(resource.len() as u32).to_le_bytes());
    bytes[second_raw..].copy_from_slice(resource);
    bytes[second_raw + 72..second_raw + 76].copy_from_slice(&(second_rva + 88).to_le_bytes());
    bytes
}

struct Fixture {
    root: tempfile::TempDir,
    authority: LumaPeerRootAuthority,
    before: InstalledAddon,
    topology: GameProxyTopology,
    assessment: TopologyHostAssessment,
    host_path: PathRef,
    host_snapshot: PeerPathSnapshot,
}

impl Fixture {
    fn new(mode: FileOwnership, content: bool, minimum: &Version) -> Self {
        let root = tempfile::tempdir().expect("root");
        std::fs::write(root.path().join("dxgi.dll"), b"outer").expect("outer");
        let live = host_bytes();
        std::fs::write(root.path().join("ReShade64.dll"), &live).expect("host");
        if content {
            std::fs::write(root.path().join("example.fx"), b"user effect").expect("content");
        }
        let authority = LumaPeerRootAuthority::resolve(root.path(), &root.path().join("dxgi.dll"))
            .expect("authority");
        let host_path = path(root.path(), "ReShade64.dll");
        let host_snapshot =
            observe_peer_path_snapshot(&host_path, authority.canonical_game_root_ref())
                .expect("host snapshot");
        let file = host_snapshot.file().expect("host file");
        let game_id = GameId::new("manual:luma-active-update-host").expect("game id");
        let root_slot = path(root.path(), "dxgi.dll");
        let downstream_receipt = match mode {
            FileOwnership::Owned => FileReceipt::owned(file.identity(), file.digest().clone()),
            FileOwnership::Reused => FileReceipt::reused(file.identity(), file.digest().clone()),
        }
        .expect("downstream receipt");
        let topology = GameProxyTopology {
            id: "optiscaler:active-update-host".to_owned(),
            game_id: game_id.clone(),
            root_slot: root_slot.clone(),
            outer: ProxyLink {
                implementation: ProxyImplementation::OptiScaler,
                path: root_slot.clone(),
                receipt: FileReceipt::owned("outer", digest(b"outer")).expect("outer receipt"),
            },
            downstream: Some(ProxyLink {
                implementation: ProxyImplementation::ReShade,
                path: host_path.clone(),
                receipt: downstream_receipt,
            }),
            downstream_origin: Some(root_slot),
            root_prestate: ProxyRootPrestate::Absent,
        };
        let baseline = match mode {
            FileOwnership::Owned => ManagedFileBaseline::Absent,
            FileOwnership::Reused => ManagedFileBaseline::Present {
                sha256: file.digest().clone(),
            },
        };
        let managed = match mode {
            FileOwnership::Owned => {
                ManagedAddonFile::owned(host_path.clone(), baseline, file.digest().clone())
            }
            FileOwnership::Reused => {
                ManagedAddonFile::reused(host_path.clone(), file.digest().clone())
            }
        };
        let before =
            InstalledAddon::new(game_id, AddonKind::Luma, path(root.path(), "luma.addon64"))
                .try_with_managed_files(vec![managed])
                .expect("managed host");
        let assessment = assess_topology_downstream_for_tool(
            root.path(),
            Path::new(host_path.as_str()),
            "Luma",
            Some(minimum),
        )
        .expect("assessment");
        Self {
            root,
            authority,
            before,
            topology,
            assessment,
            host_path,
            host_snapshot,
        }
    }

    fn project(
        &self,
        input: LumaActiveUpdateHostInput,
    ) -> Result<super::super::model::HostProjection, LumaActiveUpdateError> {
        let observation = LumaActiveUpdateHostObservation::new(
            &self.assessment,
            &self.host_path,
            &self.host_snapshot,
        );
        let mut accumulator =
            LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);
        super::project_host(
            &self.before,
            &self.topology,
            &self.authority,
            input,
            observation,
            &Version::parse("0").expect("minimum"),
            &mut accumulator,
        )
    }
}

#[test]
fn reused_preserve_and_compatible_replace_are_exact_noops() {
    let fixture = Fixture::new(FileOwnership::Reused, false, &Version::parse("0").unwrap());
    assert_eq!(
        fixture.assessment.snapshot().lifecycle,
        HostLifecycle::AdoptEmpty
    );
    let projection = fixture
        .project(LumaActiveUpdateHostInput::Preserve)
        .expect("preserve");
    let (binding, topology) = projection.into_parts();
    assert_eq!(binding.mode(), renderpilot_domain::ManagedFileMode::Reused);
    assert_eq!(
        topology,
        PlannedGameProxyTopology::Exact(fixture.topology.clone())
    );

    let projection = fixture
        .project(LumaActiveUpdateHostInput::Replace {
            bytes: b"not used for a compatible reused host".to_vec(),
        })
        .expect("compatible replace");
    let (binding, topology) = projection.into_parts();
    assert_eq!(binding.mode(), renderpilot_domain::ManagedFileMode::Reused);
    assert_eq!(topology, PlannedGameProxyTopology::Exact(fixture.topology));
}

#[test]
fn reused_repair_requires_an_absent_sidecar_and_lowers_f4_acquisition() {
    let minimum = Version::parse("999").expect("minimum");
    let fixture = Fixture::new(FileOwnership::Reused, false, &minimum);
    assert_eq!(
        fixture.assessment.snapshot().lifecycle,
        HostLifecycle::RepairEmpty
    );
    let sidecar = fixture.root.path().join("ReShade64.dll.bak");
    std::fs::write(&sidecar, b"foreign sidecar").expect("sidecar");
    let result = fixture.project(LumaActiveUpdateHostInput::Replace {
        bytes: host_bytes_version([1000, 0, 0, 0]),
    });
    assert!(matches!(
        result,
        Err(error) if error.is_active_host_lowering()
    ));

    std::fs::remove_file(sidecar).expect("remove sidecar");
    let observation = LumaActiveUpdateHostObservation::new(
        &fixture.assessment,
        &fixture.host_path,
        &fixture.host_snapshot,
    );
    let mut accumulator = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);
    let projection = super::project_host(
        &fixture.before,
        &fixture.topology,
        &fixture.authority,
        LumaActiveUpdateHostInput::Replace {
            bytes: host_bytes_version([1000, 0, 0, 0]),
        },
        observation,
        &minimum,
        &mut accumulator,
    )
    .expect("repair");
    let effects = accumulator.finalize().expect("effects").expect("program");
    assert_eq!(effects.program().endpoints().len(), 2);
    assert_eq!(
        effects.program().endpoints()[0].path(),
        &path(fixture.root.path(), "ReShade64.dll.bak")
    );
    assert_eq!(effects.program().endpoints()[1].path(), &fixture.host_path);
    assert_eq!(
        projection.into_parts().0.mode(),
        renderpilot_domain::ManagedFileMode::Owned
    );
}

#[test]
fn owned_preserve_is_exact_and_owned_changed_replace_keeps_absent_baseline() {
    let fixture = Fixture::new(FileOwnership::Owned, false, &Version::parse("0").unwrap());
    assert_eq!(
        fixture.assessment.snapshot().lifecycle,
        HostLifecycle::AdoptEmpty
    );
    let projection = fixture
        .project(LumaActiveUpdateHostInput::Preserve)
        .expect("preserve");
    let (binding, topology) = projection.into_parts();
    assert_eq!(binding, fixture.before.managed_files()[0]);
    assert_eq!(
        topology,
        PlannedGameProxyTopology::Exact(fixture.topology.clone())
    );

    let replacement = host_bytes_version([1000, 0, 0, 0]);
    let observation = LumaActiveUpdateHostObservation::new(
        &fixture.assessment,
        &fixture.host_path,
        &fixture.host_snapshot,
    );
    let mut accumulator = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);
    let projection = super::project_host(
        &fixture.before,
        &fixture.topology,
        &fixture.authority,
        LumaActiveUpdateHostInput::Replace {
            bytes: replacement.clone(),
        },
        observation,
        &Version::parse("0").unwrap(),
        &mut accumulator,
    )
    .expect("replace");
    let effects = accumulator.finalize().expect("effects").expect("program");
    assert_eq!(effects.program().endpoints().len(), 1);
    assert_eq!(
        effects.program().endpoints()[0].role(),
        renderpilot_domain::PeerEndpointRole::TopologyDownstream
    );
    assert!(matches!(
        effects.program().endpoints()[0].before(),
        EndpointExpectation::File(_)
    ));
    assert!(matches!(
        effects.program().endpoints()[0].after(),
        EndpointPostcondition::File(hash) if hash == &digest(&replacement)
    ));
    let (binding, topology) = projection.into_parts();
    assert_eq!(binding.baseline(), &ManagedFileBaseline::Absent);
    assert!(
        matches!(topology, PlannedGameProxyTopology::ObservedOwnedDownstream { planned_sha256, .. } if planned_sha256 == digest(&replacement))
    );
}

#[test]
fn owned_present_baseline_replace_never_reads_or_changes_sidecar() {
    let fixture = Fixture::new(FileOwnership::Owned, false, &Version::parse("0").unwrap());
    let current = fixture.before.managed_files()[0].installed_sha256().clone();
    let baseline = digest(b"original baseline");
    let claim = ManagedAddonFile::owned(
        fixture.host_path.clone(),
        ManagedFileBaseline::Present { sha256: baseline },
        current,
    );
    let before = InstalledAddon::new(
        fixture.topology.game_id.clone(),
        AddonKind::Luma,
        path(fixture.root.path(), "luma.addon64"),
    )
    .try_with_managed_files(vec![claim])
    .expect("managed host");
    let replacement = host_bytes_version([1000, 0, 0, 0]);
    let observation = LumaActiveUpdateHostObservation::new(
        &fixture.assessment,
        &fixture.host_path,
        &fixture.host_snapshot,
    );
    let mut accumulator = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);
    super::project_host(
        &before,
        &fixture.topology,
        &fixture.authority,
        LumaActiveUpdateHostInput::Replace { bytes: replacement },
        observation,
        &Version::parse("0").unwrap(),
        &mut accumulator,
    )
    .expect("replace");
    let effects = accumulator.finalize().expect("effects").expect("program");
    assert_eq!(effects.program().endpoints().len(), 1);
    assert!(!fixture.root.path().join("ReShade64.dll.bak").exists());
}

#[test]
fn byte_identical_owned_replace_is_noop_and_baseline_replacement_is_rejected() {
    let fixture = Fixture::new(FileOwnership::Owned, false, &Version::parse("0").unwrap());
    let live = fixture.host_snapshot.bytes().expect("live").to_vec();
    let projection = fixture
        .project(LumaActiveUpdateHostInput::Replace { bytes: live })
        .expect("identical replace");
    assert_eq!(projection.into_parts().0, fixture.before.managed_files()[0]);

    let current = fixture.before.managed_files()[0].installed_sha256().clone();
    let claim = ManagedAddonFile::owned(
        fixture.host_path.clone(),
        ManagedFileBaseline::Present {
            sha256: digest(&host_bytes_version([1000, 0, 0, 0])),
        },
        current,
    );
    let before = InstalledAddon::new(
        fixture.topology.game_id.clone(),
        AddonKind::Luma,
        path(fixture.root.path(), "luma.addon64"),
    )
    .try_with_managed_files(vec![claim])
    .expect("managed host");
    let observation = LumaActiveUpdateHostObservation::new(
        &fixture.assessment,
        &fixture.host_path,
        &fixture.host_snapshot,
    );
    let mut accumulator = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);
    let result = super::project_host(
        &before,
        &fixture.topology,
        &fixture.authority,
        LumaActiveUpdateHostInput::Replace {
            bytes: host_bytes_version([1000, 0, 0, 0]),
        },
        observation,
        &Version::parse("0").unwrap(),
        &mut accumulator,
    );
    assert!(matches!(
        result,
        Err(error) if error.is_invalid_input()
    ));
    assert!(accumulator.finalize().expect("effects").is_none());
}

#[test]
fn ownership_path_digest_game_and_observation_mismatches_fail_before_effects() {
    let fixture = Fixture::new(FileOwnership::Owned, false, &Version::parse("0").unwrap());
    let mut topology = fixture.topology.clone();
    topology.game_id = GameId::new("manual:other-game").expect("game id");
    let observation = LumaActiveUpdateHostObservation::new(
        &fixture.assessment,
        &fixture.host_path,
        &fixture.host_snapshot,
    );
    let mut accumulator = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);
    let result = super::project_host(
        &fixture.before,
        &topology,
        &fixture.authority,
        LumaActiveUpdateHostInput::Replace {
            bytes: host_bytes_version([1000, 0, 0, 0]),
        },
        observation,
        &Version::parse("0").unwrap(),
        &mut accumulator,
    );
    assert!(matches!(
        result,
        Err(error) if error.is_invalid_input()
    ));
    assert!(accumulator.finalize().expect("effects").is_none());

    let wrong_path = path(fixture.root.path(), "other.dll");
    let observation = LumaActiveUpdateHostObservation::new(
        &fixture.assessment,
        &wrong_path,
        &fixture.host_snapshot,
    );
    let mut accumulator = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);
    let result = super::project_host(
        &fixture.before,
        &fixture.topology,
        &fixture.authority,
        LumaActiveUpdateHostInput::Replace {
            bytes: host_bytes_version([1000, 0, 0, 0]),
        },
        observation,
        &Version::parse("0").unwrap(),
        &mut accumulator,
    );
    assert!(matches!(
        result,
        Err(error) if error.is_host_classification()
    ));
    assert!(accumulator.finalize().expect("effects").is_none());
}

#[test]
fn invalid_prepared_host_and_owned_policy_conflicts_are_rejected() {
    let fixture = Fixture::new(FileOwnership::Owned, false, &Version::parse("0").unwrap());
    let result = fixture.project(LumaActiveUpdateHostInput::Replace {
        bytes: b"not a PE".to_vec(),
    });
    assert!(matches!(
        result,
        Err(error) if error.is_host_classification()
    ));

    let user_fixture = Fixture::new(FileOwnership::Owned, true, &Version::parse("0").unwrap());
    assert_eq!(
        user_fixture.assessment.snapshot().lifecycle,
        HostLifecycle::ReuseUser
    );
    let result = user_fixture.project(LumaActiveUpdateHostInput::Replace {
        bytes: host_bytes_version([1000, 0, 0, 0]),
    });
    assert!(matches!(
        result,
        Err(error) if error.is_invalid_input()
    ));

    let minimum = Version::parse("999").expect("minimum");
    let repair_fixture = Fixture::new(FileOwnership::Owned, false, &minimum);
    assert_eq!(
        repair_fixture.assessment.snapshot().lifecycle,
        HostLifecycle::RepairEmpty
    );
    let result = repair_fixture.project(LumaActiveUpdateHostInput::Preserve);
    assert!(matches!(
        result,
        Err(error) if error.is_invalid_input()
    ));
}

#[test]
fn prepared_x86_host_is_rejected_for_the_reshade64_slot() {
    let fixture = Fixture::new(FileOwnership::Owned, false, &Version::parse("0").unwrap());
    let mut x86 = host_bytes_version([1000, 0, 0, 0]);
    x86[0x84..0x86].copy_from_slice(&crate::addons::test_support::MACHINE_I386.to_le_bytes());
    let result = fixture.project(LumaActiveUpdateHostInput::Replace { bytes: x86 });
    assert!(matches!(
        result,
        Err(error) if error.is_host_classification()
    ));
}
