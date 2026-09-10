use std::path::PathBuf;

use renderpilot_domain::{
    FileOwnership, FileReceipt, GameProxyTopology, PathRef, ProxyImplementation, ProxyLink,
    ProxyRootPrestate,
};

use super::super::snapshot_active_update;
use super::super::{validate_downstream_snapshot, validate_topology};
use super::{game_id, owned_receipt, path, record, target, target_at};
use crate::addons::luma::test_support::{
    MACHINE_AMD64, PE32_PLUS_MAGIC, build_nvidia_dlss_pe, build_pe_with_exports, manifest,
};
use crate::peer_mutation_executor::{PeerPathSnapshot, observe_peer_path_snapshot};

fn topology(root: &str, downstream: &str, ownership: FileOwnership) -> GameProxyTopology {
    let root = path(root);
    GameProxyTopology {
        id: "snapshot-topology".to_owned(),
        game_id: game_id(),
        root_slot: root.clone(),
        outer: ProxyLink {
            implementation: ProxyImplementation::OptiScaler,
            path: root.clone(),
            receipt: owned_receipt("outer", 'a'),
        },
        downstream: Some(ProxyLink {
            implementation: ProxyImplementation::ReShade,
            path: path(downstream),
            receipt: match ownership {
                FileOwnership::Owned => owned_receipt("downstream", 'b'),
                FileOwnership::Reused => super::reused_receipt("downstream", 'b'),
            },
        }),
        downstream_origin: Some(root),
        root_prestate: ProxyRootPrestate::Absent,
    }
}

#[test]
fn both_owned_and_reused_reshade_hosts_are_valid_update_membership() {
    let root = PathBuf::from("C:/Games/Snapshot");
    for ownership in [FileOwnership::Owned, FileOwnership::Reused] {
        validate_topology(
            &game_id(),
            &root,
            &topology(
                "C:/Games/Snapshot/dxgi.dll",
                "C:/Games/Snapshot/ReShade64.dll",
                ownership,
            ),
        )
        .expect("valid host membership");
    }
}

#[test]
fn wrong_game_outer_root_and_downstream_are_rejected() {
    let root = PathBuf::from("C:/Games/Snapshot");
    let mut wrong_game = topology(
        "C:/Games/Snapshot/dxgi.dll",
        "C:/Games/Snapshot/ReShade64.dll",
        FileOwnership::Reused,
    );
    wrong_game.game_id = renderpilot_domain::GameId::new("other-game").expect("game");
    assert!(validate_topology(&game_id(), &root, &wrong_game).is_err());

    let mut wrong_outer = topology(
        "C:/Games/Snapshot/dxgi.dll",
        "C:/Games/Snapshot/ReShade64.dll",
        FileOwnership::Reused,
    );
    wrong_outer.outer.implementation = ProxyImplementation::ReShade;
    assert!(validate_topology(&game_id(), &root, &wrong_outer).is_err());

    let outside_root = topology(
        "C:/Games/Other/dxgi.dll",
        "C:/Games/Snapshot/ReShade64.dll",
        FileOwnership::Reused,
    );
    assert!(validate_topology(&game_id(), &root, &outside_root).is_err());

    let outside_downstream = topology(
        "C:/Games/Snapshot/dxgi.dll",
        "C:/Games/Other/ReShade64.dll",
        FileOwnership::Reused,
    );
    assert!(validate_topology(&game_id(), &root, &outside_downstream).is_err());
}

#[test]
fn missing_downstream_and_absent_observation_fail_closed() {
    let mut missing = topology(
        "C:/Games/Snapshot/dxgi.dll",
        "C:/Games/Snapshot/ReShade64.dll",
        FileOwnership::Reused,
    );
    missing.downstream = None;
    missing.downstream_origin = None;
    assert!(
        validate_topology(
            &game_id(),
            PathBuf::from("C:/Games/Snapshot").as_path(),
            &missing
        )
        .is_err()
    );

    let mut absent = topology(
        "C:/Games/Snapshot/dxgi.dll",
        "C:/Games/Snapshot/ReShade64.dll",
        FileOwnership::Reused,
    );
    absent.downstream.as_mut().expect("downstream").receipt =
        super::owned_receipt("different", 'c');
    assert!(validate_downstream_snapshot(&absent, &PeerPathSnapshot::Absent).is_err());
}

#[test]
fn observed_receipt_must_match_exactly() {
    let root = tempfile::tempdir().expect("root");
    let downstream = root.path().join("ReShade64.dll");
    std::fs::write(&downstream, b"reshade host bytes").expect("host");
    let downstream_ref = PathRef::new(downstream.to_string_lossy().into_owned()).expect("path");
    let root_ref = PathRef::new(root.path().to_string_lossy().into_owned()).expect("root");
    let observed = observe_peer_path_snapshot(&downstream_ref, &root_ref).expect("observe");
    let file = observed.file().expect("file");
    let mut valid = topology(
        &format!("{}/dxgi.dll", root.path().display()),
        &downstream.to_string_lossy(),
        FileOwnership::Reused,
    );
    valid.downstream.as_mut().expect("downstream").receipt =
        FileReceipt::reused(file.identity(), file.digest().clone()).expect("receipt");
    assert!(validate_downstream_snapshot(&valid, &observed).is_ok());

    let mut drifted = valid;
    drifted.downstream.as_mut().expect("downstream").receipt = FileReceipt::reused(
        file.identity(),
        renderpilot_domain::Sha256Hash::new("c".repeat(64)).expect("digest"),
    )
    .expect("receipt");
    assert!(validate_downstream_snapshot(&drifted, &observed).is_err());
}

fn reshade_host_bytes(version: [u16; 4]) -> Vec<u8> {
    let exported = build_pe_with_exports(
        MACHINE_AMD64,
        PE32_PLUS_MAGIC,
        &[
            "ReShadeVersion",
            "ReShadeRegisterAddon",
            "ReShadeUnregisterAddon",
            "ReShadeRegisterEvent",
        ],
    );
    let versioned = build_nvidia_dlss_pe(version);
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

#[test]
fn valid_active_snapshot_carries_exact_observation_and_local_facts() {
    let root = tempfile::tempdir().expect("root");
    let host_path = root.path().join("ReShade64.dll");
    let host_bytes = reshade_host_bytes([6, 7, 0, 0]);
    std::fs::write(&host_path, &host_bytes).expect("host");
    let root_ref = PathRef::new(root.path().to_string_lossy().into_owned()).expect("root");
    let host_ref = PathRef::new(host_path.to_string_lossy().into_owned()).expect("host path");
    let observed = observe_peer_path_snapshot(&host_ref, &root_ref).expect("observe");
    let file = observed.file().expect("host file");
    let mut topology = topology(
        &root.path().join("dxgi.dll").to_string_lossy(),
        &host_path.to_string_lossy(),
        FileOwnership::Reused,
    );
    topology.downstream.as_mut().expect("downstream").receipt =
        FileReceipt::reused(file.identity(), file.digest().clone()).expect("receipt");

    let snapshot = snapshot_active_update(
        &manifest(Vec::new()),
        &game_id(),
        record(),
        target_at(root.path().to_path_buf(), None),
        topology,
        root_ref.clone(),
    )
    .expect("active snapshot");
    assert_eq!(snapshot.downstream_path().as_str(), host_ref.as_str());
    assert_eq!(snapshot.downstream_snapshot(), &observed);
    assert_eq!(snapshot.minimum_reshade_version().as_str(), "6.7.0");
    assert!(!snapshot.host_replacement_required());
    assert!(!snapshot.payload_disk_intact());
    assert_eq!(snapshot.stored_game_install_path(), &root_ref);
}

#[test]
fn snapshot_fixture_keeps_payload_fact_as_observation_not_rejection() {
    let record = record();
    assert!(!crate::addons::luma::tracking::payload_disk_intact(&record));
    let _ = target(None);
}
