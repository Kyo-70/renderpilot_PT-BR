use std::path::Path;

use renderpilot_application::{ComponentRepository, GameRepository, InstalledAddonRepository};
use renderpilot_domain::{
    AddonKind, Architecture, ComponentFile, ComponentId, ComponentKind, FileReceipt, GameId,
    GameIdentity, GameInstallation, GameProxyTopology, GameRuntime, InstalledAddon, Launcher,
    LibraryComponent, LibraryTechnology, OptiScalerAdoptionState, OptiScalerConfigurationBaseline,
    OptiScalerFileCleanup, OptiScalerFileReceipt, OptiScalerFileRole, OptiScalerInstallStateParts,
    PathRef, Platform, ProxyImplementation, ProxyLink, ProxyRootPrestate, Swappability,
};
use renderpilot_storage_sqlite::{
    GameMutationCommit, InstalledAddonMutation, OptiScalerAggregateMutation, SqliteStorage,
};

use crate::Context;
use crate::addons::luma::test_support::{
    MACHINE_AMD64, PE32_PLUS_MAGIC, build_nvidia_dlss_pe, build_pe_with_exports, manifest, rule,
    title,
};
use crate::addons::matching::{MatchKind, Status};
use crate::game_mutation_lock::try_lock;
use crate::peer_mutation_executor::observe_peer_path_snapshot;

pub(super) struct ActiveFixture {
    pub(super) _game: tempfile::TempDir,
    pub(super) context: Context,
    pub(super) game_id: GameId,
    pub(super) record: InstalledAddon,
    pub(super) manifest: crate::addons::luma::types::LumaManifest,
}

fn path_ref(path: &Path) -> PathRef {
    PathRef::new(path.to_string_lossy().into_owned()).expect("path")
}

fn game_id(value: &str) -> GameId {
    GameId::new(format!("phase3:{value}")).expect("game id")
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

pub(super) fn active_fixture(value: &str) -> ActiveFixture {
    fixture(value, true)
}

pub(super) fn inactive_fixture(value: &str) -> ActiveFixture {
    fixture(value, false)
}

fn fixture(value: &str, active: bool) -> ActiveFixture {
    let game = tempfile::tempdir().expect("game");
    let context = Context::from_storage(SqliteStorage::in_memory().expect("storage"));
    let id = game_id(value);
    let executable = game.path().join("Game.exe");
    std::fs::write(
        &executable,
        build_pe_with_exports(MACHINE_AMD64, PE32_PLUS_MAGIC, &[]),
    )
    .expect("executable");
    let identity = GameIdentity::new(id.clone(), "Phase 3 Test", Launcher::Steam)
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
    ActiveFixture {
        _game: game,
        context,
        game_id: id,
        record,
        manifest,
    }
}

pub(super) fn lock(fixture: &ActiveFixture) -> crate::game_mutation_lock::GameMutationGuard {
    try_lock(&fixture.game_id).expect("lock")
}

fn seed_active_topology(context: &Context, game_id: &GameId, root: &Path, executable: &Path) {
    let outer_path = root.join("dxgi.dll");
    let downstream_path = root.join("ReShade64.dll");
    let config_path = root.join("OptiScaler.ini");
    std::fs::write(&outer_path, b"OptiScaler outer").expect("outer");
    std::fs::write(&downstream_path, reshade_host_bytes([6, 7, 0, 0])).expect("host");
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
    let topology_id = format!("optiscaler:phase3:{}", game_id.as_str());
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
            release_id: "phase3".to_owned(),
            manifest_revision: "phase3".to_owned(),
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
            config_base_release: "phase3".to_owned(),
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
fn phase_three_captures_locked_authority_host_catalog_and_cascade_evidence() {
    let fixture = active_fixture("phase3-integrated");
    let dlss_path = fixture
        ._game
        .path()
        .join(renderpilot_detection::NVNGX_DLSS_FILE_NAME);
    let dlss_hash = renderpilot_domain::Sha256Hash::new("d".repeat(64)).expect("hash");
    let component = LibraryComponent::new(
        ComponentId::new("phase3:dlss-component").expect("component id"),
        fixture.game_id.clone(),
        ComponentKind::NativeLibrary,
        LibraryTechnology::DlssSuperResolution,
        Swappability::Swappable,
    )
    .with_file(
        ComponentFile::new(PathRef::new(dlss_path.to_string_lossy().into_owned()).expect("path"))
            .with_sha256(dlss_hash.clone()),
    );
    fixture
        .context
        .storage()
        .replace_components_for_game(&fixture.game_id, &[component])
        .expect("catalog component");

    let guard = lock(&fixture);
    let route = crate::addons::luma::use_cases::commands::update::route::snapshot_update_route(
        &fixture.context,
        &fixture.manifest,
        &guard,
    )
    .expect("active route");
    let super::super::UpdatePhase1::Active(phase1) = route else {
        panic!("expected active route");
    };
    let prepared =
        super::prepared(crate::addons::luma::peer::LumaActiveUpdatePayloadInput::Preserve);
    let phase3 = super::super::snapshot_active_update_phase3(
        &fixture.context,
        &fixture.manifest,
        &guard,
        &phase1,
        &prepared,
    )
    .expect("phase-three evidence");

    let expected_target = phase3
        .authority()
        .canonical_game_root()
        .join(renderpilot_detection::NVNGX_DLSS_FILE_NAME);
    assert_eq!(
        phase3.authority().effective_dlss_target().expect("target"),
        PathRef::from_canonical_native_absolute(&expected_target).expect("target")
    );
    assert_eq!(
        phase3.host_assessment().snapshot().exact_path,
        crate::paths::normalized_key(Path::new(phase1.downstream_path().as_str()))
    );
    assert_eq!(phase3.catalog_claim().active_hashes(), &[dlss_hash]);
    assert!(phase3.catalog_claim().baseline().is_none());
    assert!(phase3.cascade().catalog_claim().is_none());
}

#[test]
fn phase_three_rejects_inactive_route_and_phase_one_drift_before_authority_reads() {
    let inactive = inactive_fixture("phase3-inactive");
    let inactive_guard = lock(&inactive);
    let inactive_phase1 = super::phase1(inactive.record.clone());
    let inactive_error = super::super::snapshot_active_update_phase3(
        &inactive.context,
        &inactive.manifest,
        &inactive_guard,
        &inactive_phase1,
        &super::prepared(crate::addons::luma::peer::LumaActiveUpdatePayloadInput::Preserve),
    );
    assert!(inactive_error.is_err());

    let active = active_fixture("phase3-drift");
    let active_guard = lock(&active);
    let route = crate::addons::luma::use_cases::commands::update::route::snapshot_update_route(
        &active.context,
        &active.manifest,
        &active_guard,
    )
    .expect("active route");
    let super::super::UpdatePhase1::Active(mut active_phase1) = route else {
        panic!("expected active route");
    };
    active_phase1.host_replacement_required = !active_phase1.host_replacement_required;
    let drift_error = super::super::snapshot_active_update_phase3(
        &active.context,
        &active.manifest,
        &active_guard,
        &active_phase1,
        &super::prepared(crate::addons::luma::peer::LumaActiveUpdatePayloadInput::Preserve),
    );
    assert!(drift_error.is_err());
}
