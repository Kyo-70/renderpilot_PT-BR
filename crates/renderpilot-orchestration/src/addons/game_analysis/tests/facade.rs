use tempfile::tempdir;

use renderpilot_domain::{Architecture, GraphicsApi, Launcher};

use super::common::{install_in, resolved, write_minimal_pe};
use crate::addons::engine_config::{EngineIniResolution, service::resolve_for_analysis};
use crate::addons::game_analysis::analyze_game;
use crate::addons::game_analysis::assemble_facts;
use crate::addons::game_analysis::budget::AnalysisBudget;
use crate::addons::game_analysis::parsers::tokens::{
    STREAM_CHUNK_SIZE, VERSION_SECTION_SCAN_BUDGET,
};
use crate::addons::matching::Engine;

#[test]
fn assemble_facts_carries_identity_exe_and_graphics() {
    let dir = tempdir().expect("tempdir");
    let install = install_in(dir.path(), "Game.exe");
    let primary = resolved("C:/g/Game.exe", &[GraphicsApi::D3D12]);

    let facts = assemble_facts(&install, Some(&primary));
    assert_eq!(facts.launcher, Launcher::Steam);
    assert_eq!(facts.external_id.as_deref(), Some("1091500"));
    assert_eq!(facts.exe_file_name.as_deref(), Some("Game.exe"));
    assert_eq!(facts.graphics.apis(), &[GraphicsApi::D3D12]);
}

#[test]
fn analyze_game_proves_packaged_shared_engine_project_identity() {
    let dir = tempdir().expect("tempdir");
    let root = dir.path().join("Satisfactory");
    let project = root.join("FactoryGame");
    let binary = root.join("Engine/Binaries/Win64/FactoryGameSteam-Win64-Shipping.exe");
    std::fs::create_dir_all(binary.parent().expect("engine binaries")).expect("engine binaries");
    std::fs::create_dir_all(project.join("Binaries/Win64")).expect("project binaries");
    std::fs::create_dir_all(project.join("Content")).expect("project content");
    write_minimal_pe(&binary, Architecture::X64);

    let install = install_in(&root, &binary.to_string_lossy());
    let analysis = analyze_game(&install, None);

    let identity = analysis.unreal_project.expect("project identity");
    assert_eq!(identity.project_root, project);
    assert_eq!(identity.project_name, "FactoryGame");
}

#[test]
fn packaged_shared_engine_identity_resolves_existing_local_app_data_engine_ini() {
    let dir = tempdir().expect("tempdir");
    let root = dir.path().join("Satisfactory");
    let project = root.join("FactoryGame");
    let binary = root.join("Engine/Binaries/Win64/FactoryGameSteam-Win64-Shipping.exe");
    let local_app_data = dir.path().join("LocalAppData");
    let engine_ini = local_app_data.join("FactoryGame/Saved/Config/Windows/Engine.ini");
    std::fs::create_dir_all(binary.parent().expect("engine binaries")).expect("engine binaries");
    std::fs::create_dir_all(project.join("Binaries/Win64")).expect("project binaries");
    std::fs::create_dir_all(project.join("Content")).expect("project content");
    std::fs::create_dir_all(engine_ini.parent().expect("config directory"))
        .expect("config directory");
    std::fs::write(&engine_ini, b"[SystemSettings]\n").expect("engine ini");
    write_minimal_pe(&binary, Architecture::X64);

    let install = install_in(&root, &binary.to_string_lossy());
    let analysis = analyze_game(&install, None);
    let resolution = resolve_for_analysis(&analysis, Some(&local_app_data));

    assert_eq!(resolution, EngineIniResolution::Ready(engine_ini));
}

#[test]
fn assemble_facts_without_primary_has_no_exe_or_graphics() {
    let dir = tempdir().expect("tempdir");
    let install = install_in(dir.path(), "Game.exe");

    let facts = assemble_facts(&install, None);
    assert_eq!(facts.launcher, Launcher::Steam);
    assert_eq!(facts.external_id.as_deref(), Some("1091500"));
    assert_eq!(facts.exe_file_name, None);
    assert!(facts.graphics.apis().is_empty());
}

#[test]
fn assemble_facts_does_not_assume_unreal_from_shipping_candidate_name() {
    let dir = tempdir().expect("tempdir");
    let install = install_in(dir.path(), "MyGame-Win64-Shipping.exe");
    let facts = assemble_facts(&install, None);
    // Positive proof barrier: shipping exe name alone does not prove Unreal without presence proofs
    assert_eq!(facts.engine, None);
    assert_eq!(facts.unreal_version, None);
}

#[test]
fn assemble_facts_does_not_assume_unreal_from_engine_binaries_dir() {
    let dir = tempdir().expect("tempdir");
    let install = install_in(dir.path(), "Engine/Binaries/Win64/Game.exe");
    let facts = assemble_facts(&install, None);
    // Positive proof barrier: folder path alone does not prove Unreal
    assert_eq!(facts.engine, None);
    assert_eq!(facts.unreal_version, None);
}

#[test]
fn detects_unity_from_data_directory() {
    let dir = tempdir().expect("tempdir");
    std::fs::create_dir_all(dir.path().join("Game_Data")).expect("mkdir");

    let install = install_in(dir.path(), "Game.exe");
    let facts = assemble_facts(&install, None);
    assert_eq!(facts.engine, Some(Engine::Unity));
    assert_eq!(facts.unreal_version, None);
}

#[test]
fn detects_unity_from_player_dll() {
    let dir = tempdir().expect("tempdir");
    std::fs::write(dir.path().join("UnityPlayer.dll"), b"").expect("write dll");

    let install = install_in(dir.path(), "Game.exe");
    let facts = assemble_facts(&install, None);
    assert_eq!(facts.engine, Some(Engine::Unity));
    assert_eq!(facts.unreal_version, None);
}

#[test]
fn detects_no_engine_without_markers() {
    let dir = tempdir().expect("tempdir");
    let install = install_in(dir.path(), "Game.exe");
    let facts = assemble_facts(&install, None);
    assert_eq!(facts.engine, None);
    assert_eq!(facts.unreal_version, None);
}

#[test]
fn test_unity_unicode_data_dir_does_not_panic() {
    let dir = tempdir().expect("tempdir");
    // Multi-byte UTF-8 character where byte len - 5 could land inside a code point
    std::fs::create_dir_all(dir.path().join("éabcd")).expect("mkdir");
    std::fs::create_dir_all(dir.path().join("éabcd_Data")).expect("mkdir");

    let install = install_in(dir.path(), "MyGame.exe");
    let facts = assemble_facts(&install, None);
    assert_eq!(facts.engine, Some(Engine::Unity));
}

#[test]
fn test_public_constants_and_defaults() {
    // Constants verification
    assert_eq!(STREAM_CHUNK_SIZE, 64 * 1024);
    assert_eq!(VERSION_SECTION_SCAN_BUDGET, 64 * 1024 * 1024);

    // AnalysisBudget verification
    let budget = AnalysisBudget::default();
    assert_eq!(
        budget.section_stream_bytes_remaining,
        VERSION_SECTION_SCAN_BUDGET
    );
    assert_eq!(budget.max_helpers, 3);
}
