use std::path::Path;

use renderpilot_domain::{
    AddonKind, FileReceipt, GameId, GameProxyTopology, ManagedFileMode, PathRef,
    ProxyImplementation, ProxyLink, ProxyRootPrestate, Sha256Hash, TrackedSourceRole, Version,
};

use super::active_install::{LumaActiveInstallInput, compose_active_install};
use super::root_authority::LumaPeerRootAuthority;
use crate::addons::luma::dgvoodoo::{DgVoodooInstall, ReusedDgVoodoo};
use crate::addons::luma::fetch::types::LumaPayloadFile;
use crate::addons::luma::install::PreparedInstall;
use crate::addons::reshade::host_policy::assess_topology_downstream_for_tool;
use crate::coordinated_files::CatalogPathClaim;
use crate::peer_mutation_executor::observe_peer_path_snapshot;

fn path(path: &Path) -> PathRef {
    PathRef::from_canonical_native_absolute(path).expect("path")
}

fn digest(bytes: &[u8]) -> Sha256Hash {
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
    let second_raw = (exported.len() + 0x1ff) & !0x1ff;
    let resource = &versioned[0x200..];
    let second_rva = 0x2000u32;
    let mut bytes = vec![0u8; second_raw + resource.len()];
    bytes[..exported.len()].copy_from_slice(&exported);
    bytes[0x86..0x88].copy_from_slice(&2u16.to_le_bytes());
    let section = first_section_offset + 40;
    bytes[section..section + 8].copy_from_slice(b".rsrc\0\0\0");
    bytes[section + 8..section + 12].copy_from_slice(&(resource.len() as u32).to_le_bytes());
    bytes[section + 12..section + 16].copy_from_slice(&second_rva.to_le_bytes());
    bytes[section + 16..section + 20].copy_from_slice(&(resource.len() as u32).to_le_bytes());
    bytes[section + 20..section + 24].copy_from_slice(&(second_raw as u32).to_le_bytes());
    bytes[0x98 + 112 + 16..0x98 + 112 + 20].copy_from_slice(&second_rva.to_le_bytes());
    bytes[0x98 + 112 + 20..0x98 + 112 + 24].copy_from_slice(&(resource.len() as u32).to_le_bytes());
    bytes[second_raw..].copy_from_slice(resource);
    bytes[second_raw + 72..second_raw + 76].copy_from_slice(&(second_rva + 88).to_le_bytes());
    bytes
}

fn topology(root: &Path, game_id: &GameId) -> GameProxyTopology {
    let root_slot = path(&root.join("dxgi.dll"));
    GameProxyTopology {
        id: "optiscaler:active-install-test".to_owned(),
        game_id: game_id.clone(),
        root_slot: root_slot.clone(),
        outer: ProxyLink {
            implementation: ProxyImplementation::OptiScaler,
            path: root_slot,
            receipt: FileReceipt::owned("outer", digest(b"outer")).expect("receipt"),
        },
        downstream: None,
        downstream_origin: None,
        root_prestate: ProxyRootPrestate::Absent,
    }
}

fn topology_with_host(root: &Path, game_id: &GameId, host: &[u8]) -> GameProxyTopology {
    let mut topology = topology(root, game_id);
    let host_path = path(&root.join("ReShade64.dll"));
    let host_snapshot = observe_peer_path_snapshot(&host_path, &path(root)).expect("host");
    let file = host_snapshot.file().expect("host file");
    assert_eq!(file.digest(), &digest(host));
    topology.downstream = Some(ProxyLink {
        implementation: ProxyImplementation::ReShade,
        path: host_path,
        receipt: FileReceipt::reused(file.identity(), file.digest().clone()).expect("receipt"),
    });
    topology.downstream_origin = Some(topology.root_slot.clone());
    topology
}

fn prepared(game_id: GameId, host: &[u8]) -> PreparedInstall {
    PreparedInstall {
        game_id,
        proxy_dll_name: "dxgi.dll".to_owned(),
        payload: vec![LumaPayloadFile {
            relative_path: "Luma.addon".to_owned(),
            bytes: b"addon".to_vec(),
        }],
        main_addon_rel: "Luma.addon".to_owned(),
        asset_source_url: "https://example.test/luma.zip".to_owned(),
        zip_digest: "a".repeat(64),
        source_etag: None,
        source_last_modified: None,
        build_label: Some("Build 1".to_owned()),
        reshade_dll_bytes: host.to_vec(),
        reshade_source_url: "https://example.test/reshade.zip".to_owned(),
        reshade_source_etag: None,
        reshade_last_modified: None,
        reshade_digest: digest(host).to_string(),
        dgvoodoo: None,
    }
}

fn prepared_with_payload(
    game_id: GameId,
    host: &[u8],
    payload: Vec<LumaPayloadFile>,
) -> PreparedInstall {
    let mut prepared = prepared(game_id, host);
    prepared.payload = payload;
    prepared
}

#[test]
fn fresh_active_install_returns_aligned_generic_and_host_program() {
    let root = tempfile::tempdir().expect("root");
    std::fs::write(root.path().join("dxgi.dll"), b"outer").expect("outer");
    let game_id = GameId::new("manual:active-install-test").expect("game id");
    let topology = topology(root.path(), &game_id);
    let authority = LumaPeerRootAuthority::resolve(root.path(), &root.path().join("dxgi.dll"))
        .expect("authority");
    let assessment = assess_topology_downstream_for_tool(
        root.path(),
        &root.path().join("ReShade64.dll"),
        "Luma",
        Some(&Version::parse("0").expect("version")),
    )
    .expect("assessment");
    let host_path = path(&root.path().join("ReShade64.dll"));
    let host_snapshot = observe_peer_path_snapshot(&host_path, &path(root.path())).expect("host");
    let claim = CatalogPathClaim::for_test(Vec::new(), None);

    let composition = compose_active_install(LumaActiveInstallInput {
        prepared: prepared(game_id, &host_bytes()),
        topology: &topology,
        authority: &authority,
        assessment: &assessment,
        host_path: &host_path,
        host_snapshot: &host_snapshot,
        catalog_claim: &claim,
        minimum_host_version: &Version::parse("0").expect("version"),
    })
    .expect("composition");

    assert_eq!(composition.record().kind(), AddonKind::Luma);
    assert_eq!(composition.program().endpoints().len(), 2);
    assert_eq!(composition.payloads().len(), 2);
    assert_eq!(
        composition.program().endpoints()[0].path(),
        composition.record().addon_file()
    );
    assert!(composition.record().reshade_channel().is_some());
}

#[test]
fn reused_host_never_observes_or_claims_a_foreign_sidecar() {
    let root = tempfile::tempdir().expect("root");
    std::fs::write(root.path().join("dxgi.dll"), b"outer").expect("outer");
    let host = host_bytes();
    std::fs::write(root.path().join("ReShade64.dll"), &host).expect("host");
    std::fs::write(root.path().join("ReShade64.dll.bak"), b"foreign").expect("foreign bak");
    let game_id = GameId::new("manual:active-install-reused-host").expect("game id");
    let topology = topology_with_host(root.path(), &game_id, &host);
    let authority = LumaPeerRootAuthority::resolve(root.path(), &root.path().join("dxgi.dll"))
        .expect("authority");
    let minimum = Version::parse("0").expect("version");
    let assessment = assess_topology_downstream_for_tool(
        root.path(),
        &root.path().join("ReShade64.dll"),
        "Luma",
        Some(&minimum),
    )
    .expect("assessment");
    let host_path = path(&root.path().join("ReShade64.dll"));
    let host_snapshot = observe_peer_path_snapshot(&host_path, &path(root.path())).expect("host");
    let mut prepared = prepared(game_id, &[]);
    prepared.reshade_digest.clear();
    prepared.reshade_source_url.clear();

    let composition = compose_active_install(LumaActiveInstallInput {
        prepared,
        topology: &topology,
        authority: &authority,
        assessment: &assessment,
        host_path: &host_path,
        host_snapshot: &host_snapshot,
        catalog_claim: &CatalogPathClaim::for_test(Vec::new(), None),
        minimum_host_version: &minimum,
    })
    .expect("reused host composition");
    assert_eq!(
        composition.record().managed_files()[0].mode(),
        ManagedFileMode::Reused
    );
    assert_eq!(composition.record().reshade_channel(), None);
    assert!(
        !composition
            .record()
            .tracked_sources()
            .iter()
            .any(|source| source.role() == TrackedSourceRole::HostBinary)
    );
    assert!(
        composition
            .program()
            .endpoints()
            .iter()
            .all(|endpoint| !endpoint.path().as_str().ends_with("ReShade64.dll"))
    );
}

#[test]
fn f4_host_replacement_emits_sidecar_before_the_host() {
    let root = tempfile::tempdir().expect("root");
    std::fs::write(root.path().join("dxgi.dll"), b"outer").expect("outer");
    let old_host = host_bytes();
    std::fs::write(root.path().join("ReShade64.dll"), &old_host).expect("old host");
    let game_id = GameId::new("manual:active-install-host-replace").expect("game id");
    let topology = topology_with_host(root.path(), &game_id, &old_host);
    let authority = LumaPeerRootAuthority::resolve(root.path(), &root.path().join("dxgi.dll"))
        .expect("authority");
    let minimum = Version::parse("999").expect("version");
    let assessment = assess_topology_downstream_for_tool(
        root.path(),
        &root.path().join("ReShade64.dll"),
        "Luma",
        Some(&minimum),
    )
    .expect("assessment");
    let host_path = path(&root.path().join("ReShade64.dll"));
    let host_snapshot = observe_peer_path_snapshot(&host_path, &path(root.path())).expect("host");
    let prepared_host = host_bytes_version([1000, 0, 0, 0]);
    let composition = compose_active_install(LumaActiveInstallInput {
        prepared: prepared(game_id, &prepared_host),
        topology: &topology,
        authority: &authority,
        assessment: &assessment,
        host_path: &host_path,
        host_snapshot: &host_snapshot,
        catalog_claim: &CatalogPathClaim::for_test(Vec::new(), None),
        minimum_host_version: &minimum,
    })
    .expect("host replacement composition");
    let endpoints = composition.program().endpoints();
    assert_eq!(endpoints.len(), 3);
    assert!(endpoints[0].path().as_str().ends_with("Luma.addon"));
    assert!(endpoints[1].path().as_str().ends_with("ReShade64.dll.bak"));
    assert!(endpoints[2].path().as_str().ends_with("ReShade64.dll"));
    assert_eq!(
        composition
            .record()
            .tracked_sources()
            .iter()
            .filter(|source| source.role() == TrackedSourceRole::HostBinary)
            .count(),
        1
    );
}

#[test]
fn owned_dlss_is_managed_and_disjoint_from_engine_lists() {
    let root = tempfile::tempdir().expect("root");
    std::fs::write(root.path().join("dxgi.dll"), b"outer").expect("outer");
    let game_id = GameId::new("manual:active-install-dlss-owned").expect("game id");
    let dlss = crate::addons::luma::test_support::build_nvidia_dlss_pe([3, 7, 0, 0]);
    let payload = vec![
        LumaPayloadFile {
            relative_path: "Luma.addon".to_owned(),
            bytes: b"addon".to_vec(),
        },
        LumaPayloadFile {
            relative_path: "nvngx_dlss.dll".to_owned(),
            bytes: dlss,
        },
    ];
    let topology = topology(root.path(), &game_id);
    let authority = LumaPeerRootAuthority::resolve(root.path(), &root.path().join("dxgi.dll"))
        .expect("authority");
    let minimum = Version::parse("0").expect("version");
    let assessment = assess_topology_downstream_for_tool(
        root.path(),
        &root.path().join("ReShade64.dll"),
        "Luma",
        Some(&minimum),
    )
    .expect("assessment");
    let host_path = path(&root.path().join("ReShade64.dll"));
    let host_snapshot = observe_peer_path_snapshot(&host_path, &path(root.path())).expect("host");
    let composition = compose_active_install(LumaActiveInstallInput {
        prepared: prepared_with_payload(game_id, &host_bytes(), payload),
        topology: &topology,
        authority: &authority,
        assessment: &assessment,
        host_path: &host_path,
        host_snapshot: &host_snapshot,
        catalog_claim: &CatalogPathClaim::for_test(Vec::new(), None),
        minimum_host_version: &minimum,
    })
    .expect("owned DLSS composition");
    let dlss_path = composition
        .record()
        .managed_files()
        .iter()
        .find(|binding| binding.path().as_str().ends_with("nvngx_dlss.dll"))
        .expect("DLSS binding")
        .path();
    let binding = composition
        .record()
        .managed_files()
        .iter()
        .find(|binding| binding.path() == dlss_path)
        .expect("DLSS binding");
    assert_eq!(binding.mode(), ManagedFileMode::Owned);
    assert!(!composition.record().created_files().contains(dlss_path));
    assert!(!composition.record().backed_up_files().contains(dlss_path));
    assert_eq!(composition.record().managed_files().len(), 2);
}

#[test]
fn reused_dlss_keeps_foreign_sidecar_out_of_the_program() {
    let root = tempfile::tempdir().expect("root");
    std::fs::write(root.path().join("dxgi.dll"), b"outer").expect("outer");
    let game_id = GameId::new("manual:active-install-dlss-reused").expect("game id");
    let live = crate::addons::luma::test_support::build_nvidia_dlss_pe([3, 8, 0, 0]);
    std::fs::write(root.path().join("nvngx_dlss.dll"), &live).expect("live DLSS");
    std::fs::write(root.path().join("nvngx_dlss.dll.bak"), b"foreign").expect("foreign bak");
    let payload = vec![
        LumaPayloadFile {
            relative_path: "Luma.addon".to_owned(),
            bytes: b"addon".to_vec(),
        },
        LumaPayloadFile {
            relative_path: "nvngx_dlss.dll".to_owned(),
            bytes: crate::addons::luma::test_support::build_nvidia_dlss_pe([3, 7, 0, 0]),
        },
    ];
    let topology = topology(root.path(), &game_id);
    let authority = LumaPeerRootAuthority::resolve(root.path(), &root.path().join("dxgi.dll"))
        .expect("authority");
    let minimum = Version::parse("0").expect("version");
    let assessment = assess_topology_downstream_for_tool(
        root.path(),
        &root.path().join("ReShade64.dll"),
        "Luma",
        Some(&minimum),
    )
    .expect("assessment");
    let host_path = path(&root.path().join("ReShade64.dll"));
    let host_snapshot = observe_peer_path_snapshot(&host_path, &path(root.path())).expect("host");
    let composition = compose_active_install(LumaActiveInstallInput {
        prepared: prepared_with_payload(game_id, &host_bytes(), payload),
        topology: &topology,
        authority: &authority,
        assessment: &assessment,
        host_path: &host_path,
        host_snapshot: &host_snapshot,
        catalog_claim: &CatalogPathClaim::for_test(Vec::new(), None),
        minimum_host_version: &minimum,
    })
    .expect("reused DLSS composition");
    let dlss_path = composition
        .record()
        .managed_files()
        .iter()
        .find(|binding| binding.path().as_str().ends_with("nvngx_dlss.dll"))
        .expect("DLSS binding")
        .path();
    assert_eq!(
        composition
            .record()
            .managed_files()
            .iter()
            .find(|binding| binding.path() == dlss_path)
            .expect("DLSS binding")
            .mode(),
        ManagedFileMode::Reused
    );
    assert!(
        composition
            .program()
            .endpoints()
            .iter()
            .all(|endpoint| endpoint.path() != dlss_path)
    );
}

#[test]
fn changed_dgvoodoo_config_projects_the_live_path_in_both_engine_lists() {
    let root = tempfile::tempdir().expect("root");
    std::fs::write(root.path().join("dxgi.dll"), b"outer").expect("outer");
    let original = "[General]\r\nOutputAPI = d3d9\r\n";
    std::fs::write(root.path().join("dgVoodoo.conf"), original).expect("config");
    let game_id = GameId::new("manual:active-install-dgvoodoo").expect("game id");
    let topology = topology(root.path(), &game_id);
    let authority = LumaPeerRootAuthority::resolve(root.path(), &root.path().join("dxgi.dll"))
        .expect("authority");
    let minimum = Version::parse("0").expect("version");
    let assessment = assess_topology_downstream_for_tool(
        root.path(),
        &root.path().join("ReShade64.dll"),
        "Luma",
        Some(&minimum),
    )
    .expect("assessment");
    let host_path = path(&root.path().join("ReShade64.dll"));
    let host_snapshot = observe_peer_path_snapshot(&host_path, &path(root.path())).expect("host");
    let mut prepared = prepared(game_id, &host_bytes());
    prepared.dgvoodoo = Some(DgVoodooInstall::Reused(ReusedDgVoodoo {
        config_file: "dgVoodoo.conf".to_owned(),
        config_default: "[General]\r\nOutputAPI = d3d11_fl11_0\r\n".to_owned(),
        config_sections: vec![crate::addons::engine::IniSection {
            name: "General".to_owned(),
            keys: vec![("OutputAPI".to_owned(), "d3d11_fl11_0".to_owned())],
        }],
    }));
    let composition = compose_active_install(LumaActiveInstallInput {
        prepared,
        topology: &topology,
        authority: &authority,
        assessment: &assessment,
        host_path: &host_path,
        host_snapshot: &host_snapshot,
        catalog_claim: &CatalogPathClaim::for_test(Vec::new(), None),
        minimum_host_version: &minimum,
    })
    .expect("dgVoodoo composition");
    assert!(
        composition
            .record()
            .created_files()
            .iter()
            .any(|path| path.as_str().ends_with("dgVoodoo.conf"))
    );
    assert!(
        composition
            .record()
            .backed_up_files()
            .iter()
            .any(|path| path.as_str().ends_with("dgVoodoo.conf"))
    );
    assert!(
        composition
            .program()
            .endpoints()
            .iter()
            .any(|endpoint| endpoint.path().as_str().ends_with("dgVoodoo.conf.bak"))
    );
}

#[test]
fn deterministic_effect_groups_put_generic_dgvoodoo_dlss_and_host_in_order() {
    let root = tempfile::tempdir().expect("root");
    std::fs::write(root.path().join("dxgi.dll"), b"outer").expect("outer");
    std::fs::write(
        root.path().join("dgVoodoo.conf"),
        "[General]\r\nOutputAPI = d3d9\r\n",
    )
    .expect("config");
    let game_id = GameId::new("manual:active-install-order").expect("game id");
    let payload = vec![
        LumaPayloadFile {
            relative_path: "Luma.addon".to_owned(),
            bytes: b"addon".to_vec(),
        },
        LumaPayloadFile {
            relative_path: "nvngx_dlss.dll".to_owned(),
            bytes: crate::addons::luma::test_support::build_nvidia_dlss_pe([3, 7, 0, 0]),
        },
    ];
    let topology = topology(root.path(), &game_id);
    let authority = LumaPeerRootAuthority::resolve(root.path(), &root.path().join("dxgi.dll"))
        .expect("authority");
    let minimum = Version::parse("0").expect("version");
    let assessment = assess_topology_downstream_for_tool(
        root.path(),
        &root.path().join("ReShade64.dll"),
        "Luma",
        Some(&minimum),
    )
    .expect("assessment");
    let host_path = path(&root.path().join("ReShade64.dll"));
    let host_snapshot = observe_peer_path_snapshot(&host_path, &path(root.path())).expect("host");
    let mut prepared = prepared_with_payload(game_id, &host_bytes(), payload);
    prepared.dgvoodoo = Some(DgVoodooInstall::Reused(ReusedDgVoodoo {
        config_file: "dgVoodoo.conf".to_owned(),
        config_default: "[General]\r\nOutputAPI = d3d11_fl11_0\r\n".to_owned(),
        config_sections: vec![crate::addons::engine::IniSection {
            name: "General".to_owned(),
            keys: vec![("OutputAPI".to_owned(), "d3d11_fl11_0".to_owned())],
        }],
    }));
    let composition = compose_active_install(LumaActiveInstallInput {
        prepared,
        topology: &topology,
        authority: &authority,
        assessment: &assessment,
        host_path: &host_path,
        host_snapshot: &host_snapshot,
        catalog_claim: &CatalogPathClaim::for_test(Vec::new(), None),
        minimum_host_version: &minimum,
    })
    .expect("ordered composition");
    let names = composition
        .program()
        .endpoints()
        .iter()
        .map(|endpoint| endpoint.path().file_name().expect("name"))
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        vec![
            "Luma.addon",
            "dgVoodoo.conf.bak",
            "dgVoodoo.conf",
            "nvngx_dlss.dll",
            "ReShade64.dll",
        ]
    );
}

#[test]
fn malformed_addon_provenance_fails_before_payload_observation() {
    let root = tempfile::tempdir().expect("root");
    std::fs::write(root.path().join("dxgi.dll"), b"outer").expect("outer");
    let game_id = GameId::new("manual:active-install-provenance").expect("game id");
    let topology = topology(root.path(), &game_id);
    let authority = LumaPeerRootAuthority::resolve(root.path(), &root.path().join("dxgi.dll"))
        .expect("authority");
    let minimum = Version::parse("0").expect("version");
    let assessment = assess_topology_downstream_for_tool(
        root.path(),
        &root.path().join("ReShade64.dll"),
        "Luma",
        Some(&minimum),
    )
    .expect("assessment");
    let host_path = path(&root.path().join("ReShade64.dll"));
    let host_snapshot = observe_peer_path_snapshot(&host_path, &path(root.path())).expect("host");
    let mut prepared = prepared(game_id, &host_bytes());
    prepared.zip_digest = "not-a-digest".to_owned();
    prepared.payload[0].relative_path = "missing/unsafe.addon".to_owned();
    let error = compose_active_install(LumaActiveInstallInput {
        prepared,
        topology: &topology,
        authority: &authority,
        assessment: &assessment,
        host_path: &host_path,
        host_snapshot: &host_snapshot,
        catalog_claim: &CatalogPathClaim::for_test(Vec::new(), None),
        minimum_host_version: &minimum,
    })
    .expect_err("malformed provenance");
    assert!(error.to_string().contains("ZIP digest"));
}
