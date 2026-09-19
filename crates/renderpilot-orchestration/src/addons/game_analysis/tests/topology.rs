use tempfile::tempdir;

use renderpilot_domain::{Architecture, PathRef};

use super::common::{create_minimal_valid_pe, resolved_with_arch, write_minimal_pe};
use crate::addons::game_analysis::budget::AnalysisBudget;
use crate::addons::game_analysis::context::GameInstallationContext;
use crate::addons::game_analysis::evidence::Authority;
use crate::addons::game_analysis::facade::{
    EngineDetection, UnknownReason, UnrealDetection, UnrealPresenceProof,
    analyze_unreal_installation,
};
use crate::addons::game_analysis::forensics::ComponentDisposition;
use crate::addons::game_analysis::resolver::ResolvedComponent;
use crate::addons::game_analysis::topology::executable::{
    BoundPrimaryExecutable, TargetPlatformDetection, TopologyError, discover_engine_helpers,
};
use crate::addons::game_analysis::topology::metadata::{
    BoundEngineMetadata, BoundTargetMetadata, find_and_bind_project_metadata,
};

/// Regression test 2: from_resolved rejects executables outside the game installation context
#[test]
fn test_from_resolved_rejects_outside_context() {
    let temp = tempfile::tempdir().unwrap();
    let game_a = temp.path().join("GameA");
    let game_b = temp.path().join("GameB");
    std::fs::create_dir_all(&game_a).unwrap();
    std::fs::create_dir_all(game_b.join("Binaries").join("Win64")).unwrap();
    let outside_exe = game_b.join("Binaries").join("Win64").join("GameB.exe");
    std::fs::File::create(&outside_exe).unwrap();

    let context = GameInstallationContext::new(&game_a).unwrap();
    let resolved = crate::game_executable::ResolvedExecutable {
        path: PathRef::new(&*outside_exe.to_string_lossy()).unwrap(),
        file_name: "GameB.exe".to_string(),
        graphics: renderpilot_domain::ExeGraphicsInfo::new(vec![], None),
        source: crate::game_executable::ExeSource::Auto,
    };
    let res = BoundPrimaryExecutable::from_resolved(&context, &resolved);
    assert!(matches!(
        res,
        Err(TopologyError::OutsideInstallationContext(_))
    ));
}

/// Regression test 3: from_resolved rejects override_path pointing to Engine/Binaries service helpers
#[test]
fn test_from_resolved_rejects_engine_service_helpers() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("TestGame");
    let engine_bin = root.join("Engine").join("Binaries").join("Win64");
    std::fs::create_dir_all(&engine_bin).unwrap();
    let engine_exe = engine_bin.join("CrashReportClient.exe");
    std::fs::File::create(&engine_exe).unwrap();

    let context = GameInstallationContext::new(&root).unwrap();
    let resolved = crate::game_executable::ResolvedExecutable {
        path: PathRef::new(&*engine_exe.to_string_lossy()).unwrap(),
        file_name: "CrashReportClient.exe".to_string(),
        graphics: renderpilot_domain::ExeGraphicsInfo::new(vec![], None),
        source: crate::game_executable::ExeSource::Auto,
    };
    let res = BoundPrimaryExecutable::from_resolved(&context, &resolved);
    assert!(matches!(
        res,
        Err(TopologyError::EngineServiceHelperDisallowed(_))
    ));
}

/// Regression test 4: BoundEngineMetadata::open rejects untrusted metadata locations
#[test]
fn test_bound_engine_metadata_rejects_invalid_location() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("TestGame");
    let wrong_dir = temp.path().join("WrongDir");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&wrong_dir).unwrap();
    let fake_path = wrong_dir.join("Build.version");
    std::fs::File::create(&fake_path).unwrap();

    let context = GameInstallationContext::new(&root).unwrap();
    let res = BoundEngineMetadata::open(&context, &fake_path);
    assert!(matches!(
        res,
        Err(TopologyError::OutsideInstallationContext(_)
            | TopologyError::InvalidMetadataLocation(_))
    ));
}

#[test]
fn test_helper_discovery_unicode_prefix_does_not_panic() {
    let temp = tempfile::tempdir().unwrap();
    let context = GameInstallationContext::new(temp.path().to_path_buf()).unwrap();
    let bin_dir = temp.path().join("Engine").join("Binaries").join("Win64");
    std::fs::create_dir_all(&bin_dir).unwrap();

    // Multi-byte UTF-8 character in the first 17 bytes
    let unicode_helper = bin_dir.join("écrashreportclient.exe");
    std::fs::write(&unicode_helper, b"MZ").unwrap();

    let helpers = discover_engine_helpers(&context, renderpilot_domain::Architecture::X64, 3, None);
    // Does not panic, and non-PE file is rejected
    assert_eq!(helpers.len(), 0);
}

#[test]
fn test_uproject_stem_matching_with_win64_suffix() {
    let temp = tempfile::tempdir().unwrap();
    let context = GameInstallationContext::new(temp.path().to_path_buf()).unwrap();

    // Multiple .uproject files in project directory
    let shooter_uproj = temp.path().join("ShooterGame.uproject");
    std::fs::write(&shooter_uproj, r#"{"EngineAssociation": "5.4"}"#).unwrap();
    let other_uproj = temp.path().join("OtherGame.uproject");
    std::fs::write(&other_uproj, r#"{"EngineAssociation": "5.4"}"#).unwrap();

    let primary_path = temp.path().join("ShooterGame-Win64.exe");
    std::fs::write(&primary_path, b"MZ").unwrap();

    let bound = find_and_bind_project_metadata(&context, &primary_path).unwrap();
    assert!(
        bound.is_some(),
        "Must match ShooterGame.uproject via cleaned stem"
    );
    assert_eq!(
        std::fs::canonicalize(bound.unwrap().path()).unwrap(),
        std::fs::canonicalize(&shooter_uproj).unwrap()
    );
}

#[test]
fn test_bound_primary_in_engine_binaries_succeeds() {
    let temp = tempdir().unwrap();
    let context = GameInstallationContext::new(temp.path().to_path_buf()).unwrap();

    // 1. Win64 AMD64 PE in Engine/Binaries/Win64
    let win64_path = temp
        .path()
        .join("Engine")
        .join("Binaries")
        .join("Win64")
        .join("Game-Win64-Shipping.exe");
    write_minimal_pe(&win64_path, Architecture::X64);
    let win64_resolved = resolved_with_arch(win64_path.to_str().unwrap(), Architecture::X64);

    let bound64 = BoundPrimaryExecutable::from_resolved(&context, &win64_resolved);
    assert!(
        bound64.is_ok(),
        "Engine/Binaries/Win64 primary must bind successfully"
    );
    let bound64 = bound64.unwrap();
    assert_eq!(bound64.architecture(), Architecture::X64);
    assert_eq!(
        bound64.target_platform(),
        TargetPlatformDetection::Win64Amd64
    );

    // 2. Win32 x86 PE in Engine/Binaries/Win32
    let win32_path = temp
        .path()
        .join("Engine")
        .join("Binaries")
        .join("Win32")
        .join("Game-Win32-Shipping.exe");
    write_minimal_pe(&win32_path, Architecture::X86);
    let win32_resolved = resolved_with_arch(win32_path.to_str().unwrap(), Architecture::X86);

    let bound32 = BoundPrimaryExecutable::from_resolved(&context, &win32_resolved);
    assert!(
        bound32.is_ok(),
        "Engine/Binaries/Win32 primary must bind successfully"
    );
    let bound32 = bound32.unwrap();
    assert_eq!(bound32.architecture(), Architecture::X86);
    assert_eq!(bound32.target_platform(), TargetPlatformDetection::Win32X86);
}

#[test]
fn test_discover_engine_helpers_excludes_canonical_primary() {
    let temp = tempdir().unwrap();
    let context = GameInstallationContext::new(temp.path().to_path_buf()).unwrap();

    let primary_path = temp
        .path()
        .join("Engine")
        .join("Binaries")
        .join("Win64")
        .join("FactoryGameSteam-Win64-Shipping.exe");
    write_minimal_pe(&primary_path, Architecture::X64);

    let helper_path = temp
        .path()
        .join("Engine")
        .join("Binaries")
        .join("Win64")
        .join("CrashReportClient.exe");
    write_minimal_pe(&helper_path, Architecture::X64);

    let primary_res = resolved_with_arch(primary_path.to_str().unwrap(), Architecture::X64);
    let bound_primary = BoundPrimaryExecutable::from_resolved(&context, &primary_res).unwrap();

    // When exclude_canonical is provided with primary path
    let helpers =
        discover_engine_helpers(&context, Architecture::X64, 5, Some(bound_primary.path()));
    assert_eq!(helpers.len(), 1);
    assert_eq!(
        std::fs::canonicalize(helpers[0].path()).unwrap(),
        std::fs::canonicalize(&helper_path).unwrap()
    );
    assert!(!helpers.iter().any(|h| h.path() == bound_primary.path()));

    // When exclude_canonical is None, primary exe is also discovered as a candidate
    let all_helpers = discover_engine_helpers(&context, Architecture::X64, 5, None);
    assert_eq!(all_helpers.len(), 2);
}

#[test]
fn test_service_helpers_disallowed_as_primary() {
    let temp = tempdir().unwrap();
    let context = GameInstallationContext::new(temp.path().to_path_buf()).unwrap();

    let service_names = [
        "CrashReportClient.exe",
        "CrashReportClient-Win64-Shipping.exe",
        "CrashReportClientEditor.exe",
        "UnrealCEFSubProcess.exe",
        "epicwebhelper.exe",
    ];

    for name in service_names {
        let path = temp
            .path()
            .join("Engine")
            .join("Binaries")
            .join("Win64")
            .join(name);
        write_minimal_pe(&path, Architecture::X64);
        let res = resolved_with_arch(path.to_str().unwrap(), Architecture::X64);
        let bound = BoundPrimaryExecutable::from_resolved(&context, &res);
        assert!(
            matches!(bound, Err(TopologyError::EngineServiceHelperDisallowed(_))),
            "Known service helper {name} must be rejected as Primary"
        );
    }
}

#[test]
fn test_non_engine_binaries_executable_with_helper_word_not_rejected() {
    let temp = tempdir().unwrap();
    let context = GameInstallationContext::new(temp.path().to_path_buf()).unwrap();

    // An executable outside Engine/Binaries with a helper name (e.g. root or Game/Binaries)
    // must NOT be rejected by the service helper check because it's not inside Engine/Binaries.
    let path = temp
        .path()
        .join("Game")
        .join("Binaries")
        .join("Win64")
        .join("CrashReportClient.exe");
    write_minimal_pe(&path, Architecture::X64);
    let res = resolved_with_arch(path.to_str().unwrap(), Architecture::X64);
    let bound = BoundPrimaryExecutable::from_resolved(&context, &res);
    assert!(
        bound.is_ok(),
        "Executable outside Engine/Binaries must not be rejected even if named CrashReportClient"
    );
}

#[test]
fn test_primary_and_sibling_target_version_produces_exact_authoritative() {
    let temp = tempdir().unwrap();
    let context = GameInstallationContext::new(temp.path().to_path_buf()).unwrap();

    let primary_path = temp
        .path()
        .join("Engine")
        .join("Binaries")
        .join("Win64")
        .join("Foo-Win64-Shipping.exe");
    write_minimal_pe(&primary_path, Architecture::X64);

    let version_path = temp
        .path()
        .join("Engine")
        .join("Binaries")
        .join("Win64")
        .join("Foo-Win64-Shipping.version");
    std::fs::write(
        &version_path,
        r#"{
            "MajorVersion": 5,
            "MinorVersion": 6,
            "PatchVersion": 1,
            "Changelist": 502094,
            "CompatibleChangelist": 0,
            "IsLicenseeVersion": 1,
            "IsPromotedBuild": 1,
            "BranchName": "++FactoryGame+rel-main"
        }"#,
    )
    .unwrap();

    let res = resolved_with_arch(primary_path.to_str().unwrap(), Architecture::X64);
    let mut budget = AnalysisBudget::default();
    let report = analyze_unreal_installation(&context, Some(&res), &mut budget);

    assert_eq!(
        report.engine,
        EngineDetection::Unreal(UnrealDetection::Exact {
            major: 5,
            minor: 6,
            patch: 1,
        })
    );
    let has_target_version_proof = report.presence_proofs.iter().any(|p| {
        matches!(p, UnrealPresenceProof::TargetVersion { path } if path == &version_path.canonicalize().unwrap())
    });
    assert!(
        has_target_version_proof,
        "Must include TargetVersion presence proof"
    );

    // Verify Authoritative tier in forensics
    assert_eq!(report.forensics.records.len(), 1);
    let rec = &report.forensics.records[0];
    assert_eq!(rec.authority, Authority::Authoritative);
    assert_eq!(rec.major_disposition, ComponentDisposition::Decisive);
    assert_eq!(rec.minor_disposition, ComponentDisposition::Decisive);
    assert_eq!(rec.patch_disposition, ComponentDisposition::Decisive);
}

#[test]
fn test_unrelated_sibling_target_version_ignored() {
    let temp = tempdir().unwrap();
    let context = GameInstallationContext::new(temp.path().to_path_buf()).unwrap();

    let primary_path = temp
        .path()
        .join("Engine")
        .join("Binaries")
        .join("Win64")
        .join("Foo-Win64-Shipping.exe");
    write_minimal_pe(&primary_path, Architecture::X64);

    // Unrelated sibling
    let other_version = temp
        .path()
        .join("Engine")
        .join("Binaries")
        .join("Win64")
        .join("OtherTarget.version");
    std::fs::write(
        &other_version,
        r#"{
            "MajorVersion": 5,
            "MinorVersion": 6,
            "PatchVersion": 1,
            "Changelist": 502094,
            "CompatibleChangelist": 0,
            "IsLicenseeVersion": 1,
            "IsPromotedBuild": 1,
            "BranchName": "++FactoryGame+rel-main"
        }"#,
    )
    .unwrap();

    let res = resolved_with_arch(primary_path.to_str().unwrap(), Architecture::X64);
    let mut budget = AnalysisBudget::default();
    let report = analyze_unreal_installation(&context, Some(&res), &mut budget);

    assert_eq!(
        report.engine,
        EngineDetection::UnknownEngine {
            reason: UnknownReason::NoUeSignaturesFound,
        }
    );
    assert!(report.presence_proofs.is_empty());
}

#[test]
fn test_arbitrary_nested_version_ignored() {
    let temp = tempdir().unwrap();
    let context = GameInstallationContext::new(temp.path().to_path_buf()).unwrap();

    let primary_path = temp
        .path()
        .join("Engine")
        .join("Binaries")
        .join("Win64")
        .join("Foo-Win64-Shipping.exe");
    write_minimal_pe(&primary_path, Architecture::X64);

    // Nested in a different directory
    let arbitrary_version = temp
        .path()
        .join("CustomDir")
        .join("Foo-Win64-Shipping.version");
    std::fs::create_dir_all(arbitrary_version.parent().unwrap()).unwrap();
    std::fs::write(
        &arbitrary_version,
        r#"{
            "MajorVersion": 5,
            "MinorVersion": 6,
            "PatchVersion": 1,
            "Changelist": 502094,
            "CompatibleChangelist": 0,
            "IsLicenseeVersion": 1,
            "IsPromotedBuild": 1,
            "BranchName": "++FactoryGame+rel-main"
        }"#,
    )
    .unwrap();

    let res = resolved_with_arch(primary_path.to_str().unwrap(), Architecture::X64);
    let mut budget = AnalysisBudget::default();
    let report = analyze_unreal_installation(&context, Some(&res), &mut budget);

    assert_eq!(
        report.engine,
        EngineDetection::UnknownEngine {
            reason: UnknownReason::NoUeSignaturesFound,
        }
    );
    assert!(report.presence_proofs.is_empty());
}

#[test]
fn test_symlink_target_version_rejected() {
    let temp = tempdir().unwrap();
    let context = GameInstallationContext::new(temp.path().to_path_buf()).unwrap();

    let primary_path = temp
        .path()
        .join("Engine")
        .join("Binaries")
        .join("Win64")
        .join("Foo-Win64-Shipping.exe");
    write_minimal_pe(&primary_path, Architecture::X64);

    let real_version_path = temp.path().join("real_target.version");
    std::fs::write(
        &real_version_path,
        r#"{
            "MajorVersion": 5,
            "MinorVersion": 6,
            "PatchVersion": 1,
            "Changelist": 502094,
            "CompatibleChangelist": 0,
            "IsLicenseeVersion": 1,
            "IsPromotedBuild": 1,
            "BranchName": "++FactoryGame+rel-main"
        }"#,
    )
    .unwrap();

    let symlink_version_path = temp
        .path()
        .join("Engine")
        .join("Binaries")
        .join("Win64")
        .join("Foo-Win64-Shipping.version");

    #[cfg(unix)]
    let link_res = std::os::unix::fs::symlink(&real_version_path, &symlink_version_path);
    #[cfg(windows)]
    let link_res = std::os::windows::fs::symlink_file(&real_version_path, &symlink_version_path);

    if link_res.is_err() {
        // Unprivileged symlink creation may not be enabled on Windows CI without Developer Mode.
        return;
    }

    let res = resolved_with_arch(primary_path.to_str().unwrap(), Architecture::X64);
    let bound_primary = BoundPrimaryExecutable::from_resolved(&context, &res).unwrap();
    let bound_target_res = BoundTargetMetadata::from_primary(&bound_primary);
    assert!(
        matches!(bound_target_res, Err(TopologyError::SymlinkDisallowed(_))),
        "Symlink target version must be rejected with SymlinkDisallowed"
    );

    let mut budget = AnalysisBudget::default();
    let report = analyze_unreal_installation(&context, Some(&res), &mut budget);
    assert_eq!(
        report.engine,
        EngineDetection::UnknownEngine {
            reason: UnknownReason::NoUeSignaturesFound,
        }
    );
    assert!(report.presence_proofs.is_empty());
}

#[test]
fn test_corrupt_sibling_target_version_logs_diagnostic_and_ignored() {
    let temp = tempdir().unwrap();
    let context = GameInstallationContext::new(temp.path().to_path_buf()).unwrap();

    let primary_path = temp
        .path()
        .join("Engine")
        .join("Binaries")
        .join("Win64")
        .join("Foo-Win64-Shipping.exe");
    write_minimal_pe(&primary_path, Architecture::X64);

    let version_path = temp
        .path()
        .join("Engine")
        .join("Binaries")
        .join("Win64")
        .join("Foo-Win64-Shipping.version");
    std::fs::write(&version_path, b"{ invalid json contents").unwrap();

    let res = resolved_with_arch(primary_path.to_str().unwrap(), Architecture::X64);
    let mut budget = AnalysisBudget::default();
    let report = analyze_unreal_installation(&context, Some(&res), &mut budget);

    // Does not crash, fails closed
    assert_eq!(
        report.engine,
        EngineDetection::UnknownEngine {
            reason: UnknownReason::NoUeSignaturesFound,
        }
    );
    assert!(report.presence_proofs.is_empty());
}

#[test]
fn test_invalid_schema_sibling_target_version_rejected() {
    let temp = tempdir().unwrap();
    let context = GameInstallationContext::new(temp.path().to_path_buf()).unwrap();

    let primary_path = temp
        .path()
        .join("Engine")
        .join("Binaries")
        .join("Win64")
        .join("Foo-Win64-Shipping.exe");
    write_minimal_pe(&primary_path, Architecture::X64);

    let version_path = temp
        .path()
        .join("Engine")
        .join("Binaries")
        .join("Win64")
        .join("Foo-Win64-Shipping.version");

    // Single representative schema rejection through the full facade;
    // exhaustive schema variations are covered by metadata_files::test_parse_target_version_schema_rejections
    let invalid_json = r#"{"MajorVersion": 5, "MinorVersion": 6}"#;
    std::fs::write(&version_path, invalid_json).unwrap();
    let res = resolved_with_arch(primary_path.to_str().unwrap(), Architecture::X64);
    let mut budget = AnalysisBudget::default();
    let report = analyze_unreal_installation(&context, Some(&res), &mut budget);
    assert_eq!(
        report.engine,
        EngineDetection::UnknownEngine {
            reason: UnknownReason::NoUeSignaturesFound,
        },
        "Invalid json payload '{invalid_json}' must be rejected"
    );
}

#[test]
fn test_conflict_build_version_and_target_version() {
    let temp = tempdir().unwrap();
    let context = GameInstallationContext::new(temp.path().to_path_buf()).unwrap();

    let primary_path = temp
        .path()
        .join("Engine")
        .join("Binaries")
        .join("Win64")
        .join("Foo-Win64-Shipping.exe");
    write_minimal_pe(&primary_path, Architecture::X64);

    // Build.version has 5.5.3
    let build_version_path = temp
        .path()
        .join("Engine")
        .join("Build")
        .join("Build.version");
    std::fs::create_dir_all(build_version_path.parent().unwrap()).unwrap();
    std::fs::write(
        &build_version_path,
        r#"{"MajorVersion": 5, "MinorVersion": 5, "PatchVersion": 3}"#,
    )
    .unwrap();

    // Target version has 5.6.1
    let target_version_path = temp
        .path()
        .join("Engine")
        .join("Binaries")
        .join("Win64")
        .join("Foo-Win64-Shipping.version");
    std::fs::write(
        &target_version_path,
        r#"{
            "MajorVersion": 5,
            "MinorVersion": 6,
            "PatchVersion": 1,
            "Changelist": 502094,
            "CompatibleChangelist": 0,
            "IsLicenseeVersion": 1,
            "IsPromotedBuild": 1,
            "BranchName": "++FactoryGame+rel-main"
        }"#,
    )
    .unwrap();

    let res = resolved_with_arch(primary_path.to_str().unwrap(), Architecture::X64);
    let mut budget = AnalysisBudget::default();
    let report = analyze_unreal_installation(&context, Some(&res), &mut budget);

    // Both agree on major: 5. But minor conflicts (5 vs 6) at Authoritative tier!
    // Degradation to Generation { major: 5 }
    assert_eq!(
        report.engine,
        EngineDetection::Unreal(UnrealDetection::Generation { major: 5 })
    );
    assert_eq!(report.forensics.minor_status, ResolvedComponent::Conflicted);
    assert_eq!(report.forensics.records.len(), 2);
    for rec in &report.forensics.records {
        assert_eq!(rec.authority, Authority::Authoritative);
        assert_eq!(rec.major_disposition, ComponentDisposition::Decisive);
        assert_eq!(rec.minor_disposition, ComponentDisposition::ConflictMember);
    }
}

#[test]
fn test_no_primary_does_not_probe_target_version() {
    let temp = tempdir().unwrap();
    let context = GameInstallationContext::new(temp.path().to_path_buf()).unwrap();

    let version_path = temp
        .path()
        .join("Engine")
        .join("Binaries")
        .join("Win64")
        .join("Foo-Win64-Shipping.version");
    std::fs::create_dir_all(version_path.parent().unwrap()).unwrap();
    std::fs::write(
        &version_path,
        r#"{
            "MajorVersion": 5,
            "MinorVersion": 6,
            "PatchVersion": 1,
            "Changelist": 502094,
            "CompatibleChangelist": 0,
            "IsLicenseeVersion": 1,
            "IsPromotedBuild": 1,
            "BranchName": "++FactoryGame+rel-main"
        }"#,
    )
    .unwrap();

    let mut budget = AnalysisBudget::default();
    let report = analyze_unreal_installation(&context, None, &mut budget);

    assert_eq!(
        report.engine,
        EngineDetection::UnknownEngine {
            reason: UnknownReason::NoExecutablesFound,
        }
    );
    assert!(report.presence_proofs.is_empty());
}

#[test]
fn test_iostore_alone_does_not_create_ue5_verdict() {
    let temp = tempdir().unwrap();
    let context = GameInstallationContext::new(temp.path().to_path_buf()).unwrap();

    let primary_path = temp.path().join("Game.exe");
    write_minimal_pe(&primary_path, Architecture::X64);

    // Create IoStore container header with magic and version 6
    let utoc_path = temp.path().join("Content").join("Paks").join("global.utoc");
    std::fs::create_dir_all(utoc_path.parent().unwrap()).unwrap();
    let mut utoc_bytes = vec![0u8; 256];
    utoc_bytes[0..16].copy_from_slice(b"-==--==--==--==-");
    utoc_bytes[16] = 6; // ContainerHeaderVersion::Initial
    std::fs::write(&utoc_path, utoc_bytes).unwrap();

    let res = resolved_with_arch(primary_path.to_str().unwrap(), Architecture::X64);
    let mut budget = AnalysisBudget::default();
    let report = analyze_unreal_installation(&context, Some(&res), &mut budget);

    // Fails closed: IoStore alone cannot prove UE5
    assert_eq!(
        report.engine,
        EngineDetection::UnknownEngine {
            reason: UnknownReason::NoUeSignaturesFound,
        }
    );
}

#[test]
fn test_helper_branch_string_alone_does_not_create_ue5_verdict() {
    let temp = tempdir().unwrap();
    let context = GameInstallationContext::new(temp.path().to_path_buf()).unwrap();

    let primary_path = temp
        .path()
        .join("Engine")
        .join("Binaries")
        .join("Win64")
        .join("Game.exe");
    write_minimal_pe(&primary_path, Architecture::X64);

    // Helper containing custom branch string
    let helper_path = temp
        .path()
        .join("Engine")
        .join("Binaries")
        .join("Win64")
        .join("CrashReportClient.exe");
    let mut helper_bytes = create_minimal_valid_pe(Architecture::X64);
    helper_bytes.extend_from_slice(b"++FactoryGame+rel-main-anniversary-2026\0");
    std::fs::write(&helper_path, helper_bytes).unwrap();

    let res = resolved_with_arch(primary_path.to_str().unwrap(), Architecture::X64);
    let mut budget = AnalysisBudget::default();
    let report = analyze_unreal_installation(&context, Some(&res), &mut budget);

    assert_eq!(
        report.engine,
        EngineDetection::UnknownEngine {
            reason: UnknownReason::NoUeSignaturesFound,
        }
    );
}

#[test]
fn test_iris_replication_strings_alone_do_not_create_unreal_verdict() {
    let temp = tempdir().unwrap();
    let context = GameInstallationContext::new(temp.path().to_path_buf()).unwrap();

    let primary_path = temp.path().join("Game.exe");
    let mut pe_bytes = create_minimal_valid_pe(Architecture::X64);
    pe_bytes.extend_from_slice(b"IrisReplicationSystemNetBlobHandler\0");
    std::fs::write(&primary_path, pe_bytes).unwrap();

    let res = resolved_with_arch(primary_path.to_str().unwrap(), Architecture::X64);
    let mut budget = AnalysisBudget::default();
    let report = analyze_unreal_installation(&context, Some(&res), &mut budget);

    assert_eq!(
        report.engine,
        EngineDetection::UnknownEngine {
            reason: UnknownReason::NoUeSignaturesFound,
        }
    );
}
