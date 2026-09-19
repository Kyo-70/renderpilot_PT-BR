use std::path::PathBuf;
use std::sync::LazyLock;

use crate::addons::game_analysis::context::GameInstallationContext;
use crate::addons::game_analysis::evidence::{
    Authority, EvidenceScope, EvidenceSource, ValidatedEvidence,
};
use crate::addons::game_analysis::evidence_set::GameEvidenceSet;
use crate::addons::game_analysis::facade::{UnrealDetection, UnrealPresenceProof};
use crate::addons::game_analysis::forensics::{
    ComponentDisposition, RejectionReason, resolve_unreal_version,
};
use crate::addons::game_analysis::parsers::tokens::VersionClaim;
use crate::addons::game_analysis::resolver::ResolvedComponent;

static TEST_CONTEXT: LazyLock<GameInstallationContext> =
    LazyLock::new(|| GameInstallationContext::synthetic(PathBuf::from("C:/Games/TestGame")));

fn make_test_evidence(
    auth: Authority,
    claim: VersionClaim,
    offset: u64,
) -> ValidatedEvidence<'static> {
    let (source, scope) = match auth {
        Authority::Authoritative => (
            EvidenceSource::BuildVersionFile,
            EvidenceScope::EngineMetadata,
        ),
        Authority::Strong => (
            EvidenceSource::CanonicalReleaseMarker,
            EvidenceScope::PrimaryExecutable,
        ),
        Authority::Supporting => (
            EvidenceSource::CanonicalReleaseMarker,
            EvidenceScope::EngineHelper,
        ),
        Authority::Weak => (
            EvidenceSource::ProjectDescriptorFile,
            EvidenceScope::ProjectMetadata,
        ),
    };
    ValidatedEvidence::synthetic(
        TEST_CONTEXT.id().clone(),
        claim,
        source,
        scope,
        PathBuf::from("C:/Games/TestGame/Binaries/Win64/Test.exe"),
        offset,
    )
}

fn make_test_set(evidences: Vec<ValidatedEvidence<'static>>) -> GameEvidenceSet<'static> {
    let mut set = GameEvidenceSet::new(&TEST_CONTEXT);
    for ev in evidences {
        set.insert(ev)
            .expect("test evidence must match TEST_CONTEXT");
    }
    set
}

// 1. Conflict barrier monotonicity: exhaustive verification across all maj1 != maj2 and weak_maj
#[test]
fn test_monotonic_conflict_barrier_exhaustive() {
    for maj1 in [4u32, 5u32] {
        for maj2 in [4u32, 5u32] {
            if maj1 == maj2 {
                continue;
            }
            for weak_maj in [4u32, 5u32] {
                let e1 = make_test_evidence(
                    Authority::Strong,
                    VersionClaim::Generation { major: maj1 },
                    0x1000,
                );
                let e2 = make_test_evidence(
                    Authority::Strong,
                    VersionClaim::Generation { major: maj2 },
                    0x2000,
                );
                let ew = make_test_evidence(
                    Authority::Weak,
                    VersionClaim::Generation { major: weak_maj },
                    0x3000,
                );

                let set = make_test_set(vec![e1, e2, ew]);
                let (res, forensics) = resolve_unreal_version(&set);
                assert!(matches!(res, UnrealDetection::AmbiguousMajor { .. }));
                assert_eq!(forensics.major_status, ResolvedComponent::Conflicted);
                assert_eq!(
                    forensics.records[2].major_disposition,
                    ComponentDisposition::Rejected(
                        RejectionReason::BlockedByHigherAuthorityConflict
                    )
                );
            }
        }
    }
}

// 2. Weaker contradiction cannot veto higher authority outcome: exhaustive verification
#[test]
fn test_weaker_contradiction_cannot_veto_outcome_exhaustive() {
    for auth_maj in [4u32, 5u32] {
        for weak_maj in 1u32..=9 {
            if auth_maj == weak_maj {
                continue;
            }
            let ea = make_test_evidence(
                Authority::Authoritative,
                VersionClaim::Generation { major: auth_maj },
                0,
            );
            let ew = make_test_evidence(
                Authority::Weak,
                VersionClaim::Generation { major: weak_maj },
                0x1000,
            );

            let set = make_test_set(vec![ea, ew]);
            let (res, forensics) = resolve_unreal_version(&set);

            assert_eq!(res, UnrealDetection::Generation { major: auth_maj });
            assert_eq!(
                forensics.records[1].major_disposition,
                ComponentDisposition::Rejected(RejectionReason::ContradictedByHigherTier)
            );
        }
    }
}

// 3. Semantic duplicate invariance: exhaustive verification across all generation and minor values
#[test]
fn test_semantic_duplicate_invariance_exhaustive() {
    for maj in [4u32, 5u32] {
        for min in 0u32..=27 {
            let e1 = make_test_evidence(
                Authority::Strong,
                VersionClaim::MajorMinor {
                    major: maj,
                    minor: min,
                },
                0x1000,
            );
            let e2 = make_test_evidence(
                Authority::Strong,
                VersionClaim::MajorMinor {
                    major: maj,
                    minor: min,
                },
                0x2000,
            );

            let set1 = make_test_set(vec![e1.clone()]);
            let set2 = make_test_set(vec![e1, e2]);

            let (res1, f1) = resolve_unreal_version(&set1);
            let (res2, f2) = resolve_unreal_version(&set2);

            assert_eq!(res1, res2);
            assert_eq!(f1.major_status, f2.major_status);
            assert_eq!(f1.minor_status, f2.minor_status);
        }
    }
}

// 4. Order permutation invariance: exhaustive verification across input orderings
#[test]
fn test_order_permutation_invariance_exhaustive() {
    for maj in [4u32, 5u32] {
        for min in 0u32..=27 {
            for pat in 0u32..=10 {
                let ea = make_test_evidence(
                    Authority::Authoritative,
                    VersionClaim::Generation { major: maj },
                    0,
                );
                let es = make_test_evidence(
                    Authority::Strong,
                    VersionClaim::MajorMinor {
                        major: maj,
                        minor: min,
                    },
                    0x1000,
                );
                let ew = make_test_evidence(
                    Authority::Weak,
                    VersionClaim::Exact {
                        major: maj,
                        minor: min,
                        patch: pat,
                    },
                    0x2000,
                );

                let perms = [
                    [ea.clone(), es.clone(), ew.clone()],
                    [ea.clone(), ew.clone(), es.clone()],
                    [es.clone(), ea.clone(), ew.clone()],
                    [es.clone(), ew.clone(), ea.clone()],
                    [ew.clone(), ea.clone(), es.clone()],
                    [ew, es, ea],
                ];

                let (baseline, _) = resolve_unreal_version(&make_test_set(perms[0].to_vec()));
                for p in &perms[1..] {
                    let (res, _) = resolve_unreal_version(&make_test_set(p.to_vec()));
                    assert_eq!(res, baseline);
                }
            }
        }
    }
}

// 6. Hierarchical child refinement lattice: Generation -> MajorMinor -> Exact resolution
#[test]
fn test_hierarchical_child_refinement_lattice_exhaustive() {
    for maj in [4u32, 5u32] {
        for min in 0u32..=27 {
            for pat in 0u32..=10 {
                let ea = make_test_evidence(
                    Authority::Authoritative,
                    VersionClaim::Generation { major: maj },
                    0,
                );
                let es = make_test_evidence(
                    Authority::Strong,
                    VersionClaim::MajorMinor {
                        major: maj,
                        minor: min,
                    },
                    0x1000,
                );
                let ew = make_test_evidence(
                    Authority::Supporting,
                    VersionClaim::Exact {
                        major: maj,
                        minor: min,
                        patch: pat,
                    },
                    0x2000,
                );

                let set = make_test_set(vec![ea, es, ew]);
                let (res, forensics) = resolve_unreal_version(&set);

                assert_eq!(
                    res,
                    UnrealDetection::Exact {
                        major: maj,
                        minor: min,
                        patch: pat,
                    }
                );
                assert_eq!(
                    forensics.records[0].major_disposition,
                    ComponentDisposition::Decisive
                );
                assert_eq!(
                    forensics.records[1].minor_disposition,
                    ComponentDisposition::Decisive
                );
                assert_eq!(
                    forensics.records[2].patch_disposition,
                    ComponentDisposition::Decisive
                );
            }
        }
    }
}

// 7. Production-reachable evidence hierarchy: uproject corroborated by primary release marker
#[test]
fn test_production_reachable_evidence_hierarchy_exhaustive() {
    for maj in [4u32, 5u32] {
        for min in 0u32..=27 {
            for pat in 0u32..=10 {
                let e_uproject = make_test_evidence(
                    Authority::Weak,
                    VersionClaim::MajorMinor {
                        major: maj,
                        minor: min,
                    },
                    0,
                );
                let e_marker = make_test_evidence(
                    Authority::Strong,
                    VersionClaim::Exact {
                        major: maj,
                        minor: min,
                        patch: pat,
                    },
                    0x1000,
                );

                let set = make_test_set(vec![e_uproject, e_marker]);
                let (res, forensics) = resolve_unreal_version(&set);

                assert_eq!(
                    res,
                    UnrealDetection::Exact {
                        major: maj,
                        minor: min,
                        patch: pat,
                    }
                );
                assert_eq!(
                    forensics.records[1].major_disposition,
                    ComponentDisposition::Decisive
                );
                assert_eq!(
                    forensics.records[0].major_disposition,
                    ComponentDisposition::Corroborating
                );
                assert_eq!(
                    forensics.records[1].minor_disposition,
                    ComponentDisposition::Decisive
                );
                assert_eq!(
                    forensics.records[0].minor_disposition,
                    ComponentDisposition::Corroborating
                );
                assert_eq!(
                    forensics.records[1].patch_disposition,
                    ComponentDisposition::Decisive
                );
            }
        }
    }
}

// UnrealPresenceProof total ordering and canonical deduplication
#[test]
fn test_unreal_presence_proof_total_order_and_dedup() {
    let p_desc = UnrealPresenceProof::ProjectDescriptor {
        path: PathBuf::from("Game.uproject"),
    };
    let p_meta = UnrealPresenceProof::CanonicalBuildVersion {
        path: PathBuf::from("Engine/Build/Build.version"),
    };
    let p_marker = UnrealPresenceProof::ReleaseMarker {
        source_file: PathBuf::from("Binaries/Win64/Game.exe"),
        offset: 100,
    };
    let p_pdb = UnrealPresenceProof::CanonicalPdb {
        source_file: PathBuf::from("Binaries/Win64/Game.exe"),
        pdb_name: "Game.pdb".to_string(),
    };
    let p_ue3 = UnrealPresenceProof::Ue3Package {
        source_file: PathBuf::from("CookedPC/Game.upk"),
        file_version: 800,
    };
    let p_iostore = UnrealPresenceProof::IoStoreContainer {
        source_file: PathBuf::from("Content/Paks/global.utoc"),
        toc_version: 6,
    };

    // Strict RFC ordering check
    assert!(p_desc < p_meta);
    assert!(p_meta < p_marker);
    assert!(p_marker < p_pdb);
    assert!(p_pdb < p_ue3);
    assert!(p_ue3 < p_iostore);

    // Deduplication and canonical sorting
    let mut proofs = vec![
        p_marker.clone(),
        p_desc.clone(),
        p_iostore.clone(),
        p_marker.clone(),
        p_meta.clone(),
        p_ue3.clone(),
        p_desc.clone(),
        p_pdb.clone(),
        p_iostore.clone(),
    ];
    proofs.sort();
    proofs.dedup();

    assert_eq!(
        proofs,
        vec![p_desc, p_meta, p_marker, p_pdb, p_ue3, p_iostore]
    );
}
