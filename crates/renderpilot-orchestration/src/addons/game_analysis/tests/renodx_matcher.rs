use crate::addons::game_analysis::facade::{EngineDetection, UnrealDetection};
use crate::addons::game_analysis::renodx_matcher::{
    IncompatibleReason, InsufficientReason, RenoDxCompatibility, VersionRequirement,
    evaluate_ue_extended_fallback_compatibility,
};
use crate::addons::game_analysis::topology::executable::TargetPlatformDetection;

/// Regression test 6: RenoDX evaluator full safety matrix
#[test]
fn test_renodx_matcher_safety_matrix() {
    let win64 = TargetPlatformDetection::Win64Amd64;
    let unknown_plat = TargetPlatformDetection::Unknown;
    let req_any = VersionRequirement::AnySupportedUnreal;

    // 1. UE3 + Unknown platform -> Incompatible (unsupported generation blocks before platform check)
    let ue3 = EngineDetection::Unreal(UnrealDetection::Generation { major: 3 });
    assert!(matches!(
        evaluate_ue_extended_fallback_compatibility(&ue3, &unknown_plat, &req_any),
        RenoDxCompatibility::Incompatible {
            reason: IncompatibleReason::UnsupportedUnrealGeneration { detected: 3 }
        }
    ));

    // 2. UE6 + Win64 -> Incompatible (generation 6 is outside supported {4, 5} range)
    let ue6 = EngineDetection::Unreal(UnrealDetection::Generation { major: 6 });
    assert!(matches!(
        evaluate_ue_extended_fallback_compatibility(&ue6, &win64, &req_any),
        RenoDxCompatibility::Incompatible {
            reason: IncompatibleReason::UnsupportedUnrealGeneration { detected: 6 }
        }
    ));

    // 3. UE5 + Unknown platform -> InsufficientEvidence (platform is unknown)
    let ue5 = EngineDetection::Unreal(UnrealDetection::Exact {
        major: 5,
        minor: 4,
        patch: 3,
    });
    assert!(matches!(
        evaluate_ue_extended_fallback_compatibility(&ue5, &unknown_plat, &req_any),
        RenoDxCompatibility::InsufficientEvidence {
            reason: InsufficientReason::UnknownPlatformArchitecture
        }
    ));

    // 4. AmbiguousMajor([]) + Win64 -> InsufficientEvidence
    let ue_ambig = EngineDetection::Unreal(UnrealDetection::AmbiguousMajor { candidates: vec![] });
    assert!(matches!(
        evaluate_ue_extended_fallback_compatibility(&ue_ambig, &win64, &req_any),
        RenoDxCompatibility::InsufficientEvidence {
            reason: InsufficientReason::AmbiguousMajorVersion
        }
    ));

    // 5. UE5 (5.4.3) + Win64 + AnySupportedUnreal -> Compatible
    assert_eq!(
        evaluate_ue_extended_fallback_compatibility(&ue5, &win64, &req_any),
        RenoDxCompatibility::Compatible
    );

    // 6. UE4 + Win32 x86 -> Incompatible (UE Extended universal addon is unavailable for 32-bit: renodx-ue-extended.addon32 = 404)
    let win32 = TargetPlatformDetection::Win32X86;
    let ue4 = EngineDetection::Unreal(UnrealDetection::Exact {
        major: 4,
        minor: 27,
        patch: 2,
    });
    assert_eq!(
        evaluate_ue_extended_fallback_compatibility(&ue4, &win32, &req_any),
        RenoDxCompatibility::Incompatible {
            reason: IncompatibleReason::UnsupportedPlatformArchitecture {
                machine: 0x014C,
                is_64bit: false
            },
        }
    );
}

/// Regression test 14: RenoDX matcher requirement variants evaluation
#[test]
fn test_renodx_matcher_requirement_variants() {
    let win64 = TargetPlatformDetection::Win64Amd64;
    let ue5_exact = EngineDetection::Unreal(UnrealDetection::Exact {
        major: 5,
        minor: 4,
        patch: 3,
    });
    let ue5_mm = EngineDetection::Unreal(UnrealDetection::MajorMinor { major: 5, minor: 4 });
    let ue5_gen = EngineDetection::Unreal(UnrealDetection::Generation { major: 5 });

    // ExactGeneration
    assert_eq!(
        evaluate_ue_extended_fallback_compatibility(
            &ue5_exact,
            &win64,
            &VersionRequirement::ExactGeneration { major: 5 }
        ),
        RenoDxCompatibility::Compatible
    );
    assert_eq!(
        evaluate_ue_extended_fallback_compatibility(
            &ue5_exact,
            &win64,
            &VersionRequirement::ExactGeneration { major: 4 }
        ),
        RenoDxCompatibility::Incompatible {
            reason: IncompatibleReason::GenerationMismatch {
                detected: 5,
                expected: 4
            }
        }
    );

    // AtLeastMinorWithinGeneration
    assert_eq!(
        evaluate_ue_extended_fallback_compatibility(
            &ue5_exact,
            &win64,
            &VersionRequirement::AtLeastMinorWithinGeneration {
                major: 5,
                min_minor: 4
            }
        ),
        RenoDxCompatibility::Compatible
    );
    assert_eq!(
        evaluate_ue_extended_fallback_compatibility(
            &ue5_exact,
            &win64,
            &VersionRequirement::AtLeastMinorWithinGeneration {
                major: 5,
                min_minor: 5
            }
        ),
        RenoDxCompatibility::Incompatible {
            reason: IncompatibleReason::MinorTooLow {
                detected: 4,
                min_expected: 5
            }
        }
    );
    assert_eq!(
        evaluate_ue_extended_fallback_compatibility(
            &ue5_gen,
            &win64,
            &VersionRequirement::AtLeastMinorWithinGeneration {
                major: 5,
                min_minor: 4
            }
        ),
        RenoDxCompatibility::InsufficientEvidence {
            reason: InsufficientReason::MinorUnknown
        }
    );

    // ExactMajorMinor
    assert_eq!(
        evaluate_ue_extended_fallback_compatibility(
            &ue5_exact,
            &win64,
            &VersionRequirement::ExactMajorMinor { major: 5, minor: 4 }
        ),
        RenoDxCompatibility::Compatible
    );
    assert_eq!(
        evaluate_ue_extended_fallback_compatibility(
            &ue5_exact,
            &win64,
            &VersionRequirement::ExactMajorMinor { major: 5, minor: 3 }
        ),
        RenoDxCompatibility::Incompatible {
            reason: IncompatibleReason::MajorMinorMismatch {
                detected: (5, 4),
                expected: (5, 3)
            }
        }
    );
    assert_eq!(
        evaluate_ue_extended_fallback_compatibility(
            &ue5_gen,
            &win64,
            &VersionRequirement::ExactMajorMinor { major: 5, minor: 4 }
        ),
        RenoDxCompatibility::InsufficientEvidence {
            reason: InsufficientReason::MinorUnknown
        }
    );

    // ExactPatch
    assert_eq!(
        evaluate_ue_extended_fallback_compatibility(
            &ue5_exact,
            &win64,
            &VersionRequirement::ExactPatch {
                major: 5,
                minor: 4,
                patch: 3
            }
        ),
        RenoDxCompatibility::Compatible
    );
    assert_eq!(
        evaluate_ue_extended_fallback_compatibility(
            &ue5_exact,
            &win64,
            &VersionRequirement::ExactPatch {
                major: 5,
                minor: 4,
                patch: 2
            }
        ),
        RenoDxCompatibility::Incompatible {
            reason: IncompatibleReason::PatchMismatch {
                detected: (5, 4, 3),
                expected: (5, 4, 2)
            }
        }
    );
    assert_eq!(
        evaluate_ue_extended_fallback_compatibility(
            &ue5_mm,
            &win64,
            &VersionRequirement::ExactPatch {
                major: 5,
                minor: 4,
                patch: 3
            }
        ),
        RenoDxCompatibility::InsufficientEvidence {
            reason: InsufficientReason::PatchUnknown
        }
    );
}
