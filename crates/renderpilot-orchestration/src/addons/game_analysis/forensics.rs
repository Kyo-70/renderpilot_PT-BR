//! Forensic observability, disposition labels, and detection reports.

use std::path::PathBuf;

use renderpilot_detection::pe::{
    CodeViewExtractionStatus, ScanTerminalCondition, SectionScanCoverage,
};

use crate::addons::game_analysis::evidence::Authority;
use crate::addons::game_analysis::evidence_set::GameEvidenceSet;
use crate::addons::game_analysis::facade::{EngineDetection, UnrealDetection, UnrealPresenceProof};
use crate::addons::game_analysis::parsers::tokens::VersionClaim;
use crate::addons::game_analysis::resolver::{ResolvedComponent, resolve_component};
use crate::addons::game_analysis::topology::executable::TargetPlatformDetection;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RejectionReason {
    ContradictsDecisiveParent,
    ContradictedByHigherTier,
    BlockedByHigherAuthorityConflict,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotEvaluatedReason {
    ParentComponentUnresolved,
    ComponentUnresolved,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComponentDisposition {
    /// Decisive evidence on the winning tier.
    Decisive,
    /// Corroborating evidence on same or lower tier agreeing with the decided value.
    Corroborating,
    /// Participant in an irreconcilable conflict on the winning tier.
    ConflictMember,
    /// Rejected due to contradiction with decided value or higher tier conflict barrier.
    Rejected(RejectionReason),
    /// Not evaluated because component or its parent could not be resolved.
    NotEvaluated(NotEvaluatedReason),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForensicEvidenceRecord {
    pub evidence_index: usize,
    pub authority: Authority,
    pub claim: VersionClaim,
    pub major_disposition: ComponentDisposition,
    pub minor_disposition: ComponentDisposition,
    pub patch_disposition: ComponentDisposition,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForensicReport {
    pub records: Vec<ForensicEvidenceRecord>,
    pub major_status: ResolvedComponent,
    pub minor_status: ResolvedComponent,
    pub patch_status: ResolvedComponent,
}

pub fn resolve_unreal_version(set: &GameEvidenceSet<'_>) -> (UnrealDetection, ForensicReport) {
    let evidences = set.evidences();

    // 1. Major resolution
    let (major_res, major_tier) = resolve_component(evidences, |c| Some(c.major()));

    // 2. Minor resolution (only among evidence compatible with proven Major)
    let (minor_res, minor_tier) = match major_res {
        ResolvedComponent::Determined(major_val) => resolve_component(evidences, |claim| {
            if claim.major() == major_val {
                claim.minor()
            } else {
                None
            }
        }),
        _ => (ResolvedComponent::Unresolved, None),
    };

    // 3. Patch resolution (only among evidence compatible with proven Major and Minor)
    let (patch_res, patch_tier) = match (major_res, minor_res) {
        (ResolvedComponent::Determined(major_val), ResolvedComponent::Determined(minor_val)) => {
            resolve_component(evidences, |claim| {
                if claim.major() == major_val && claim.minor() == Some(minor_val) {
                    claim.patch()
                } else {
                    None
                }
            })
        }
        _ => (ResolvedComponent::Unresolved, None),
    };

    // 4. Synthesis of final verdict with prefix degradation
    let detection = match (major_res, minor_res, patch_res) {
        (
            ResolvedComponent::Determined(maj),
            ResolvedComponent::Determined(min),
            ResolvedComponent::Determined(pat),
        ) => UnrealDetection::Exact {
            major: maj,
            minor: min,
            patch: pat,
        },
        (ResolvedComponent::Determined(maj), ResolvedComponent::Determined(min), _) => {
            UnrealDetection::MajorMinor {
                major: maj,
                minor: min,
            }
        }
        (ResolvedComponent::Determined(maj), _, _) => UnrealDetection::Generation { major: maj },
        (ResolvedComponent::Conflicted, _, _) => {
            let mut candidates: Vec<u32> = evidences
                .iter()
                .filter(|e| Some(e.authority()) == major_tier)
                .map(|e| e.claim().major())
                .collect();
            candidates.sort_unstable();
            candidates.dedup();
            UnrealDetection::AmbiguousMajor { candidates }
        }
        (ResolvedComponent::Unresolved, _, _) => UnrealDetection::Unknown,
    };

    // 5. Build exhaustive forensic report
    let records = evidences
        .iter()
        .enumerate()
        .map(|(idx, ev)| {
            let auth = ev.authority();
            let claim = ev.claim();

            // Evaluate Major
            let major_disp = match major_res {
                ResolvedComponent::Determined(maj) => {
                    if claim.major() == maj {
                        if Some(auth) == major_tier {
                            ComponentDisposition::Decisive
                        } else {
                            ComponentDisposition::Corroborating
                        }
                    } else {
                        ComponentDisposition::Rejected(RejectionReason::ContradictedByHigherTier)
                    }
                }
                ResolvedComponent::Conflicted => {
                    let m_tier = major_tier.unwrap();
                    if auth == m_tier {
                        ComponentDisposition::ConflictMember
                    } else if auth < m_tier {
                        ComponentDisposition::Rejected(
                            RejectionReason::BlockedByHigherAuthorityConflict,
                        )
                    } else {
                        ComponentDisposition::NotEvaluated(NotEvaluatedReason::ComponentUnresolved)
                    }
                }
                ResolvedComponent::Unresolved => {
                    ComponentDisposition::NotEvaluated(NotEvaluatedReason::ComponentUnresolved)
                }
            };

            // Evaluate Minor
            let minor_disp = match (major_res, claim.minor()) {
                (ResolvedComponent::Determined(maj), Some(min_val)) => {
                    if claim.major() != maj {
                        ComponentDisposition::Rejected(RejectionReason::ContradictsDecisiveParent)
                    } else {
                        match minor_res {
                            ResolvedComponent::Determined(decisive_min) => {
                                if min_val == decisive_min {
                                    if Some(auth) == minor_tier {
                                        ComponentDisposition::Decisive
                                    } else {
                                        ComponentDisposition::Corroborating
                                    }
                                } else {
                                    ComponentDisposition::Rejected(
                                        RejectionReason::ContradictedByHigherTier,
                                    )
                                }
                            }
                            ResolvedComponent::Conflicted => {
                                let m_tier = minor_tier.unwrap();
                                if auth == m_tier {
                                    ComponentDisposition::ConflictMember
                                } else if auth < m_tier {
                                    ComponentDisposition::Rejected(
                                        RejectionReason::BlockedByHigherAuthorityConflict,
                                    )
                                } else {
                                    ComponentDisposition::NotEvaluated(
                                        NotEvaluatedReason::ComponentUnresolved,
                                    )
                                }
                            }
                            ResolvedComponent::Unresolved => ComponentDisposition::NotEvaluated(
                                NotEvaluatedReason::ComponentUnresolved,
                            ),
                        }
                    }
                }
                (ResolvedComponent::Determined(_), None) => {
                    ComponentDisposition::NotEvaluated(NotEvaluatedReason::ComponentUnresolved)
                }
                _ => ComponentDisposition::NotEvaluated(
                    NotEvaluatedReason::ParentComponentUnresolved,
                ),
            };

            // Evaluate Patch
            let patch_disp = match (major_res, minor_res, claim.patch()) {
                (
                    ResolvedComponent::Determined(maj),
                    ResolvedComponent::Determined(min),
                    Some(pat_val),
                ) => {
                    if claim.major() != maj || claim.minor() != Some(min) {
                        ComponentDisposition::Rejected(RejectionReason::ContradictsDecisiveParent)
                    } else {
                        match patch_res {
                            ResolvedComponent::Determined(decisive_pat) => {
                                if pat_val == decisive_pat {
                                    if Some(auth) == patch_tier {
                                        ComponentDisposition::Decisive
                                    } else {
                                        ComponentDisposition::Corroborating
                                    }
                                } else {
                                    ComponentDisposition::Rejected(
                                        RejectionReason::ContradictedByHigherTier,
                                    )
                                }
                            }
                            ResolvedComponent::Conflicted => {
                                let p_tier = patch_tier.unwrap();
                                if auth == p_tier {
                                    ComponentDisposition::ConflictMember
                                } else if auth < p_tier {
                                    ComponentDisposition::Rejected(
                                        RejectionReason::BlockedByHigherAuthorityConflict,
                                    )
                                } else {
                                    ComponentDisposition::NotEvaluated(
                                        NotEvaluatedReason::ComponentUnresolved,
                                    )
                                }
                            }
                            ResolvedComponent::Unresolved => ComponentDisposition::NotEvaluated(
                                NotEvaluatedReason::ComponentUnresolved,
                            ),
                        }
                    }
                }
                (ResolvedComponent::Determined(_), ResolvedComponent::Determined(_), None) => {
                    ComponentDisposition::NotEvaluated(NotEvaluatedReason::ComponentUnresolved)
                }
                _ => ComponentDisposition::NotEvaluated(
                    NotEvaluatedReason::ParentComponentUnresolved,
                ),
            };

            ForensicEvidenceRecord {
                evidence_index: idx,
                authority: auth,
                claim,
                major_disposition: major_disp,
                minor_disposition: minor_disp,
                patch_disposition: patch_disp,
            }
        })
        .collect();

    let forensics = ForensicReport {
        records,
        major_status: major_res,
        minor_status: minor_res,
        patch_status: patch_res,
    };

    (detection, forensics)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DetectionDiagnostic {
    SectionScanBudgetExceeded {
        section_name: String,
        scanned_bytes: u64,
    },
    TruncatedSection {
        section_name: String,
        expected_bytes: u64,
        read_bytes: u64,
    },
    MalformedPeHeader {
        path: PathBuf,
        reason: String,
    },
    UnsupportedPlatformArchitecture {
        machine: u16,
        is_64bit: bool,
    },
    MarkerScanIncompleteBudgetExhausted {
        file_path: PathBuf,
    },
    MetadataReadFailure {
        path: PathBuf,
        reason: String,
    },
    AmbiguousProjectHierarchy {
        project_descriptors: Vec<PathBuf>,
    },
    DebugDirectoryExceedsLimit {
        actual_bytes: usize,
        max_bytes: usize,
    },
    TooManyDebugEntries {
        actual_entries: usize,
        max_entries: usize,
    },
    MalformedDebugDirectory {
        size: usize,
    },
    UnmappableDebugRva {
        rva: u32,
        size: u32,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectionReport {
    pub engine: EngineDetection,
    pub platform: TargetPlatformDetection,
    pub presence_proofs: Vec<UnrealPresenceProof>,
    pub forensics: ForensicReport,
    pub diagnostics: Vec<DetectionDiagnostic>,
    pub scan_coverage: Vec<SectionScanCoverage>,
}

/// Maps section scan coverage results to diagnostics.
pub fn map_coverage_to_diagnostics(
    coverage: &[SectionScanCoverage],
    diagnostics: &mut Vec<DetectionDiagnostic>,
) {
    for cov in coverage {
        match cov.terminal_condition {
            ScanTerminalCondition::BudgetExhausted => {
                diagnostics.push(DetectionDiagnostic::SectionScanBudgetExceeded {
                    section_name: cov.section_name.clone(),
                    scanned_bytes: cov.actual_scanned_bytes,
                });
            }
            ScanTerminalCondition::TruncatedFile => {
                diagnostics.push(DetectionDiagnostic::TruncatedSection {
                    section_name: cov.section_name.clone(),
                    expected_bytes: cov.total_raw_size,
                    read_bytes: cov.actual_scanned_bytes,
                });
            }
            _ => {}
        }
    }
}

/// Maps CodeView extraction status to diagnostics.
pub fn map_codeview_status_to_diagnostics(
    status: CodeViewExtractionStatus,
    diagnostics: &mut Vec<DetectionDiagnostic>,
) {
    match status {
        CodeViewExtractionStatus::DebugDirectoryExceedsLimit {
            actual_bytes,
            max_bytes,
        } => {
            diagnostics.push(DetectionDiagnostic::DebugDirectoryExceedsLimit {
                actual_bytes,
                max_bytes,
            });
        }
        CodeViewExtractionStatus::TooManyDebugEntries {
            actual_entries,
            max_entries,
        } => {
            diagnostics.push(DetectionDiagnostic::TooManyDebugEntries {
                actual_entries,
                max_entries,
            });
        }
        CodeViewExtractionStatus::MalformedDirectory { size } => {
            diagnostics.push(DetectionDiagnostic::MalformedDebugDirectory { size });
        }
        CodeViewExtractionStatus::UnmappableRva { rva, size } => {
            diagnostics.push(DetectionDiagnostic::UnmappableDebugRva { rva, size });
        }
        _ => {}
    }
}
