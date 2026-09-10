use std::path::Path;

use renderpilot_application::{GameRepository, InstalledAddonRepository};
use renderpilot_domain::{
    AddonKind, Architecture, FileReceipt, GameId, GameIdentity, GameInstallation,
    GameProxyTopology, GameRuntime, InstalledAddon, Launcher, OptiScalerAdoptionState,
    OptiScalerConfigurationBaseline, OptiScalerFileCleanup, OptiScalerFileReceipt,
    OptiScalerFileRole, OptiScalerInstallStateParts, PathRef, Platform, ProxyImplementation,
    ProxyLink, ProxyRootPrestate,
};
use renderpilot_storage_sqlite::{
    GameMutationCommit, InstalledAddonMutation, OptiScalerAggregateMutation, SqliteStorage,
};

use super::super::super::{UpdateRequest, update};
use super::super::{InactiveUpdatePhase1, UpdatePhase1, snapshot_update_route};
use crate::Context;
use crate::addons::luma::test_support::{
    MACHINE_AMD64, PE32_PLUS_MAGIC, build_nvidia_dlss_pe, build_pe_with_exports, manifest, rule,
    title,
};
use crate::addons::matching::{MatchKind, Status};
use crate::game_mutation_lock::try_lock;
use crate::peer_mutation_executor::observe_peer_path_snapshot;

struct RouteFixture {
    _game: tempfile::TempDir,
    context: Context,
    game_id: GameId,
    record: InstalledAddon,
    manifest: crate::addons::luma::types::LumaManifest,
}

fn path_ref(path: &Path) -> PathRef {
    PathRef::new(path.to_string_lossy().into_owned()).expect("path")
}

fn safety(fixture: &RouteFixture) -> crate::GameSafetyPermit {
    let authority = crate::FileSafetyAuthority::new();
    let assessment = authority
        .issue_game_assessment(&fixture.context, &fixture.game_id)
        .expect("safety assessment");
    authority
        .game_permit(fixture.game_id.clone(), Some(&assessment.context_token))
        .expect("safety permit")
}

fn game_id(value: &str) -> GameId {
    GameId::new(format!("manual:{value}")).expect("game id")
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

fn fixture(value: &str, active: bool) -> RouteFixture {
    let game = tempfile::tempdir().expect("game");
    let context = Context::from_storage(SqliteStorage::in_memory().expect("storage"));
    let id = game_id(value);
    let executable = game.path().join("Game.exe");
    std::fs::write(
        &executable,
        build_pe_with_exports(MACHINE_AMD64, PE32_PLUS_MAGIC, &[]),
    )
    .expect("executable");
    let identity = GameIdentity::new(id.clone(), "Route Test", Launcher::Steam)
        .expect("identity")
        .with_external_id(value)
        .expect("external id");
    let game_install = GameInstallation::new(
        identity,
        Platform::Windows,
        GameRuntime::NativeWindows,
        path_ref(game.path()),
    )
    .with_executable_candidate(path_ref(&executable));
    context.storage().upsert_game(&game_install).expect("game");

    let addon = game.path().join("Luma.addon64");
    let record = InstalledAddon::new(id.clone(), AddonKind::Luma, path_ref(&addon));
    context
        .storage()
        .upsert_installed_addon(&record)
        .expect("record");
    let record = crate::addons::records::record_of_kind(&context, &id, AddonKind::Luma)
        .expect("stored record")
        .expect("record row");

    if active {
        seed_active_topology(&context, &id, game.path(), &executable);
    }

    let manifest = manifest(vec![title(
        value,
        "Luma.zip",
        Architecture::X64,
        Status::Working,
        vec![rule(MatchKind::SteamAppid, value, 100)],
    )]);
    RouteFixture {
        _game: game,
        context,
        game_id: id,
        record,
        manifest,
    }
}

fn seed_active_topology(context: &Context, game_id: &GameId, root: &Path, executable: &Path) {
    let outer_path = root.join("dxgi.dll");
    let downstream_path = root.join("ReShade64.dll");
    let config_path = root.join("OptiScaler.ini");
    std::fs::write(&outer_path, b"OptiScaler outer").expect("outer");
    std::fs::write(&downstream_path, reshade_host_bytes([6, 7, 0, 0])).expect("ReShade host");
    std::fs::write(&config_path, b"[OptiScaler]\n").expect("config");
    let root_ref = path_ref(root);
    let outer_ref = path_ref(&outer_path);
    let downstream_ref = path_ref(&downstream_path);
    let outer_observation = observe_peer_path_snapshot(&outer_ref, &root_ref).expect("outer");
    let downstream_observation =
        observe_peer_path_snapshot(&downstream_ref, &root_ref).expect("downstream");
    let config_ref = path_ref(&config_path);
    let config_observation = observe_peer_path_snapshot(&config_ref, &root_ref).expect("config");
    let outer_file = outer_observation.file().expect("outer file");
    let downstream_file = downstream_observation.file().expect("downstream file");
    let config_file = config_observation.file().expect("config file");
    let config_receipt = FileReceipt::reused(config_file.identity(), config_file.digest().clone())
        .expect("config receipt");
    let topology_id = format!("optiscaler:route:{}", game_id.as_str());
    let topology = GameProxyTopology {
        id: topology_id.clone(),
        game_id: game_id.clone(),
        root_slot: outer_ref.clone(),
        outer: ProxyLink {
            implementation: ProxyImplementation::OptiScaler,
            path: outer_ref,
            receipt: FileReceipt::reused(outer_file.identity(), outer_file.digest().clone())
                .expect("outer receipt"),
        },
        downstream: Some(ProxyLink {
            implementation: ProxyImplementation::ReShade,
            path: downstream_ref,
            receipt: FileReceipt::reused(
                downstream_file.identity(),
                downstream_file.digest().clone(),
            )
            .expect("downstream receipt"),
        }),
        downstream_origin: Some(path_ref(&outer_path)),
        root_prestate: ProxyRootPrestate::Absent,
    };
    let state = renderpilot_domain::from_persisted(
        OptiScalerInstallStateParts {
            game_id: game_id.clone(),
            release_id: "route".to_owned(),
            manifest_revision: "route".to_owned(),
            archive_sha256: None,
            source: None,
            target_exe_path: path_ref(executable),
            target_dir: root_ref,
            modules: vec!["core".to_owned()],
            release_files: vec![OptiScalerFileReceipt {
                path: config_ref,
                installed: config_receipt.clone(),
                role: OptiScalerFileRole::Configuration,
                cleanup: OptiScalerFileCleanup::PreserveUnchanged,
                baseline: renderpilot_domain::OptiScalerReleaseFileBaseline::Absent,
            }],
            runtime_bindings: Vec::new(),
            directory_receipts: Vec::new(),
            proxy_topology_id: Some(topology_id),
            config_schema: 1,
            config_base_release: "route".to_owned(),
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
        .expect("topology");
}

#[test]
fn route_returns_exact_inactive_record_and_torn_snapshot_from_storage() {
    let fixture = fixture("route-inactive", false);
    let game = fixture
        .context
        .storage()
        .find_game(&fixture.game_id)
        .expect("game")
        .expect("game");
    let marker = crate::addons::engine::sentinel_path(
        Path::new(game.install_path().as_str()),
        AddonKind::Luma,
    );
    std::fs::write(&marker, b"torn").expect("marker");
    let guard = try_lock(&fixture.game_id).expect("lock");
    let route =
        snapshot_update_route(&fixture.context, &fixture.manifest, &guard).expect("inactive route");

    let UpdatePhase1::Inactive(snapshot) = route else {
        panic!("expected inactive route");
    };
    let InactiveUpdatePhase1 {
        record,
        had_torn_marker,
    } = *snapshot;
    assert_eq!(record, fixture.record);
    assert!(had_torn_marker);
}

#[test]
fn route_with_active_topology_returns_the_exact_active_snapshot() {
    let fixture = fixture("route-active", true);
    let guard = try_lock(&fixture.game_id).expect("lock");
    let route =
        snapshot_update_route(&fixture.context, &fixture.manifest, &guard).expect("active route");
    let UpdatePhase1::Active(snapshot) = route else {
        panic!("expected active route");
    };
    assert_eq!(snapshot.record(), &fixture.record);
    assert_eq!(snapshot.topology().game_id, fixture.game_id);
    assert!(crate::paths::same_path(
        snapshot.canonical_game_root(),
        snapshot.target().game_dir.as_path()
    ));
    assert_eq!(
        snapshot.downstream_path().file_name(),
        Some("ReShade64.dll")
    );
    assert!(!snapshot.host_replacement_required());
}

#[tokio::test]
async fn public_update_keeps_active_route_out_of_inactive_apply() {
    let fixture = fixture("public-active-route", true);
    let reshade_sources = crate::addons::luma::test_support::reshade_sources();
    let game = fixture
        .context
        .storage()
        .find_game(&fixture.game_id)
        .expect("game")
        .expect("game");
    let marker = crate::addons::engine::sentinel_path(
        Path::new(game.install_path().as_str()),
        AddonKind::Luma,
    );
    let result = update(UpdateRequest {
        context: &fixture.context,
        manifest: &fixture.manifest,
        reshade_sources: &reshade_sources,
        game_id: &fixture.game_id,
        force_full: false,
        safety: safety(&fixture),
        progress: None,
    })
    .await
    .expect_err("active preparation must reject missing payload provenance");

    assert!(
        result
            .to_string()
            .contains("active Luma record is missing add-on payload provenance")
    );
    assert!(!marker.exists());
}

#[tokio::test]
async fn public_update_keeps_inactive_route_behind_the_existing_prepare_contract() {
    let fixture = fixture("public-inactive-route", false);
    let reshade_sources = crate::addons::luma::test_support::reshade_sources();
    let game = fixture
        .context
        .storage()
        .find_game(&fixture.game_id)
        .expect("game")
        .expect("game");
    let marker = crate::addons::engine::sentinel_path(
        Path::new(game.install_path().as_str()),
        AddonKind::Luma,
    );
    let result = update(UpdateRequest {
        context: &fixture.context,
        manifest: &fixture.manifest,
        reshade_sources: &reshade_sources,
        game_id: &fixture.game_id,
        force_full: false,
        safety: safety(&fixture),
        progress: None,
    })
    .await
    .expect_err("inactive preparation must reject missing payload provenance");

    assert!(
        result
            .to_string()
            .contains("missing add-on payload provenance")
    );
    assert!(!marker.exists());
}
