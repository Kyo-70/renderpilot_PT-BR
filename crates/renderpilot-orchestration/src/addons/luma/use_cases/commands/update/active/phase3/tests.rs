use std::path::PathBuf;

use renderpilot_domain::{
    AddonKind, Architecture, FileReceipt, GameId, GameProxyTopology, InstalledAddon,
    ManagedAddonFile, ManagedFileBaseline, PathRef, ProxyImplementation, ProxyLink,
    ProxyRootPrestate, Version,
};

use super::super::super::route::{InactiveUpdatePhase1, UpdatePhase1};
use super::super::model::{ActiveUpdatePhase1, DgVoodooLocalDecision};
use super::*;
use crate::addons::luma::fetch::types::{LumaPayload, LumaPayloadFile};
use crate::addons::luma::peer::LumaPeerRootAuthority;
use crate::addons::luma::peer::{
    LumaActiveUpdateDgVoodooInput, LumaActiveUpdateHostInput, LumaActiveUpdatePayloadInput,
    LumaActiveUpdatePrepared,
};

#[path = "storage.rs"]
mod storage;

fn id() -> GameId {
    GameId::new("phase3-test-game").expect("game id")
}

fn path(value: impl Into<String>) -> PathRef {
    PathRef::new(value.into()).expect("path")
}

fn hash(byte: char) -> renderpilot_domain::Sha256Hash {
    renderpilot_domain::Sha256Hash::new(byte.to_string().repeat(64)).expect("hash")
}

fn record() -> InstalledAddon {
    InstalledAddon::new(id(), AddonKind::Luma, path("C:/Games/Phase3/Luma.addon64"))
}

fn phase1(record: InstalledAddon) -> ActiveUpdatePhase1 {
    let root = path("C:/Games/Phase3");
    let outer = path("C:/Games/Phase3/dxgi.dll");
    let downstream = path("C:/Games/Phase3/ReShade64.dll");
    ActiveUpdatePhase1 {
        record,
        topology: GameProxyTopology {
            id: "phase3-topology".to_owned(),
            game_id: id(),
            root_slot: outer.clone(),
            outer: ProxyLink {
                implementation: ProxyImplementation::OptiScaler,
                path: outer.clone(),
                receipt: FileReceipt::owned("outer", hash('a')).expect("outer receipt"),
            },
            downstream: Some(ProxyLink {
                implementation: ProxyImplementation::ReShade,
                path: downstream.clone(),
                receipt: FileReceipt::reused("host", hash('b')).expect("host receipt"),
            }),
            downstream_origin: Some(outer),
            root_prestate: ProxyRootPrestate::Absent,
        },
        target: crate::addons::luma::use_cases::update_target::ResolvedUpdateTarget {
            game_dir: PathBuf::from(root.as_str()),
            asset: "Luma.zip".to_owned(),
            addon_file: "Luma.addon64".to_owned(),
            arch: Architecture::X64,
            proxy_dll_name: "dxgi.dll".to_owned(),
            external_requirement: None,
        },
        stored_game_install_path: root.clone(),
        canonical_game_root: PathBuf::from(root.as_str()),
        downstream_path: downstream,
        downstream_snapshot: crate::peer_mutation_executor::PeerPathSnapshot::Absent,
        minimum_reshade_version: Version::parse("6.7.0").expect("version"),
        had_torn_marker: false,
        payload_disk_intact: true,
        dependency_paths: Vec::new(),
        dgvoodoo: DgVoodooLocalDecision::Preserve {
            config_owned: false,
        },
        host_replacement_required: false,
    }
}

fn prepared(payload: LumaActiveUpdatePayloadInput) -> LumaActiveUpdatePrepared {
    LumaActiveUpdatePrepared::new(
        payload,
        LumaActiveUpdateHostInput::Preserve,
        LumaActiveUpdateDgVoodooInput::Preserve,
        Vec::new(),
        Vec::new(),
        None,
    )
}

fn full_payload(files: &[&str]) -> LumaActiveUpdatePayloadInput {
    LumaActiveUpdatePayloadInput::Full(LumaPayload {
        files: files
            .iter()
            .map(|relative_path| LumaPayloadFile {
                relative_path: (*relative_path).to_owned(),
                bytes: Vec::new(),
            })
            .collect(),
        main_addon_rel: "Luma.addon64".to_owned(),
        zip_digest: "zip".to_owned(),
        etag: None,
        last_modified: None,
        build_number: None,
    })
}

fn owned_record(mode: renderpilot_domain::ManagedFileMode) -> InstalledAddon {
    record()
        .try_with_managed_files(vec![match mode {
            renderpilot_domain::ManagedFileMode::Reused => {
                ManagedAddonFile::reused(path("C:/Games/Phase3/nvngx_dlss.dll"), hash('c'))
            }
            renderpilot_domain::ManagedFileMode::Owned => ManagedAddonFile::owned(
                path("C:/Games/Phase3/nvngx_dlss.dll"),
                ManagedFileBaseline::Absent,
                hash('c'),
            ),
        }])
        .expect("managed binding")
}

#[test]
fn phase_one_route_must_remain_active_and_exactly_equal() {
    let expected = phase1(record());
    let inactive = UpdatePhase1::Inactive(Box::new(InactiveUpdatePhase1 {
        record: record(),
        had_torn_marker: false,
    }));
    assert!(require_unchanged_active(inactive, &expected).is_err());

    let mut changed = phase1(record());
    changed.host_replacement_required = true;
    assert!(require_unchanged_active(UpdatePhase1::Active(Box::new(changed)), &expected).is_err());
}

#[test]
fn cascade_selector_releases_only_owned_dlss_when_full_payload_omits_it() {
    let target = path("C:/Games/Phase3/nvngx_dlss.dll");
    let owned = phase1(owned_record(renderpilot_domain::ManagedFileMode::Owned));
    assert_eq!(
        cascade_owned_paths(&owned, &prepared(full_payload(&["Luma.addon64"])), &target)
            .expect("selector"),
        vec![PathBuf::from(target.as_str())]
    );

    assert!(
        cascade_owned_paths(
            &owned,
            &prepared(LumaActiveUpdatePayloadInput::Preserve),
            &target
        )
        .expect("selector")
        .is_empty()
    );
    assert!(
        cascade_owned_paths(
            &owned,
            &prepared(full_payload(&["Luma.addon64", "nvngx_dlss.dll"])),
            &target,
        )
        .expect("selector")
        .is_empty()
    );
}

#[test]
fn cascade_selector_never_releases_reused_or_absent_dlss() {
    let target = path("C:/Games/Phase3/nvngx_dlss.dll");
    for record in [
        record(),
        owned_record(renderpilot_domain::ManagedFileMode::Reused),
    ] {
        assert!(
            cascade_owned_paths(
                &phase1(record),
                &prepared(full_payload(&["Luma.addon64"])),
                &target
            )
            .expect("selector")
            .is_empty()
        );
    }
}

#[test]
fn cascade_selector_never_expands_beyond_the_effective_dlss_target() {
    let target = path("C:/Games/Phase3/nvngx_dlss.dll");
    let owned = phase1(owned_record(renderpilot_domain::ManagedFileMode::Owned));
    let nested = cascade_owned_paths(
        &owned,
        &prepared(full_payload(&["Luma.addon64", "nested/nvngx_dlss.dll"])),
        &target,
    )
    .expect("selector");
    assert_eq!(nested, vec![PathBuf::from(target.as_str())]);

    let duplicate = cascade_owned_paths(
        &owned,
        &prepared(full_payload(&[
            "Luma.addon64",
            "nvngx_dlss.dll",
            "NVNGX_DLSS.DLL",
        ])),
        &target,
    )
    .expect("selector");
    assert!(duplicate.is_empty());
}

#[test]
fn authority_rejects_persisted_addon_path_drift() {
    let game = tempfile::tempdir().expect("game");
    let payload = tempfile::tempdir().expect("payload");
    std::fs::write(
        game.path().join("ReShade.ini"),
        format!("[ADDON]\r\nAddonPath={}\r\n", payload.path().display()),
    )
    .expect("config");
    let authority = LumaPeerRootAuthority::resolve(game.path(), &game.path().join("dxgi.dll"))
        .expect("authority");
    let drifted = InstalledAddon::new(
        id(),
        AddonKind::Luma,
        PathRef::new(game.path().join("Luma.addon64").to_string_lossy()).expect("path"),
    );
    assert!(authority.validate(Some(&drifted), None, None, &[]).is_err());
}

#[test]
fn authority_builds_the_exact_effective_dlss_target() {
    let game = tempfile::tempdir().expect("game");
    let authority = LumaPeerRootAuthority::resolve(game.path(), &game.path().join("dxgi.dll"))
        .expect("authority");
    let expected = authority
        .canonical_game_root()
        .join(renderpilot_detection::NVNGX_DLSS_FILE_NAME);
    assert_eq!(
        authority.effective_dlss_target().expect("target"),
        PathRef::from_canonical_native_absolute(&expected).expect("path")
    );
}

#[cfg(windows)]
#[test]
fn authority_rejects_non_utf8_effective_target_without_lossy_conversion() {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;

    let game = tempfile::tempdir().expect("game");
    let invalid = game.path().join(OsString::from_wide(&[0xd800]));
    assert!(LumaPeerRootAuthority::resolve(game.path(), &invalid).is_err());
}
