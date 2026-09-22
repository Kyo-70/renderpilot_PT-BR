use crate::addons::game_analysis::facade::{EngineDetection, UnrealDetection};
use crate::addons::game_analysis::renodx_matcher::{
    IncompatibleReason, InsufficientReason, RenoDxCompatibility,
    evaluate_ue_extended_fallback_compatibility,
};
use crate::addons::game_analysis::topology::executable::TargetPlatformDetection;

/// Regression test 6: RenoDX evaluator full safety matrix
#[test]
fn test_renodx_matcher_safety_matrix() {
    let win64 = TargetPlatformDetection::Win64Amd64;
    let unknown_plat = TargetPlatformDetection::Unknown;

    // 1. UE3 + Unknown platform -> Incompatible (unsupported generation blocks before platform check)
    let ue3 = EngineDetection::Unreal(UnrealDetection::Generation { major: 3 });
    assert!(matches!(
        evaluate_ue_extended_fallback_compatibility(&ue3, &unknown_plat),
        RenoDxCompatibility::Incompatible {
            reason: IncompatibleReason::UnsupportedUnrealGeneration { detected: 3 }
        }
    ));

    // 2. UE6 + Win64 -> Incompatible (generation 6 is outside supported {4, 5} range)
    let ue6 = EngineDetection::Unreal(UnrealDetection::Generation { major: 6 });
    assert!(matches!(
        evaluate_ue_extended_fallback_compatibility(&ue6, &win64),
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
        evaluate_ue_extended_fallback_compatibility(&ue5, &unknown_plat),
        RenoDxCompatibility::InsufficientEvidence {
            reason: InsufficientReason::UnknownPlatformArchitecture
        }
    ));

    // 4. AmbiguousMajor([]) + Win64 -> InsufficientEvidence
    let ue_ambig = EngineDetection::Unreal(UnrealDetection::AmbiguousMajor { candidates: vec![] });
    assert!(matches!(
        evaluate_ue_extended_fallback_compatibility(&ue_ambig, &win64),
        RenoDxCompatibility::InsufficientEvidence {
            reason: InsufficientReason::AmbiguousMajorVersion
        }
    ));

    // 5. UE5 (5.4.3) + Win64 -> Compatible
    assert_eq!(
        evaluate_ue_extended_fallback_compatibility(&ue5, &win64),
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
        evaluate_ue_extended_fallback_compatibility(&ue4, &win32),
        RenoDxCompatibility::Incompatible {
            reason: IncompatibleReason::UnsupportedPlatformArchitecture {
                machine: 0x014C,
                is_64bit: false,
            },
        }
    );
}
