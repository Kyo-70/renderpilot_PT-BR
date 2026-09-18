use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

use tempfile::TempDir;

use super::pe::is_readable_windows_pe_executable;
use super::shipping::{
    is_bound_shipping_target, is_shipping_binary_name, strip_exe_suffix, strip_shipping_suffix,
};
use super::*;

fn write_file(path: &Path, bytes: &[u8]) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent dir");
    }
    let mut f = File::create(path).expect("create file");
    f.write_all(bytes).expect("write contents");
}

#[test]
fn returns_empty_for_missing_directory() {
    let path = std::env::temp_dir().join("renderpilot-no-such-folder-91823");
    assert!(detect_executable_candidates(&path).is_empty());
}

#[test]
fn returns_empty_when_no_exe_files() {
    let tmp = TempDir::new().unwrap();
    write_file(&tmp.path().join("readme.txt"), b"hi");
    assert!(detect_executable_candidates(tmp.path()).is_empty());
}

#[test]
fn ranks_root_exe_above_nested_one_of_same_size() {
    let tmp = TempDir::new().unwrap();
    write_file(&tmp.path().join("Game.exe"), &[0u8; 1024]);
    write_file(&tmp.path().join("bin/Game.exe"), &[0u8; 1024]);

    let results = detect_executable_candidates(tmp.path());
    let game_only: Vec<&ExecutableCandidate> =
        results.iter().filter(|c| c.rejection.is_none()).collect();
    assert_eq!(game_only.len(), 2);
    assert_eq!(game_only[0].depth, 0);
    assert_eq!(game_only[1].depth, 1);
}

#[test]
fn rejects_launcher_and_setup_exes() {
    let tmp = TempDir::new().unwrap();
    write_file(&tmp.path().join("Game.exe"), &[0u8; 1024]);
    write_file(&tmp.path().join("GameLauncher.exe"), &[0u8; 1024]);
    write_file(&tmp.path().join("Setup.exe"), &[0u8; 1024]);

    let results = detect_executable_candidates(tmp.path());
    let kept: Vec<&ExecutableCandidate> =
        results.iter().filter(|c| c.rejection.is_none()).collect();
    let rejected: Vec<&ExecutableCandidate> =
        results.iter().filter(|c| c.rejection.is_some()).collect();

    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].file_name, "Game.exe");
    assert_eq!(rejected.len(), 2);
    // Order: kept first, rejected after.
    assert_eq!(results[0].file_name, "Game.exe");
}

#[test]
fn rejects_exact_non_game_suffix_names() {
    let tmp = TempDir::new().unwrap();
    let file_names = [
        "Server.exe",
        "Dedicated.exe",
        "Editor.exe",
        "Tool.exe",
        "Update.exe",
        "Settings.exe",
        "Benchmark.exe",
        "Diag.exe",
    ];

    for file_name in file_names {
        write_file(&tmp.path().join(file_name), &[0u8; 1024]);
    }

    let results = detect_executable_candidates(tmp.path());
    for file_name in file_names {
        let candidate = results
            .iter()
            .find(|candidate| candidate.file_name == file_name)
            .expect("candidate present");
        assert_eq!(
            candidate.rejection.as_ref().map(RejectionReason::kind),
            Some("non_game_suffix"),
            "{file_name} should be rejected by its exact suffix"
        );
    }
}

#[test]
fn rejection_reasons_carry_matched_token() {
    let tmp = TempDir::new().unwrap();
    write_file(&tmp.path().join("CrashHandler_x64.exe"), &[0u8; 1024]);

    let results = detect_executable_candidates(tmp.path());
    assert_eq!(results.len(), 1);
    let r = results[0].rejection.as_ref().expect("should be rejected");
    // "crash" is in NON_GAME_EXE_SUBSTRINGS.
    assert_eq!(r.kind(), "non_game_substring");
    assert_eq!(r.token(), "crash");
}

#[test]
fn folder_name_match_promotes_main_binary() {
    let tmp = TempDir::new().unwrap();
    let game_dir = tmp.path().join("Cyberpunk2077");
    fs::create_dir_all(&game_dir).unwrap();
    // Two equally-sized exe's, neither at root. The one whose
    // stem matches the install folder name should rank higher.
    write_file(&game_dir.join("Cyberpunk2077.exe"), &[0u8; 1024]);
    write_file(&game_dir.join("RandomOther.exe"), &[0u8; 1024]);

    let results = detect_executable_candidates(&game_dir);
    let kept: Vec<&ExecutableCandidate> =
        results.iter().filter(|c| c.rejection.is_none()).collect();
    assert_eq!(kept.len(), 2);
    assert_eq!(kept[0].file_name, "Cyberpunk2077.exe");
}

#[test]
fn normalized_folder_name_match_tolerates_spacing_and_punctuation() {
    let tmp = TempDir::new().unwrap();
    // Folder carries a space the binary omits; the exact (pre-normalization)
    // comparison would miss this, the normalized one must not.
    let game_dir = tmp.path().join("Cyberpunk 2077");
    fs::create_dir_all(&game_dir).unwrap();
    write_file(&game_dir.join("Cyberpunk2077.exe"), &[0u8; 1024]);
    write_file(&game_dir.join("RandomOther.exe"), &[0u8; 1024]);

    let results = detect_executable_candidates(&game_dir);
    let kept: Vec<&ExecutableCandidate> =
        results.iter().filter(|c| c.rejection.is_none()).collect();
    assert_eq!(kept.len(), 2);
    assert_eq!(kept[0].file_name, "Cyberpunk2077.exe");
}

#[test]
fn rejects_executables_inside_installer_and_redist_folders() {
    let tmp = TempDir::new().unwrap();
    // The real game binary sits deep under the engine tree; installer/redist
    // helpers sit in well-known folders and must be filtered by location even
    // when their name (e.g. `Cleanup.exe`, `vc_redist.x64.exe`) passes the
    // filename filters.
    write_file(
        &tmp.path().join("SwGame/Binaries/Win64/JediSurvivor.exe"),
        &[0u8; 1024],
    );
    write_file(&tmp.path().join("__Installer/Cleanup.exe"), &[0u8; 1024]);
    write_file(
        &tmp.path()
            .join("Engine/Extras/Redist/en-us/UEPrereqSetup_x64.exe"),
        &[0u8; 1024],
    );

    let results = detect_executable_candidates(tmp.path());
    let kept: Vec<&str> = results
        .iter()
        .filter(|c| c.rejection.is_none())
        .map(|c| c.file_name.as_str())
        .collect();
    assert_eq!(kept, ["JediSurvivor.exe"]);

    let cleanup = results
        .iter()
        .find(|c| c.file_name == "Cleanup.exe")
        .expect("cleanup present");
    assert_eq!(
        cleanup.rejection.as_ref().map(RejectionReason::kind),
        Some("non_game_location")
    );
}

#[test]
fn relative_path_uses_forward_slashes() {
    let tmp = TempDir::new().unwrap();
    write_file(&tmp.path().join("bin/win64/Game.exe"), &[0u8; 1024]);

    let results = detect_executable_candidates(tmp.path());
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].relative_path, "bin/win64/Game.exe");
}

#[test]
fn skips_dotted_and_non_runtime_directories() {
    let tmp = TempDir::new().unwrap();
    write_file(&tmp.path().join("Game.exe"), &[0u8; 1024]);
    write_file(&tmp.path().join(".git/Decoy.exe"), &[0u8; 1024]);
    write_file(&tmp.path().join(".cache/Decoy.exe"), &[0u8; 1024]);
    write_file(&tmp.path().join("Development/Decoy.exe"), &[0u8; 1024]);

    let results = detect_executable_candidates(tmp.path());
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].file_name, "Game.exe");
}

#[test]
fn detects_case_insensitive_exe_extension() {
    let tmp = TempDir::new().unwrap();
    write_file(&tmp.path().join("Game.EXE"), &[0u8; 1024]);
    write_file(&tmp.path().join("OTHER.Exe"), &[0u8; 1024]);

    let results = detect_executable_candidates(tmp.path());
    assert_eq!(results.len(), 2);
}

#[test]
fn pe_validation_rejects_extension_only_and_accepts_signatures() {
    let tmp = TempDir::new().unwrap();
    let fake = tmp.path().join("Fake.exe");
    write_file(&fake, b"not a PE");
    assert!(!is_readable_windows_pe_executable(&fake));

    let pe = tmp.path().join("Game.exe");
    let mut bytes = vec![0_u8; 0x84];
    bytes[..2].copy_from_slice(b"MZ");
    bytes[0x3c..0x40].copy_from_slice(&0x80_u32.to_le_bytes());
    bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
    write_file(&pe, &bytes);
    assert!(is_readable_windows_pe_executable(&pe));
}

#[test]
fn intentional_probe_depth_limit_is_not_a_filesystem_diagnostic() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join("a/b/c/d/e")).expect("deep directory");

    let report = inspect_executable_candidates(tmp.path());

    assert_eq!(report.completeness(), InstallTreeCompleteness::Incomplete);
    assert!(
        report.diagnostics().is_empty(),
        "a bounded advisory probe is not an access failure"
    );
}

#[test]
fn larger_binaries_outrank_tiny_ones_when_other_signals_tie() {
    let tmp = TempDir::new().unwrap();
    // Both at root, same level, neither matches folder name.
    let big_game = tmp.path().join("BigGame.exe");
    let big_file = File::create(&big_game).expect("create BigGame.exe");
    big_file
        .set_len(110 * 1024 * 1024)
        .expect("set BigGame.exe length");
    write_file(&tmp.path().join("TinyGame.exe"), &[0u8; 1024]);

    let results = detect_executable_candidates(tmp.path());
    let kept: Vec<&ExecutableCandidate> =
        results.iter().filter(|c| c.rejection.is_none()).collect();
    assert!(kept.len() >= 2);
    assert_eq!(kept[0].file_name, "BigGame.exe");
}

#[test]
fn root_stub_wins_over_nested_shipping_in_pure_filesystem_ranking() {
    let tmp = TempDir::new().unwrap();
    // Without launcher metadata or graphics imports, filesystem ranking
    // favors the root executable over nested binaries.
    write_file(&tmp.path().join("FactoryGameSteam.exe"), &[0u8; 1024]);
    write_file(
        &tmp.path()
            .join("Engine/Binaries/Win64/FactoryGameSteam-Win64-Shipping.exe"),
        &[0u8; 1024],
    );

    let results = detect_executable_candidates(tmp.path());
    let kept: Vec<&ExecutableCandidate> =
        results.iter().filter(|c| c.rejection.is_none()).collect();
    assert_eq!(kept.len(), 2);
    assert_eq!(kept[0].file_name, "FactoryGameSteam.exe");
    assert_eq!(kept[1].file_name, "FactoryGameSteam-Win64-Shipping.exe");
}

#[test]
fn unicode_safe_suffix_stripping() {
    // Multi-byte UTF-8 stems must not panic on suffix stripping
    assert_eq!(strip_exe_suffix("éabc"), "éabc");
    assert_eq!(strip_exe_suffix("é.exe"), "é");
    assert_eq!(strip_shipping_suffix("éabc"), None);
    assert_eq!(strip_shipping_suffix("é-Shipping.exe"), Some("é"));
    assert_eq!(
        strip_shipping_suffix("Café-Win64-Shipping.exe"),
        Some("Café")
    );
    assert!(is_bound_shipping_target(
        "Café-Win64-Shipping.exe",
        "café.exe"
    ));
}

#[test]
fn strip_shipping_suffix_and_binding_semantics() {
    assert_eq!(
        strip_shipping_suffix("FactoryGameSteam-Win64-Shipping.exe"),
        Some("FactoryGameSteam")
    );
    assert_eq!(
        strip_shipping_suffix("FactoryGameSteam-Win32-Shipping.exe"),
        Some("FactoryGameSteam")
    );
    assert_eq!(strip_shipping_suffix("Game-Shipping.exe"), Some("Game"));
    assert_eq!(strip_shipping_suffix("Game_Shipping.exe"), Some("Game"));
    assert_eq!(
        strip_shipping_suffix("FactoryGameSteam-Win64-Shipping"),
        Some("FactoryGameSteam")
    );
    assert_eq!(strip_shipping_suffix("Game.exe"), None);
    assert_eq!(strip_shipping_suffix("-Shipping.exe"), None);
    assert_eq!(strip_shipping_suffix("Shipping.exe"), None);

    assert!(is_shipping_binary_name("Foo-Win64-Shipping.exe"));
    assert!(!is_shipping_binary_name("Foo.exe"));

    // Bound shipping target verification
    assert!(is_bound_shipping_target(
        "FactoryGameSteam-Win64-Shipping.exe",
        "FactoryGameSteam.exe"
    ));
    assert!(is_bound_shipping_target(
        "factorygamesteam-win64-shipping.exe",
        "FactoryGameSteam.exe"
    ));
    assert!(is_bound_shipping_target("Game-Shipping.exe", "Game.exe"));
    // Unrelated shipping binary does NOT bind to launcher
    assert!(!is_bound_shipping_target(
        "CrashReportClient-Win64-Shipping.exe",
        "FactoryGameSteam.exe"
    ));
    assert!(!is_bound_shipping_target(
        "Unrelated-Shipping.exe",
        "Game.exe"
    ));
    assert!(!is_bound_shipping_target("Game.exe", "Game.exe"));
}
