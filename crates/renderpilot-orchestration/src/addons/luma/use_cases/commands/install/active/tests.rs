#[cfg(windows)]
use std::path::Path;

#[cfg(windows)]
use super::*;
#[cfg(windows)]
use crate::Context;
#[cfg(windows)]
use crate::addons::engine;
#[cfg(windows)]
use crate::addons::luma::test_support::{
    MACHINE_AMD64, PE32_PLUS_MAGIC, build_pe_with_exports, manifest,
};
#[cfg(windows)]
use renderpilot_application::{GameRepository, ProxyTopologyRepository};
#[cfg(windows)]
use renderpilot_domain::{
    FileOwnership, FileReceipt, GameId, GameIdentity, GameInstallation, GameProxyTopology,
    GameRuntime, Launcher, ManagedFileMode, OptiScalerAdoptionState,
    OptiScalerConfigurationBaseline, OptiScalerFileCleanup, OptiScalerFileReceipt,
    OptiScalerFileRole, OptiScalerInstallStateParts, PathRef, Platform, ProxyImplementation,
    ProxyLink, ProxyRootPrestate,
};
#[cfg(windows)]
use renderpilot_storage_sqlite::{
    GameMutationCommit, InstalledAddonMutation, OptiScalerAggregateMutation,
};
#[cfg(windows)]
use tempfile::tempdir;

#[cfg(windows)]
fn seed_game(context: &Context, game_id: &GameId, appid: &str, game_dir: &Path, exe_path: &Path) {
    let identity = GameIdentity::new(game_id.clone(), "Dishonored 2", Launcher::Steam)
        .expect("identity")
        .with_external_id(appid)
        .expect("external id");
    let game = GameInstallation::new(
        identity,
        Platform::Windows,
        GameRuntime::NativeWindows,
        PathRef::new(game_dir.to_string_lossy().replace('\\', "/")).expect("install path"),
    )
    .with_executable_candidate(
        PathRef::new(exe_path.to_string_lossy().replace('\\', "/")).expect("exe path"),
    );
    context.storage().upsert_game(&game).expect("seed game");
}

#[cfg(windows)]
fn write_stub_exe(path: &Path) {
    std::fs::write(
        path,
        build_pe_with_exports(MACHINE_AMD64, PE32_PLUS_MAGIC, &[]),
    )
    .expect("write exe");
}

#[cfg(windows)]
fn seed_active_topology(context: &Context, game_id: &GameId, root: &Path) {
    seed_active_topology_with_host(context, game_id, root, None);
}

#[cfg(windows)]
fn seed_active_topology_with_host(
    context: &Context,
    game_id: &GameId,
    root: &Path,
    reshade_host: Option<&[u8]>,
) {
    let proxy = root.join("dxgi.dll");
    let config = root.join("OptiScaler.ini");
    let executable = root.join("Dishonored2.exe");
    std::fs::write(&proxy, b"outer").expect("proxy");
    std::fs::write(&config, b"[OptiScaler]\n").expect("config");
    if let Some(bytes) = reshade_host {
        std::fs::write(root.join("ReShade64.dll"), bytes).expect("ReShade host");
    }

    let root_slot = PathRef::new(proxy.to_string_lossy().into_owned()).expect("root slot");
    let game_root = PathRef::new(root.to_string_lossy().into_owned()).expect("game root");
    let outer_snapshot =
        crate::peer_mutation_executor::observe_peer_path_snapshot(&root_slot, &game_root)
            .expect("outer snapshot");
    let outer_file = outer_snapshot.file().expect("outer file");
    let config_path = PathRef::new(config.to_string_lossy().into_owned()).expect("config path");
    let config_snapshot =
        crate::peer_mutation_executor::observe_peer_path_snapshot(&config_path, &game_root)
            .expect("config snapshot");
    let config_file = config_snapshot.file().expect("config file");
    let config_receipt = FileReceipt::reused(config_file.identity(), config_file.digest().clone())
        .expect("config receipt");
    let topology_id = format!("optiscaler:active-luma-install:{}", game_id.as_str());
    let downstream = reshade_host.map(|bytes| {
        let host_path = PathRef::new(root.join("ReShade64.dll").to_string_lossy().into_owned())
            .expect("host path");
        let snapshot =
            crate::peer_mutation_executor::observe_peer_path_snapshot(&host_path, &game_root)
                .expect("host snapshot");
        let file = snapshot.file().expect("host file");
        assert_eq!(
            file.digest(),
            &renderpilot_detection::sha256_bytes(bytes).expect("digest")
        );
        ProxyLink {
            implementation: ProxyImplementation::ReShade,
            path: host_path,
            receipt: FileReceipt::reused(file.identity(), file.digest().clone())
                .expect("host receipt"),
        }
    });
    let topology = GameProxyTopology {
        id: topology_id.clone(),
        game_id: game_id.clone(),
        root_slot: root_slot.clone(),
        outer: ProxyLink {
            implementation: ProxyImplementation::OptiScaler,
            path: root_slot.clone(),
            receipt: FileReceipt::reused(outer_file.identity(), outer_file.digest().clone())
                .expect("outer receipt"),
        },
        downstream_origin: downstream.as_ref().map(|_| root_slot),
        downstream,
        root_prestate: ProxyRootPrestate::Absent,
    };
    let state = renderpilot_domain::from_persisted(
        OptiScalerInstallStateParts {
            game_id: game_id.clone(),
            release_id: "active-luma-install".to_owned(),
            manifest_revision: "active-luma-install".to_owned(),
            archive_sha256: None,
            source: None,
            target_exe_path: PathRef::new(executable.to_string_lossy().into_owned()).expect("exe"),
            target_dir: PathRef::new(root.to_string_lossy().into_owned()).expect("target"),
            modules: vec!["core".to_owned()],
            release_files: vec![OptiScalerFileReceipt {
                path: config_path,
                installed: config_receipt.clone(),
                role: OptiScalerFileRole::Configuration,
                cleanup: OptiScalerFileCleanup::PreserveUnchanged,
                baseline: renderpilot_domain::OptiScalerReleaseFileBaseline::Absent,
            }],
            runtime_bindings: Vec::new(),
            directory_receipts: Vec::new(),
            proxy_topology_id: Some(topology_id),
            config_schema: 1,
            config_base_release: "active-luma-install".to_owned(),
            adoption_state: OptiScalerAdoptionState::AdoptedExact,
            prerequisite_binding: renderpilot_domain::OptiScalerPrerequisiteBinding::None,
            created_at: None,
            updated_at: None,
        },
        OptiScalerConfigurationBaseline::present(config_receipt, b"[OptiScaler]\n".to_vec())
            .expect("configuration baseline"),
    )
    .expect("state");
    context
        .storage()
        .commit_game_mutation(GameMutationCommit {
            game_id,
            component_set: None,
            baseline_mutations: &[],
            addon: InstalledAddonMutation::OptiScaler(
                OptiScalerAggregateMutation::AdoptExactMetadata {
                    state: &state,
                    topology: &topology,
                },
            ),
            mutation_id: None,
        })
        .expect("active topology");
}

#[cfg(windows)]
fn game_safety(context: &Context, game_id: &GameId) -> crate::GameSafetyPermit {
    let authority = crate::FileSafetyAuthority::new();
    let assessment = authority
        .issue_game_assessment(context, game_id)
        .expect("assessment");
    authority
        .game_permit(game_id.clone(), Some(&assessment.context_token))
        .expect("permit")
}

#[cfg(windows)]
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

#[tokio::test]
#[cfg(windows)]
async fn active_install_commits_local_prepared_payload_through_the_real_peer_transaction() {
    let db_dir = tempdir().expect("db dir");
    let game_dir = tempdir().expect("game dir");
    let context = Context::open_at(db_dir.path().join("catalog.sqlite")).expect("context");
    let game_id = GameId::new("steam:active-luma-success").expect("game id");
    let exe_path = game_dir.path().join("Dishonored2.exe");
    write_stub_exe(&exe_path);
    seed_game(
        &context,
        &game_id,
        "active-luma-success",
        game_dir.path(),
        &exe_path,
    );

    let reshade_host = reshade_host_bytes([6, 7, 0, 0]);
    seed_active_topology_with_host(&context, &game_id, game_dir.path(), Some(&reshade_host));
    let topology = context
        .storage()
        .get_proxy_topology(&game_id)
        .expect("topology")
        .expect("active topology");
    assert_eq!(
        topology
            .downstream
            .as_ref()
            .expect("reused downstream")
            .receipt
            .ownership(),
        FileOwnership::Reused
    );

    let manifest = manifest(vec![crate::addons::luma::test_support::title(
        "active-luma-success",
        "Luma.zip",
        renderpilot_domain::Architecture::X64,
        crate::addons::matching::Status::Working,
        vec![crate::addons::luma::test_support::rule(
            crate::addons::matching::MatchKind::SteamAppid,
            "active-luma-success",
            100,
        )],
    )]);
    let phase1 = {
        let _guard =
            crate::mutation_boundary::enter_game_mutation_boundary_async(&context, &game_id)
                .await
                .expect("phase-1 lock");
        super::phase::resolve_phase1(&context, &manifest, &game_id, &topology).expect("phase 1")
    };
    assert!(!phase1.snapshot.writes_host);

    let payload = b"local active Luma payload".to_vec();
    let prepared = crate::addons::luma::install::PreparedInstall {
        game_id: game_id.clone(),
        proxy_dll_name: "dxgi.dll".to_owned(),
        payload: vec![crate::addons::luma::fetch::types::LumaPayloadFile {
            relative_path: "Luma.addon".to_owned(),
            bytes: payload.clone(),
        }],
        main_addon_rel: "Luma.addon".to_owned(),
        asset_source_url: "https://example.test/luma.zip".to_owned(),
        zip_digest: "a".repeat(64),
        source_etag: None,
        source_last_modified: None,
        build_label: Some("Build local".to_owned()),
        reshade_dll_bytes: Vec::new(),
        reshade_source_url: String::new(),
        reshade_source_etag: None,
        reshade_last_modified: None,
        reshade_digest: String::new(),
        dgvoodoo: None,
    };
    let safety = game_safety(&context, &game_id);
    let returned = super::commit_prepared(
        &context, &manifest, &game_id, &safety, None, &phase1, prepared,
    )
    .await
    .expect("active install commit");

    let addon_path = std::path::PathBuf::from(returned.addon_file().as_str());
    assert_eq!(std::fs::read(&addon_path).expect("payload"), payload);
    assert!(returned.created_files().contains(returned.addon_file()));
    assert_eq!(returned.created_files().len(), 1);
    assert!(returned.backed_up_files().is_empty());
    assert!(!returned.created_files().iter().any(|path| {
        path.as_str()
            .eq_ignore_ascii_case(&game_dir.path().join("ReShade64.dll").to_string_lossy())
    }));
    assert!(
        returned.managed_files().iter().any(|managed| managed
            .path()
            .as_str()
            .ends_with("ReShade64.dll")
            && managed.mode() == ManagedFileMode::Reused)
    );
    assert_eq!(returned.managed_files().len(), 1);
    assert!(!game_dir.path().join("Luma.addon.bak").exists());
    assert!(!game_dir.path().join("ReShade64.dll.bak").exists());
    assert!(
        !game_dir
            .path()
            .join("renderpilot-luma-install.lock")
            .exists()
    );
    assert!(!engine::is_install_torn(game_dir.path(), AddonKind::Luma));

    let persisted = crate::addons::records::record_of_kind(&context, &game_id, AddonKind::Luma)
        .expect("persisted record")
        .expect("Luma record");
    assert!(persisted.installed_at().is_some());
    assert!(persisted.updated_at().is_some());
    assert_eq!(
        persisted,
        returned.with_timestamps(persisted.installed_at(), persisted.updated_at(),)
    );
    assert_eq!(
        context
            .storage()
            .get_proxy_topology(&game_id)
            .expect("topology after")
            .expect("topology after value"),
        topology,
        "active install must preserve the OptiScaler/ReShade reused topology"
    );
    assert!(
        context
            .storage()
            .pending_file_mutations_for_game(&game_id)
            .expect("pending mutations")
            .is_empty()
    );
}

#[tokio::test]
#[cfg(windows)]
async fn install_rejects_a_torn_luma_install_before_network_and_preserves_sentinel() {
    use crate::addons::matching::{MatchKind, Status};

    let db_dir = tempdir().expect("db dir");
    let game_dir = tempdir().expect("game dir");
    let context = Context::open_at(db_dir.path().join("catalog.sqlite")).expect("context");
    let game_id = GameId::new("steam:active-luma-install").expect("game id");
    let exe_path = game_dir.path().join("Dishonored2.exe");
    write_stub_exe(&exe_path);
    seed_game(
        &context,
        &game_id,
        "active-luma-install",
        game_dir.path(),
        &exe_path,
    );
    seed_active_topology(&context, &game_id, game_dir.path());
    let sentinel = game_dir.path().join("renderpilot-luma-install.lock");
    std::fs::write(&sentinel, b"").expect("sentinel");
    let manifest = manifest(vec![crate::addons::luma::test_support::title(
        "active-luma-install",
        "Luma.zip",
        renderpilot_domain::Architecture::X64,
        Status::Working,
        vec![crate::addons::luma::test_support::rule(
            MatchKind::SteamAppid,
            "active-luma-install",
            100,
        )],
    )]);

    let error = super::super::install(InstallRequest {
        context: &context,
        manifest: &manifest,
        reshade_sources: &crate::addons::luma::test_support::reshade_sources(),
        game_id: &game_id,
        safety: game_safety(&context, &game_id),
        progress: None,
    })
    .await
    .expect_err("active route must fail closed on a torn sentinel");

    match error {
        ServiceError::InvalidInput(message) => {
            assert!(message.contains("incomplete earlier Luma install"));
            assert!(!message.contains("remove OptiScaler"));
        }
        other => panic!("expected InvalidInput, got {other:?}"),
    }
    assert!(
        sentinel.is_file(),
        "active preflight must not recover or delete it"
    );
    assert!(engine::is_install_torn(game_dir.path(), AddonKind::Luma));
}
