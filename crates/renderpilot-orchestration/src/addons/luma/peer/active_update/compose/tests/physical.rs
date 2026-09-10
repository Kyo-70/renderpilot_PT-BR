use std::path::Path;

use renderpilot_domain::{
    AddonKind, FileReceipt, GameProxyTopology, InstalledAddon, ManagedAddonFile, PeerEndpointRole,
    ProxyImplementation, ProxyLink, ProxyRootPrestate, TrackedSource, TrackedSourceRole, Version,
};

use crate::{
    addons::{
        luma::peer::{
            active_update::{
                compose_active_update,
                model::{
                    LumaActiveUpdateComposition, LumaActiveUpdateDgVoodooInput,
                    LumaActiveUpdateEvidence, LumaActiveUpdateHostInput,
                    LumaActiveUpdateHostObservation, LumaActiveUpdateInput,
                    LumaActiveUpdatePayloadInput, LumaActiveUpdatePrepared,
                },
            },
            root_authority::LumaPeerRootAuthority,
        },
        reshade::host_policy::assess_topology_downstream_for_tool,
    },
    catalog::cascade::CascadeResult,
    coordinated_files::CatalogPathClaim,
    peer_mutation_executor::observe_peer_path_snapshot,
};

fn path(root: &Path, relative: &str) -> renderpilot_domain::PathRef {
    renderpilot_domain::PathRef::new(root.join(relative).to_string_lossy().into_owned())
        .expect("path")
}

fn digest(bytes: &[u8]) -> renderpilot_domain::Sha256Hash {
    renderpilot_detection::sha256_bytes(bytes).expect("digest")
}

fn active_host_bytes(version: [u16; 4]) -> Vec<u8> {
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

fn sources(host: &renderpilot_domain::Sha256Hash) -> Vec<TrackedSource> {
    vec![
        TrackedSource::new(
            TrackedSourceRole::AddonPayload,
            "https://example.invalid/luma.addon64",
            None,
            "a".repeat(64),
        ),
        TrackedSource::new(
            TrackedSourceRole::HostBinary,
            "https://example.invalid/ReShade64.dll",
            None,
            host.as_str(),
        ),
    ]
}

#[test]
fn physical_composition_keeps_host_topology_authoritative_and_payload_aligned() {
    let root = tempfile::tempdir().expect("root");
    let old_host = active_host_bytes([6, 7, 0, 0]);
    let new_host = active_host_bytes([1000, 0, 0, 0]);
    std::fs::write(root.path().join("dxgi.dll"), b"outer").expect("outer");
    std::fs::write(root.path().join("ReShade64.dll"), &old_host).expect("host");

    let authority = LumaPeerRootAuthority::resolve(root.path(), &root.path().join("dxgi.dll"))
        .expect("authority");
    let host_path = path(root.path(), "ReShade64.dll");
    let root_slot = path(root.path(), "dxgi.dll");
    let old_digest = digest(&old_host);
    let snapshot = observe_peer_path_snapshot(&host_path, authority.canonical_game_root_ref())
        .expect("snapshot");
    let old_identity = snapshot.file().expect("host file").identity().to_owned();
    let downstream = FileReceipt::reused(old_identity, old_digest.clone()).expect("receipt");
    let topology = GameProxyTopology {
        id: "optiscaler:active-update-physical".to_owned(),
        game_id: renderpilot_domain::GameId::new("manual:luma-active-update-physical")
            .expect("game"),
        root_slot: root_slot.clone(),
        outer: ProxyLink {
            implementation: ProxyImplementation::OptiScaler,
            path: root_slot.clone(),
            receipt: FileReceipt::owned("outer", digest(b"outer")).expect("outer receipt"),
        },
        downstream: Some(ProxyLink {
            implementation: ProxyImplementation::ReShade,
            path: host_path.clone(),
            receipt: downstream,
        }),
        downstream_origin: Some(root_slot),
        root_prestate: ProxyRootPrestate::Absent,
    };
    let before = InstalledAddon::new(
        topology.game_id.clone(),
        AddonKind::Luma,
        path(root.path(), "Luma.addon64"),
    )
    .try_with_managed_files(vec![ManagedAddonFile::reused(
        host_path.clone(),
        old_digest.clone(),
    )])
    .expect("managed host")
    .with_tracked_sources(sources(&old_digest));
    let minimum = Version::parse("999").expect("minimum");
    let assessment = assess_topology_downstream_for_tool(
        root.path(),
        Path::new(host_path.as_str()),
        "Luma",
        Some(&minimum),
    )
    .expect("assessment");
    let observation = LumaActiveUpdateHostObservation::new(&assessment, &host_path, &snapshot);
    let new_digest = digest(&new_host);
    let prepared = LumaActiveUpdatePrepared::new(
        LumaActiveUpdatePayloadInput::Preserve,
        LumaActiveUpdateHostInput::Replace {
            bytes: new_host.clone(),
        },
        LumaActiveUpdateDgVoodooInput::Preserve,
        Vec::<renderpilot_domain::PathRef>::new(),
        sources(&new_digest),
        Some("updated".to_owned()),
    );
    let claim = CatalogPathClaim::for_test(Vec::new(), None);
    let cascade = CascadeResult::empty_for_test();
    let evidence = LumaActiveUpdateEvidence::new(
        &before,
        &topology,
        &authority,
        observation,
        &minimum,
        &claim,
        &cascade,
    );
    let result = compose_active_update(LumaActiveUpdateInput::new(evidence, prepared))
        .expect("physical composition");

    let LumaActiveUpdateComposition::Physical(physical) = result else {
        panic!("host replacement must be physical");
    };
    assert_eq!(physical.program().endpoints().len(), 2);
    assert_eq!(physical.payloads().len(), 2);
    let sidecar_path = path(root.path(), "ReShade64.dll.bak");
    let sidecar = &physical.program().endpoints()[0];
    assert_eq!(sidecar.path(), &sidecar_path);
    assert_eq!(sidecar.role(), PeerEndpointRole::Disjoint);
    assert_eq!(physical.payloads()[0].as_deref(), Some(old_host.as_slice()));
    let endpoint = &physical.program().endpoints()[1];
    assert_eq!(endpoint.path(), &host_path);
    assert_eq!(endpoint.role(), PeerEndpointRole::TopologyDownstream);
    assert_eq!(physical.payloads()[1].as_deref(), Some(new_host.as_slice()));
    assert!(matches!(
        physical.planned_topology(),
        renderpilot_domain::PlannedGameProxyTopology::ObservedOwnedDownstream { .. }
    ));
    assert_eq!(
        physical.after_record().managed_files()[0].installed_sha256(),
        &new_digest
    );
    assert_eq!(
        physical.after_record().tracked_sources()[1].digest(),
        new_digest.as_str()
    );
}
