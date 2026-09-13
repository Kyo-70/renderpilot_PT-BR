use std::path::Path;

use renderpilot_domain::{
    FileOwnership, FileReceipt, GameId, GameProxyTopology, ManagedFileBaseline, PathRef,
    PlannedGameProxyTopology, ProxyImplementation, ProxyLink, ProxyRootPrestate, Version,
};

use super::active_host::{
    ActiveHostClassification, ActiveHostClassificationError, ActiveHostLoweringError,
    classify_active_host, lower_active_host_owned,
};
use super::effects::{LumaPeerEffectAccumulator, LumaPeerOperationOrder};
use super::root_authority::LumaPeerRootAuthority;
use crate::addons::reshade::host_policy::{
    HostLifecycle, TopologyHostAssessment, assess_topology_downstream_for_tool,
};
use crate::addons::reshade::scan::ReshadeAddonSupport;
use crate::peer_mutation_executor::{
    EndpointExpectation, EndpointPostcondition, PeerPathSnapshot, observe_peer_path_snapshot,
};

fn path(path: &Path) -> PathRef {
    PathRef::new(path.to_string_lossy().into_owned()).expect("path")
}

fn digest(bytes: &[u8]) -> renderpilot_domain::Sha256Hash {
    renderpilot_detection::sha256_bytes(bytes).expect("digest")
}

fn minimum_version() -> Version {
    Version::parse("0").expect("minimum version")
}

fn host_bytes() -> Vec<u8> {
    host_bytes_version([6, 7, 0, 0])
}

fn x86_host_bytes() -> Vec<u8> {
    let mut bytes = host_bytes();
    bytes[0x84..0x86].copy_from_slice(&crate::addons::test_support::MACHINE_I386.to_le_bytes());
    bytes
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
    // Reuse the production test resource fixture and add the ReShade export
    // section to it. This keeps the prepared image both versioned and fully
    // add-on-capable without a second filesystem representation.
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

fn authority(root: &Path) -> LumaPeerRootAuthority {
    LumaPeerRootAuthority::resolve(root, &root.join("dxgi.dll")).expect("authority")
}

fn topology(root: &Path, downstream: Option<(FileOwnership, &[u8])>) -> GameProxyTopology {
    let root_slot = path(&root.join("dxgi.dll"));
    GameProxyTopology {
        id: "optiscaler:active-host-test".to_owned(),
        game_id: GameId::new("manual:active-host-test").expect("game id"),
        root_slot: root_slot.clone(),
        outer: ProxyLink {
            implementation: ProxyImplementation::OptiScaler,
            path: root_slot,
            receipt: FileReceipt::owned("outer", digest(b"outer")).expect("outer receipt"),
        },
        downstream: downstream.map(|(ownership, bytes)| {
            let observed_host = snapshot(root, "ReShade64.dll");
            let observed_file = observed_host.file().expect("host snapshot");
            assert_eq!(
                observed_file.digest(),
                &digest(bytes),
                "topology fixture receipt must describe the retained host image"
            );
            let receipt = match ownership {
                FileOwnership::Owned => {
                    FileReceipt::owned(observed_file.identity(), observed_file.digest().clone())
                }
                FileOwnership::Reused => {
                    FileReceipt::reused(observed_file.identity(), observed_file.digest().clone())
                }
            }
            .expect("host receipt");
            ProxyLink {
                implementation: ProxyImplementation::ReShade,
                path: path(&root.join("ReShade64.dll")),
                receipt,
            }
        }),
        downstream_origin: downstream.map(|_| path(&root.join("dxgi.dll"))),
        root_prestate: ProxyRootPrestate::Absent,
    }
}

fn assessment(root: &Path) -> TopologyHostAssessment {
    assess_topology_downstream_for_tool(
        root,
        &root.join("ReShade64.dll"),
        "Luma",
        Some(&minimum_version()),
    )
    .expect("assessment")
}

fn snapshot(root: &Path, name: &str) -> PeerPathSnapshot {
    observe_peer_path_snapshot(&path(&root.join(name)), &path(root)).expect("snapshot")
}

#[test]
fn fresh_slot_requires_prepared_host_and_lowers_to_one_coordinated_create() {
    let root = tempfile::tempdir().expect("root");
    std::fs::write(root.path().join("dxgi.dll"), b"outer").expect("outer");
    let authority = authority(root.path());
    let topology = topology(root.path(), None);
    let assessment = assessment(root.path());
    let host_path = path(&root.path().join("ReShade64.dll"));
    let absent = snapshot(root.path(), "ReShade64.dll");
    let prepared = host_bytes_version([1000, 0, 0, 0]);

    let classification = classify_active_host(
        &authority,
        &topology,
        &assessment,
        &host_path,
        &absent,
        Some(prepared.clone()),
        &minimum_version(),
    )
    .expect("fresh classification");
    let plan = classification.owned().expect("owned create");
    assert!(plan.is_create());
    assert!(matches!(
        plan.binding().baseline(),
        ManagedFileBaseline::Absent
    ));
    assert!(matches!(
        plan.planned_topology(),
        PlannedGameProxyTopology::ObservedOwnedDownstream {
            implementation: ProxyImplementation::ReShade,
            downstream_path,
            planned_sha256,
            planned_length,
            ..
        } if downstream_path == &host_path
            && planned_sha256 == &digest(&prepared)
            && *planned_length == prepared.len() as u64
    ));

    let mut effects = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);
    lower_active_host_owned(
        classification.into_owned().expect("owned create"),
        &absent,
        None,
        &mut effects,
    )
    .expect("lower fresh create");
    let effects = effects.finalize().expect("finalize").expect("host effect");
    assert_eq!(effects.program().endpoints().len(), 1);
    assert_eq!(effects.program().endpoints()[0].path(), &host_path);
    assert_eq!(
        effects.program().endpoints()[0].role(),
        renderpilot_domain::PeerEndpointRole::TopologyDownstream
    );
    assert!(matches!(
        effects.program().endpoints()[0].before(),
        EndpointExpectation::Absent
    ));
    assert!(matches!(
        effects.program().endpoints()[0].after(),
        EndpointPostcondition::File(hash) if hash == &digest(&prepared)
    ));
}

#[test]
fn reused_downstream_is_exact_noop_and_does_not_request_a_foreign_sidecar() {
    let root = tempfile::tempdir().expect("root");
    std::fs::write(root.path().join("dxgi.dll"), b"outer").expect("outer");
    let live = host_bytes();
    std::fs::write(root.path().join("ReShade64.dll"), &live).expect("host");
    std::fs::write(root.path().join("ReShade64.dll.bak"), b"foreign backup")
        .expect("foreign backup");
    let authority = authority(root.path());
    let topology = topology(root.path(), Some((FileOwnership::Reused, &live)));
    let assessment = assessment(root.path());
    assert_eq!(assessment.snapshot().lifecycle, HostLifecycle::AdoptEmpty);
    assert_eq!(
        assessment.snapshot().addon_support,
        Some(ReshadeAddonSupport::Full)
    );
    let host_path = path(&root.path().join("ReShade64.dll"));
    let live_snapshot = snapshot(root.path(), "ReShade64.dll");

    let classification = classify_active_host(
        &authority,
        &topology,
        &assessment,
        &host_path,
        &live_snapshot,
        None,
        &minimum_version(),
    )
    .expect("reused classification");
    let ActiveHostClassification::Reused {
        binding,
        planned_topology,
    } = classification
    else {
        panic!("expected reused classification");
    };
    assert_eq!(binding.mode(), renderpilot_domain::ManagedFileMode::Reused);
    assert_eq!(planned_topology, PlannedGameProxyTopology::Exact(topology));
}

#[test]
fn repair_empty_requires_absent_managed_sidecar_and_emits_sidecar_then_host_replace() {
    let root = tempfile::tempdir().expect("root");
    std::fs::write(root.path().join("dxgi.dll"), b"outer").expect("outer");
    let original = host_bytes();
    std::fs::write(root.path().join("ReShade64.dll"), &original).expect("host");
    let authority = authority(root.path());
    let topology = topology(root.path(), Some((FileOwnership::Reused, &original)));
    let minimum = Version::parse("999").expect("minimum");
    let assessment = assess_topology_downstream_for_tool(
        root.path(),
        &root.path().join("ReShade64.dll"),
        "Luma",
        Some(&minimum),
    )
    .expect("assessment");
    assert_eq!(assessment.snapshot().lifecycle, HostLifecycle::RepairEmpty);
    let host_path = path(&root.path().join("ReShade64.dll"));
    let live_snapshot = snapshot(root.path(), "ReShade64.dll");
    std::fs::write(root.path().join("ReShade64.dll.bak"), b"foreign backup")
        .expect("foreign sidecar");
    let sidecar_snapshot = snapshot(root.path(), "ReShade64.dll.bak");
    let prepared = host_bytes_version([1000, 0, 0, 0]);

    let classification = classify_active_host(
        &authority,
        &topology,
        &assessment,
        &host_path,
        &live_snapshot,
        Some(prepared.clone()),
        &minimum,
    )
    .expect("repair classification");
    let plan = classification.into_owned().expect("owned replace");
    assert!(!plan.is_create());
    assert!(plan.sidecar_path().is_ok());

    let mut effects = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);
    let error = lower_active_host_owned(
        plan.clone(),
        &live_snapshot,
        Some(&sidecar_snapshot),
        &mut effects,
    )
    .expect_err("present sidecar must reject acquisition");
    assert!(matches!(error, ActiveHostLoweringError::Snapshot(_)));
    assert!(effects.finalize().expect("finalize").is_none());

    std::fs::remove_file(root.path().join("ReShade64.dll.bak")).expect("remove foreign sidecar");
    let mut effects = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);
    let sidecar = plan.sidecar_path().expect("sidecar");
    let sidecar_snapshot =
        observe_peer_path_snapshot(&sidecar, &path(root.path())).expect("absent managed sidecar");
    lower_active_host_owned(plan, &live_snapshot, Some(&sidecar_snapshot), &mut effects)
        .expect("lower repair");
    let effects = effects.finalize().expect("finalize").expect("effects");
    assert_eq!(effects.program().endpoints().len(), 2);
    assert_eq!(effects.program().endpoints()[0].path(), &sidecar);
    assert_eq!(effects.program().endpoints()[1].path(), &host_path);
    assert_eq!(effects.payloads(), &[Some(original), Some(prepared)]);
}

#[test]
fn owned_downstream_is_foreign_authority_and_prepared_host_is_strictly_validated() {
    let root = tempfile::tempdir().expect("root");
    std::fs::write(root.path().join("dxgi.dll"), b"outer").expect("outer");
    let live = host_bytes();
    std::fs::write(root.path().join("ReShade64.dll"), &live).expect("host");
    let authority = authority(root.path());
    let owned_topology = topology(root.path(), Some((FileOwnership::Owned, &live)));
    let assessment = assessment(root.path());
    let host_path = path(&root.path().join("ReShade64.dll"));
    let live_snapshot = snapshot(root.path(), "ReShade64.dll");

    assert!(matches!(
        classify_active_host(
            &authority,
            &owned_topology,
            &assessment,
            &host_path,
            &live_snapshot,
            Some(live),
            &minimum_version(),
        ),
        Err(ActiveHostClassificationError::Topology(_))
    ));

    let no_downstream = topology(root.path(), None);
    let absent = snapshot(root.path(), "missing.dll");
    assert!(matches!(
        classify_active_host(
            &authority,
            &no_downstream,
            &assessment,
            &host_path,
            &absent,
            Some(b"not a PE".to_vec()),
            &minimum_version(),
        ),
        Err(ActiveHostClassificationError::Evidence { .. }
            | ActiveHostClassificationError::Assessment(_),)
    ));
}

#[test]
fn active_reshade64_slot_rejects_a_32_bit_prepared_host() {
    let root = tempfile::tempdir().expect("root");
    std::fs::write(root.path().join("dxgi.dll"), b"outer").expect("outer");
    let authority = authority(root.path());
    let topology = topology(root.path(), None);
    let assessment = assessment(root.path());
    let host_path = path(&root.path().join("ReShade64.dll"));
    let absent = snapshot(root.path(), "ReShade64.dll");

    assert!(matches!(
        classify_active_host(
            &authority,
            &topology,
            &assessment,
            &host_path,
            &absent,
            Some(x86_host_bytes()),
            &minimum_version(),
        ),
        Err(ActiveHostClassificationError::Prepared(
            "prepared bytes are not a 64-bit ReShade image for ReShade64.dll"
        ))
    ));
}
