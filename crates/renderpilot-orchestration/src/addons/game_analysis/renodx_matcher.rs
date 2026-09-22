//! Compatibility evaluator for universal RenoDX UE Extended fallback (`ue-extended` / `_univ`).

use crate::addons::game_analysis::facade::{EngineDetection, UnrealDetection};
use crate::addons::game_analysis::topology::executable::TargetPlatformDetection;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenoDxCompatibility {
    Compatible,
    Incompatible { reason: IncompatibleReason },
    InsufficientEvidence { reason: InsufficientReason },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IncompatibleReason {
    UnsupportedPlatformArchitecture { machine: u16, is_64bit: bool },
    UnsupportedUnrealGeneration { detected: u32 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InsufficientReason {
    UnknownEngine,
    UnknownPlatformArchitecture,
    UnknownVersion,
    AmbiguousMajorVersion,
}

/// Pure evaluator for RenoDX universal UE Extended fallback compatibility.
///
/// SCOPE: Invoked exclusively for universal fallback (`ue-extended` / `_univ`).
/// Curated titles and legacy 32-bit profiles are handled by RenoDX catalog rules before this evaluator.
#[must_use]
pub fn evaluate_ue_extended_fallback_compatibility(
    engine: &EngineDetection,
    platform: &TargetPlatformDetection,
) -> RenoDxCompatibility {
    match engine {
        EngineDetection::UnknownEngine { .. } => RenoDxCompatibility::InsufficientEvidence {
            reason: InsufficientReason::UnknownEngine,
        },
        EngineDetection::Unreal(unreal) => {
            // Structural generation barrier: UE4 and UE5 only. Checked BEFORE platform.
            if let Some(major) = unreal.known_major().filter(|&m| !matches!(m, 4 | 5)) {
                return RenoDxCompatibility::Incompatible {
                    reason: IncompatibleReason::UnsupportedUnrealGeneration { detected: major },
                };
            }
            if let UnrealDetection::AmbiguousMajor { candidates } = unreal {
                if candidates.is_empty() {
                    return RenoDxCompatibility::InsufficientEvidence {
                        reason: InsufficientReason::AmbiguousMajorVersion,
                    };
                }
                if candidates.iter().all(|&c| !matches!(c, 4 | 5)) {
                    return RenoDxCompatibility::Incompatible {
                        reason: IncompatibleReason::UnsupportedUnrealGeneration {
                            detected: candidates[0],
                        },
                    };
                }
            }

            // Platform barrier: Win64 AMD64 only for universal UE Extended fallback.
            match platform {
                TargetPlatformDetection::Win32X86 => {
                    return RenoDxCompatibility::Incompatible {
                        reason: IncompatibleReason::UnsupportedPlatformArchitecture {
                            machine: 0x014C,
                            is_64bit: false,
                        },
                    };
                }
                TargetPlatformDetection::Unsupported { machine, is_64bit } => {
                    return RenoDxCompatibility::Incompatible {
                        reason: IncompatibleReason::UnsupportedPlatformArchitecture {
                            machine: *machine,
                            is_64bit: *is_64bit,
                        },
                    };
                }
                TargetPlatformDetection::Unknown => {
                    return RenoDxCompatibility::InsufficientEvidence {
                        reason: InsufficientReason::UnknownPlatformArchitecture,
                    };
                }
                TargetPlatformDetection::Win64Amd64 => {}
            }

            // Version requirement evaluation: Any supported Unreal (UE4/UE5).
            match unreal {
                UnrealDetection::Unknown => RenoDxCompatibility::InsufficientEvidence {
                    reason: InsufficientReason::UnknownVersion,
                },
                UnrealDetection::AmbiguousMajor { .. } => {
                    RenoDxCompatibility::InsufficientEvidence {
                        reason: InsufficientReason::AmbiguousMajorVersion,
                    }
                }
                _ => RenoDxCompatibility::Compatible,
            }
        }
    }
}
