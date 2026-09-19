//! Compatibility evaluator for universal RenoDX UE Extended fallback (`ue-extended` / `_univ`).

use crate::addons::game_analysis::facade::{EngineDetection, UnrealDetection};
use crate::addons::game_analysis::topology::executable::TargetPlatformDetection;

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub enum VersionRequirement {
    /// Any verified Unreal Engine of supported generations (UE4 or UE5).
    AnySupportedUnreal,
    /// Exact engine generation (e.g. UE 5).
    ExactGeneration { major: u32 },
    /// Minor version at or above threshold within generation (e.g. >= 5.4, but not 6.0).
    AtLeastMinorWithinGeneration { major: u32, min_minor: u32 },
    /// Exact Major.Minor pair (e.g. 4.26).
    ExactMajorMinor { major: u32, minor: u32 },
    /// Exact Patch (e.g. 5.4.3).
    ExactPatch { major: u32, minor: u32, patch: u32 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenoDxCompatibility {
    Compatible,
    Incompatible { reason: IncompatibleReason },
    InsufficientEvidence { reason: InsufficientReason },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IncompatibleReason {
    UnsupportedPlatformArchitecture {
        machine: u16,
        is_64bit: bool,
    },
    UnsupportedUnrealGeneration {
        detected: u32,
    },
    GenerationMismatch {
        detected: u32,
        expected: u32,
    },
    MinorTooLow {
        detected: u32,
        min_expected: u32,
    },
    MajorMinorMismatch {
        detected: (u32, u32),
        expected: (u32, u32),
    },
    PatchMismatch {
        detected: (u32, u32, u32),
        expected: (u32, u32, u32),
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InsufficientReason {
    UnknownEngine,
    UnknownPlatformArchitecture,
    UnknownVersion,
    AmbiguousMajorVersion,
    MinorUnknown,
    PatchUnknown,
}

/// Pure evaluator for RenoDX universal UE Extended fallback compatibility.
///
/// SCOPE: Invoked exclusively for universal fallback (`ue-extended` / `_univ`).
/// Curated titles and legacy 32-bit profiles are handled by RenoDX catalog rules before this evaluator.
#[must_use]
pub fn evaluate_ue_extended_fallback_compatibility(
    engine: &EngineDetection,
    platform: &TargetPlatformDetection,
    requirement: &VersionRequirement,
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

            // Version requirement evaluation
            match (unreal, requirement) {
                (_, VersionRequirement::AnySupportedUnreal) => match unreal {
                    UnrealDetection::Unknown => RenoDxCompatibility::InsufficientEvidence {
                        reason: InsufficientReason::UnknownVersion,
                    },
                    UnrealDetection::AmbiguousMajor { .. } => {
                        RenoDxCompatibility::InsufficientEvidence {
                            reason: InsufficientReason::AmbiguousMajorVersion,
                        }
                    }
                    _ => RenoDxCompatibility::Compatible,
                },

                (UnrealDetection::Unknown, _) => RenoDxCompatibility::InsufficientEvidence {
                    reason: InsufficientReason::UnknownVersion,
                },
                (UnrealDetection::AmbiguousMajor { .. }, _) => {
                    RenoDxCompatibility::InsufficientEvidence {
                        reason: InsufficientReason::AmbiguousMajorVersion,
                    }
                }

                // ExactGeneration
                (
                    UnrealDetection::Generation { major: m }
                    | UnrealDetection::MajorMinor { major: m, .. }
                    | UnrealDetection::Exact { major: m, .. },
                    VersionRequirement::ExactGeneration { major: req_m },
                ) => {
                    if *m == *req_m {
                        RenoDxCompatibility::Compatible
                    } else {
                        RenoDxCompatibility::Incompatible {
                            reason: IncompatibleReason::GenerationMismatch {
                                detected: *m,
                                expected: *req_m,
                            },
                        }
                    }
                }

                // AtLeastMinorWithinGeneration
                (
                    UnrealDetection::Generation { major: m },
                    VersionRequirement::AtLeastMinorWithinGeneration { major: req_m, .. },
                ) => {
                    if *m != *req_m {
                        RenoDxCompatibility::Incompatible {
                            reason: IncompatibleReason::GenerationMismatch {
                                detected: *m,
                                expected: *req_m,
                            },
                        }
                    } else {
                        RenoDxCompatibility::InsufficientEvidence {
                            reason: InsufficientReason::MinorUnknown,
                        }
                    }
                }
                (
                    UnrealDetection::MajorMinor { major: m, minor }
                    | UnrealDetection::Exact {
                        major: m, minor, ..
                    },
                    VersionRequirement::AtLeastMinorWithinGeneration {
                        major: req_m,
                        min_minor,
                    },
                ) => {
                    if *m != *req_m {
                        RenoDxCompatibility::Incompatible {
                            reason: IncompatibleReason::GenerationMismatch {
                                detected: *m,
                                expected: *req_m,
                            },
                        }
                    } else if *minor >= *min_minor {
                        RenoDxCompatibility::Compatible
                    } else {
                        RenoDxCompatibility::Incompatible {
                            reason: IncompatibleReason::MinorTooLow {
                                detected: *minor,
                                min_expected: *min_minor,
                            },
                        }
                    }
                }

                // ExactMajorMinor
                (
                    UnrealDetection::Generation { major: m },
                    VersionRequirement::ExactMajorMinor { major: req_m, .. },
                ) => {
                    if *m != *req_m {
                        RenoDxCompatibility::Incompatible {
                            reason: IncompatibleReason::GenerationMismatch {
                                detected: *m,
                                expected: *req_m,
                            },
                        }
                    } else {
                        RenoDxCompatibility::InsufficientEvidence {
                            reason: InsufficientReason::MinorUnknown,
                        }
                    }
                }
                (
                    UnrealDetection::MajorMinor { major: m, minor }
                    | UnrealDetection::Exact {
                        major: m, minor, ..
                    },
                    VersionRequirement::ExactMajorMinor {
                        major: req_m,
                        minor: req_min,
                    },
                ) => {
                    if *m == *req_m && *minor == *req_min {
                        RenoDxCompatibility::Compatible
                    } else if *m != *req_m {
                        RenoDxCompatibility::Incompatible {
                            reason: IncompatibleReason::GenerationMismatch {
                                detected: *m,
                                expected: *req_m,
                            },
                        }
                    } else {
                        RenoDxCompatibility::Incompatible {
                            reason: IncompatibleReason::MajorMinorMismatch {
                                detected: (*m, *minor),
                                expected: (*req_m, *req_min),
                            },
                        }
                    }
                }

                // ExactPatch
                (
                    UnrealDetection::Generation { major: m },
                    VersionRequirement::ExactPatch { major: req_m, .. },
                ) => {
                    if *m != *req_m {
                        RenoDxCompatibility::Incompatible {
                            reason: IncompatibleReason::GenerationMismatch {
                                detected: *m,
                                expected: *req_m,
                            },
                        }
                    } else {
                        RenoDxCompatibility::InsufficientEvidence {
                            reason: InsufficientReason::MinorUnknown,
                        }
                    }
                }
                (
                    UnrealDetection::MajorMinor { major: m, minor },
                    VersionRequirement::ExactPatch {
                        major: req_m,
                        minor: req_min,
                        ..
                    },
                ) => {
                    if *m != *req_m {
                        RenoDxCompatibility::Incompatible {
                            reason: IncompatibleReason::GenerationMismatch {
                                detected: *m,
                                expected: *req_m,
                            },
                        }
                    } else if *minor != *req_min {
                        RenoDxCompatibility::Incompatible {
                            reason: IncompatibleReason::MajorMinorMismatch {
                                detected: (*m, *minor),
                                expected: (*req_m, *req_min),
                            },
                        }
                    } else {
                        RenoDxCompatibility::InsufficientEvidence {
                            reason: InsufficientReason::PatchUnknown,
                        }
                    }
                }
                (
                    UnrealDetection::Exact {
                        major: m,
                        minor,
                        patch,
                    },
                    VersionRequirement::ExactPatch {
                        major: req_m,
                        minor: req_min,
                        patch: req_p,
                    },
                ) => {
                    if *m == *req_m && *minor == *req_min && *patch == *req_p {
                        RenoDxCompatibility::Compatible
                    } else if *m != *req_m {
                        RenoDxCompatibility::Incompatible {
                            reason: IncompatibleReason::GenerationMismatch {
                                detected: *m,
                                expected: *req_m,
                            },
                        }
                    } else if *minor != *req_min {
                        RenoDxCompatibility::Incompatible {
                            reason: IncompatibleReason::MajorMinorMismatch {
                                detected: (*m, *minor),
                                expected: (*req_m, *req_min),
                            },
                        }
                    } else {
                        RenoDxCompatibility::Incompatible {
                            reason: IncompatibleReason::PatchMismatch {
                                detected: (*m, *minor, *patch),
                                expected: (*req_m, *req_min, *req_p),
                            },
                        }
                    }
                }
            }
        }
    }
}
