use super::read_guards::{
    FinalReadGuardInput, InitialReadGuardInput, ReadGuardCompanions, ReadGuardTransition,
    test_bind_initial, test_validate_final,
};
use crate::InstalledAddonMutation;
use crate::{
    BeginFileMutationPreparation, BeginSharedVulkanMutation, SharedArtifactMutation,
    SharedVulkanMutationScope, SqliteStorage,
};
use renderpilot_application::{GameRepository, InstalledAddonRepository, ProxyTopologyRepository};
use renderpilot_domain::{
    AddonKind, ComponentFile, ComponentId, ComponentKind, ComponentRollbackBaseline, FileReceipt,
    GameId, GameIdentity, GameInstallation, GameProxyTopology, GameRuntime, InstalledAddon,
    Launcher, LibraryComponent, LibraryTechnology, ManagedAddonFile, ManagedFileBaseline, PathRef,
    PeerCatalogDeletedBaseline, PeerCatalogPhysicalContract, PeerCatalogRollbackClaim,
    PeerEndpointIntent, PeerFileImage, PeerReadGuardEvidence, PeerReadGuardExpectation,
    PlannedGameProxyTopology, Platform, ProxyImplementation, ProxyLink, ProxyPeerRoute,
    ProxyRootPrestate, Sha256Hash, Swappability,
};

const ROOT: &str = "C:/game";

fn hash(byte: char) -> Sha256Hash {
    Sha256Hash::new(byte.to_string().repeat(64)).expect("hash")
}

fn path(value: &str) -> PathRef {
    PathRef::new(value).expect("path")
}

fn topology(game_id: &GameId) -> GameProxyTopology {
    let root_slot = path("C:/game/outer.dll");
    GameProxyTopology {
        id: "topology:test".to_owned(),
        game_id: game_id.clone(),
        root_slot: root_slot.clone(),
        outer: ProxyLink {
            implementation: ProxyImplementation::OptiScaler,
            path: root_slot.clone(),
            receipt: FileReceipt::owned("outer-id", hash('a')).expect("outer receipt"),
        },
        downstream: Some(ProxyLink {
            implementation: ProxyImplementation::ReShade,
            path: path("C:/game/downstream.dll"),
            receipt: FileReceipt::owned("downstream-id", hash('b')).expect("downstream receipt"),
        }),
        downstream_origin: Some(root_slot),
        root_prestate: ProxyRootPrestate::Absent,
    }
}

fn peer(game_id: &GameId, managed_path: &str) -> InstalledAddon {
    InstalledAddon::new(
        game_id.clone(),
        AddonKind::Luma,
        path("C:/game/peer.addon64"),
    )
    .try_with_managed_files(vec![
        ManagedAddonFile::owned(
            path(managed_path),
            ManagedFileBaseline::Present { sha256: hash('c') },
            hash('d'),
        ),
        ManagedAddonFile::reused(path("C:/game/reused.dll"), hash('e')),
    ])
    .expect("managed peer")
}

fn peer_with_managed_paths(
    game_id: &GameId,
    owned_path: &str,
    reused_path: &str,
) -> InstalledAddon {
    InstalledAddon::new(
        game_id.clone(),
        AddonKind::Luma,
        path("C:/game/peer.addon64"),
    )
    .try_with_managed_files(vec![
        ManagedAddonFile::owned(
            path(owned_path),
            ManagedFileBaseline::Present { sha256: hash('c') },
            hash('d'),
        ),
        ManagedAddonFile::reused(path(reused_path), hash('e')),
    ])
    .expect("managed peer")
}

fn topology_at_root(game_id: &GameId, root: &str, digest: char) -> GameProxyTopology {
    let outer_path = path(&format!("{root}/outer.dll"));
    GameProxyTopology {
        id: "topology:rooted-guard-test".to_owned(),
        game_id: game_id.clone(),
        root_slot: outer_path.clone(),
        outer: ProxyLink {
            implementation: ProxyImplementation::OptiScaler,
            path: outer_path,
            receipt: FileReceipt::owned("outer-id", hash(digest)).expect("outer receipt"),
        },
        downstream: None,
        downstream_origin: None,
        root_prestate: ProxyRootPrestate::Absent,
    }
}

fn catalog_contract_at_root(
    game_id: &GameId,
    root: &str,
    physical_program: &[PeerEndpointIntent],
    preimages: &[Option<PeerFileImage>],
) -> PeerCatalogPhysicalFixture {
    let component_id = ComponentId::new("component:rooted-guard-test").expect("component id");
    let file_path = path(&format!("{root}/catalog.dll"));
    let digest = hash('a');
    let component = LibraryComponent::new(
        component_id.clone(),
        game_id.clone(),
        ComponentKind::NativeLibrary,
        LibraryTechnology::DlssSuperResolution,
        Swappability::Swappable,
    )
    .with_file(ComponentFile::new(file_path.clone()).with_sha256(digest.clone()));
    let baseline =
        ComponentRollbackBaseline::new(vec![ComponentFile::new(file_path).with_sha256(digest)]);
    let claim = PeerCatalogRollbackClaim::new(
        vec![component],
        vec![PeerCatalogDeletedBaseline::new(component_id, baseline)],
    )
    .expect("catalog claim");
    let physical =
        PeerCatalogPhysicalContract::derive(&claim, None, None, physical_program, preimages)
            .expect("catalog physical contract");
    PeerCatalogPhysicalFixture { claim, physical }
}

struct PeerCatalogPhysicalFixture {
    claim: PeerCatalogRollbackClaim,
    physical: PeerCatalogPhysicalContract,
}

fn fixture() -> (
    GameId,
    InstalledAddon,
    InstalledAddon,
    GameProxyTopology,
    PlannedGameProxyTopology,
    Vec<PeerEndpointIntent>,
    Vec<String>,
) {
    let game_id = GameId::new("test:guards").expect("game id");
    let before = peer(&game_id, "C:/game/owned.dll");
    let after = before
        .clone()
        .with_created_file(path("C:/game/changed.dll"));
    let topology = topology(&game_id);
    let planned = PlannedGameProxyTopology::Exact(topology.clone());
    let intent = PeerEndpointIntent::create(
        path("C:/game/changed.dll"),
        renderpilot_domain::PeerEndpointRole::Disjoint,
        Some(hash('f')),
        Some(3),
    )
    .expect("intent");
    (
        game_id,
        before,
        after,
        topology,
        planned,
        vec![intent],
        vec![ROOT.to_owned(), "C:/payload".to_owned()],
    )
}

fn evidence_for(
    requirements: &[renderpilot_domain::PeerReadGuardRequirement],
    identity_suffix: &str,
) -> Vec<PeerReadGuardEvidence> {
    requirements
        .iter()
        .map(|requirement| {
            let observed = match requirement.expectation() {
                PeerReadGuardExpectation::Absent => None,
                PeerReadGuardExpectation::Digest { sha256 } => Some(
                    PeerFileImage::new(format!("{identity_suffix}-digest"), sha256.clone(), 3)
                        .expect("digest image"),
                ),
                PeerReadGuardExpectation::Receipt { identity, sha256 } => Some(
                    PeerFileImage::new(identity.clone(), sha256.clone(), 3).expect("receipt image"),
                ),
            };
            PeerReadGuardEvidence::new(requirement.path().clone(), observed)
        })
        .collect()
}

fn requirements_for(
    before: &InstalledAddon,
    after: &InstalledAddon,
    topology: &GameProxyTopology,
    planned: &PlannedGameProxyTopology,
    intents: &[PeerEndpointIntent],
) -> Vec<renderpilot_domain::PeerReadGuardRequirement> {
    renderpilot_domain::required_read_guards(
        Some(before),
        Some(after),
        Some(topology),
        Some(planned),
        ProxyPeerRoute::DurableDisjoint,
        intents,
    )
    .expect("requirements")
}

#[test]
fn bind_initial_derives_outer_downstream_owned_baseline_and_reused_live_guards() {
    let (_, before, after, topology, planned, intents, roots) = fixture();
    let requirements = requirements_for(&before, &after, &topology, &planned, &intents);
    let evidence = evidence_for(&requirements, "initial");
    let projection = test_bind_initial(InitialReadGuardInput {
        canonical_game_root: ROOT,
        transition: ReadGuardTransition {
            sealed_roots: &roots,
            before_peer: Some(&before),
            after_peer: Some(&after),
            before_topology: Some(&topology),
            planned_after_topology: Some(&planned),
            route: ProxyPeerRoute::DurableDisjoint,
            intents: &intents,
            program_before: &[],
        },
        initial_evidence: &evidence,
        companions: ReadGuardCompanions::default(),
    })
    .expect("initial guards");
    let paths = projection
        .requirements
        .iter()
        .map(|requirement| requirement.path().as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        paths,
        vec![
            "C:/game/downstream.dll",
            "C:/game/outer.dll",
            "C:/game/owned.dll.bak",
            "C:/game/reused.dll",
        ]
    );
    assert_eq!(projection.initial_evidence, evidence);
}

#[test]
fn two_roots_accept_external_managed_guards_at_initial_and_final_validation() {
    let (game_id, _, _, topology, planned, intents, roots) = fixture();
    let before = peer_with_managed_paths(&game_id, "C:/payload/owned.dll", "C:/payload/reused.dll");
    let after = before
        .clone()
        .with_created_file(path("C:/game/changed.dll"));
    let requirements = requirements_for(&before, &after, &topology, &planned, &intents);
    assert!(requirements.iter().any(|requirement| {
        requirement.path().as_str() == "C:/payload/owned.dll.bak"
            && !requirement.is_topology_sourced()
    }));
    assert!(requirements.iter().any(|requirement| {
        requirement.path().as_str() == "C:/payload/reused.dll" && !requirement.is_topology_sourced()
    }));
    let initial = evidence_for(&requirements, "external-initial");
    test_bind_initial(InitialReadGuardInput {
        canonical_game_root: ROOT,
        transition: ReadGuardTransition {
            sealed_roots: &roots,
            before_peer: Some(&before),
            after_peer: Some(&after),
            before_topology: Some(&topology),
            planned_after_topology: Some(&planned),
            route: ProxyPeerRoute::DurableDisjoint,
            intents: &intents,
            program_before: &[],
        },
        initial_evidence: &initial,
        companions: ReadGuardCompanions::default(),
    })
    .expect("external managed guards at initial binding");
    test_validate_final(FinalReadGuardInput {
        canonical_game_root: &path(ROOT),
        transition: ReadGuardTransition {
            sealed_roots: &roots,
            before_peer: Some(&before),
            after_peer: Some(&after),
            before_topology: Some(&topology),
            planned_after_topology: Some(&planned),
            route: ProxyPeerRoute::DurableDisjoint,
            intents: &intents,
            program_before: &[],
        },
        stored_requirements: &requirements,
        initial_evidence: &initial,
        final_evidence: &evidence_for(&requirements, "external-final"),
        companions: ReadGuardCompanions::default(),
    })
    .expect("external managed guards at final validation");
}

#[test]
fn external_guards_outside_or_missing_the_second_root_are_rejected() {
    let (game_id, _, _, topology, planned, intents, roots) = fixture();
    let before = peer_with_managed_paths(&game_id, "C:/outside/owned.dll", "C:/outside/reused.dll");
    let after = before
        .clone()
        .with_created_file(path("C:/game/changed.dll"));
    let requirements = requirements_for(&before, &after, &topology, &planned, &intents);
    let evidence = evidence_for(&requirements, "outside");
    assert!(
        test_bind_initial(InitialReadGuardInput {
            canonical_game_root: ROOT,
            transition: ReadGuardTransition {
                sealed_roots: &roots,
                before_peer: Some(&before),
                after_peer: Some(&after),
                before_topology: Some(&topology),
                planned_after_topology: Some(&planned),
                route: ProxyPeerRoute::DurableDisjoint,
                intents: &intents,
                program_before: &[],
            },
            initial_evidence: &evidence,
            companions: ReadGuardCompanions::default(),
        })
        .is_err()
    );

    let valid_before =
        peer_with_managed_paths(&game_id, "C:/payload/owned.dll", "C:/payload/reused.dll");
    let valid_after = valid_before
        .clone()
        .with_created_file(path("C:/game/changed.dll"));
    let valid_requirements =
        requirements_for(&valid_before, &valid_after, &topology, &planned, &intents);
    let valid_initial = evidence_for(&valid_requirements, "missing-root");
    assert!(
        test_validate_final(FinalReadGuardInput {
            canonical_game_root: &path(ROOT),
            transition: ReadGuardTransition {
                sealed_roots: &[ROOT.to_owned()],
                before_peer: Some(&valid_before),
                after_peer: Some(&valid_after),
                before_topology: Some(&topology),
                planned_after_topology: Some(&planned),
                route: ProxyPeerRoute::DurableDisjoint,
                intents: &intents,
                program_before: &[],
            },
            stored_requirements: &valid_requirements,
            initial_evidence: &valid_initial,
            final_evidence: &valid_initial,
            companions: ReadGuardCompanions::default(),
        })
        .is_err()
    );
}

#[test]
fn sealed_root_list_is_revalidated_at_initial_and_final_boundaries() {
    let (_, before, after, topology, planned, intents, valid_roots) = fixture();
    let requirements = requirements_for(&before, &after, &topology, &planned, &intents);
    let evidence = evidence_for(&requirements, "root-list");
    let invalid_roots = [
        Vec::new(),
        vec![
            ROOT.to_owned(),
            "C:/payload".to_owned(),
            "C:/third".to_owned(),
        ],
        vec![ROOT.to_owned(), "c:/GAME".to_owned()],
        vec!["C:/other".to_owned(), "C:/payload".to_owned()],
    ];
    for roots in invalid_roots {
        assert!(
            test_bind_initial(InitialReadGuardInput {
                canonical_game_root: ROOT,
                transition: ReadGuardTransition {
                    sealed_roots: &roots,
                    before_peer: Some(&before),
                    after_peer: Some(&after),
                    before_topology: Some(&topology),
                    planned_after_topology: Some(&planned),
                    route: ProxyPeerRoute::DurableDisjoint,
                    intents: &intents,
                    program_before: &[],
                },
                initial_evidence: &evidence,
                companions: ReadGuardCompanions::default(),
            })
            .is_err(),
            "initial roots: {roots:?}"
        );
        assert!(
            test_validate_final(FinalReadGuardInput {
                canonical_game_root: &path(ROOT),
                transition: ReadGuardTransition {
                    sealed_roots: &roots,
                    before_peer: Some(&before),
                    after_peer: Some(&after),
                    before_topology: Some(&topology),
                    planned_after_topology: Some(&planned),
                    route: ProxyPeerRoute::DurableDisjoint,
                    intents: &intents,
                    program_before: &[],
                },
                stored_requirements: &requirements,
                initial_evidence: &evidence,
                final_evidence: &evidence,
                companions: ReadGuardCompanions::default(),
            })
            .is_err(),
            "final roots: {roots:?}"
        );
    }
    test_validate_final(FinalReadGuardInput {
        canonical_game_root: &path(ROOT),
        transition: ReadGuardTransition {
            sealed_roots: &valid_roots,
            before_peer: Some(&before),
            after_peer: Some(&after),
            before_topology: Some(&topology),
            planned_after_topology: Some(&planned),
            route: ProxyPeerRoute::DurableDisjoint,
            intents: &intents,
            program_before: &[],
        },
        stored_requirements: &requirements,
        initial_evidence: &evidence,
        final_evidence: &evidence,
        companions: ReadGuardCompanions::default(),
    })
    .expect("valid roots remain accepted");
}

#[test]
fn topology_and_merged_guards_under_second_root_are_rejected_at_both_boundaries() {
    let (game_id, _, _, _, _, intents, roots) = fixture();
    let topology = topology_at_root(&game_id, "C:/payload", 'a');
    let planned = PlannedGameProxyTopology::Exact(topology.clone());
    let before = peer(&game_id, "C:/game/owned.dll");
    let after = before
        .clone()
        .with_created_file(path("C:/game/changed.dll"));
    let requirements = requirements_for(&before, &after, &topology, &planned, &intents);
    let topology_evidence = evidence_for(&requirements, "topology-second-root");
    assert!(requirements.iter().any(|requirement| {
        requirement.path().as_str() == "C:/payload/outer.dll" && requirement.is_topology_sourced()
    }));
    assert!(
        test_bind_initial(InitialReadGuardInput {
            canonical_game_root: ROOT,
            transition: ReadGuardTransition {
                sealed_roots: &roots,
                before_peer: Some(&before),
                after_peer: Some(&after),
                before_topology: Some(&topology),
                planned_after_topology: Some(&planned),
                route: ProxyPeerRoute::DurableDisjoint,
                intents: &intents,
                program_before: &[],
            },
            initial_evidence: &topology_evidence,
            companions: ReadGuardCompanions::default(),
        })
        .is_err()
    );
    assert!(
        test_validate_final(FinalReadGuardInput {
            canonical_game_root: &path(ROOT),
            transition: ReadGuardTransition {
                sealed_roots: &roots,
                before_peer: Some(&before),
                after_peer: Some(&after),
                before_topology: Some(&topology),
                planned_after_topology: Some(&planned),
                route: ProxyPeerRoute::DurableDisjoint,
                intents: &intents,
                program_before: &[],
            },
            stored_requirements: &requirements,
            initial_evidence: &topology_evidence,
            final_evidence: &topology_evidence,
            companions: ReadGuardCompanions::default(),
        })
        .is_err()
    );

    let merged_topology = topology_at_root(&game_id, "C:/payload", 'e');
    let merged_planned = PlannedGameProxyTopology::Exact(merged_topology.clone());
    let merged_before =
        peer_with_managed_paths(&game_id, "C:/game/owned.dll", "C:/payload/outer.dll");
    let merged_after = merged_before
        .clone()
        .with_created_file(path("C:/game/changed.dll"));
    let merged_requirements = requirements_for(
        &merged_before,
        &merged_after,
        &merged_topology,
        &merged_planned,
        &intents,
    );
    let merged_evidence = evidence_for(&merged_requirements, "merged-second-root");
    let merged = merged_requirements
        .iter()
        .find(|requirement| requirement.path().as_str() == "C:/payload/outer.dll")
        .expect("merged topology and managed guard");
    assert!(merged.is_topology_sourced());
    assert!(merged.sources().len() > 1);
    assert!(
        test_bind_initial(InitialReadGuardInput {
            canonical_game_root: ROOT,
            transition: ReadGuardTransition {
                sealed_roots: &roots,
                before_peer: Some(&merged_before),
                after_peer: Some(&merged_after),
                before_topology: Some(&merged_topology),
                planned_after_topology: Some(&merged_planned),
                route: ProxyPeerRoute::DurableDisjoint,
                intents: &intents,
                program_before: &[],
            },
            initial_evidence: &merged_evidence,
            companions: ReadGuardCompanions::default(),
        })
        .is_err()
    );
    assert!(
        test_validate_final(FinalReadGuardInput {
            canonical_game_root: &path(ROOT),
            transition: ReadGuardTransition {
                sealed_roots: &roots,
                before_peer: Some(&merged_before),
                after_peer: Some(&merged_after),
                before_topology: Some(&merged_topology),
                planned_after_topology: Some(&merged_planned),
                route: ProxyPeerRoute::DurableDisjoint,
                intents: &intents,
                program_before: &[],
            },
            stored_requirements: &merged_requirements,
            initial_evidence: &merged_evidence,
            final_evidence: &merged_evidence,
            companions: ReadGuardCompanions::default(),
        })
        .is_err()
    );
}

#[test]
fn catalog_guards_under_second_root_are_accepted_at_initial_and_final_validation() {
    let game_id = GameId::new("test:catalog-rooted-guards").expect("game id");
    let intent = PeerEndpointIntent::remove(
        path("C:/payload/catalog.dll.bak"),
        renderpilot_domain::PeerEndpointRole::Disjoint,
    )
    .expect("intent");
    let intents = vec![intent];
    let catalog_preimages = vec![Some(
        PeerFileImage::new("catalog-sidecar", hash('a'), 3).expect("catalog preimage"),
    )];
    let catalog = catalog_contract_at_root(&game_id, "C:/payload", &intents, &catalog_preimages);
    let roots = vec![ROOT.to_owned(), "C:/payload".to_owned()];
    let requirements = renderpilot_domain::required_read_guards_with_catalog(
        None,
        None,
        None,
        None,
        ProxyPeerRoute::DurableDisjoint,
        &intents,
        Some(&catalog.physical),
    )
    .expect("catalog requirements");
    assert!(requirements.iter().all(|requirement| {
        requirement.path().as_str().starts_with("C:/payload/") && !requirement.is_topology_sourced()
    }));
    let initial = evidence_for(&requirements, "catalog-initial");
    test_bind_initial(InitialReadGuardInput {
        canonical_game_root: ROOT,
        transition: ReadGuardTransition {
            sealed_roots: &roots,
            before_peer: None,
            after_peer: None,
            before_topology: None,
            planned_after_topology: None,
            route: ProxyPeerRoute::DurableDisjoint,
            intents: &intents,
            program_before: &[],
        },
        initial_evidence: &initial,
        companions: ReadGuardCompanions {
            catalog: Some(&catalog.physical),
            ..Default::default()
        },
    })
    .expect("catalog guards at initial binding");
    test_validate_final(FinalReadGuardInput {
        canonical_game_root: &path(ROOT),
        transition: ReadGuardTransition {
            sealed_roots: &roots,
            before_peer: None,
            after_peer: None,
            before_topology: None,
            planned_after_topology: None,
            route: ProxyPeerRoute::DurableDisjoint,
            intents: &intents,
            program_before: &catalog_preimages,
        },
        stored_requirements: &requirements,
        initial_evidence: &initial,
        final_evidence: &evidence_for(&requirements, "catalog-final"),
        companions: ReadGuardCompanions {
            catalog_claim: Some(&catalog.claim),
            ..Default::default()
        },
    })
    .expect("catalog guards at final validation");
}

#[test]
fn bind_initial_rejects_omitted_extra_reordered_duplicate_and_alternate_paths() {
    let (_, before, after, topology, planned, intents, roots) = fixture();
    let requirements = requirements_for(&before, &after, &topology, &planned, &intents);
    let valid = evidence_for(&requirements, "initial");
    let mut omitted = valid.clone();
    omitted.pop();
    let mut extra = valid.clone();
    extra.push(valid[0].clone());
    let mut reordered = valid.clone();
    reordered.swap(0, 1);
    let mut duplicate = valid.clone();
    duplicate[1] = duplicate[0].clone();
    let mut alternate = valid.clone();
    alternate[0] = PeerReadGuardEvidence::new(
        path("C:/game//downstream.dll"),
        valid[0].observed().cloned(),
    );
    for evidence in [omitted, extra, reordered, duplicate, alternate] {
        assert!(
            test_bind_initial(InitialReadGuardInput {
                canonical_game_root: ROOT,
                transition: ReadGuardTransition {
                    sealed_roots: &roots,
                    before_peer: Some(&before),
                    after_peer: Some(&after),
                    before_topology: Some(&topology),
                    planned_after_topology: Some(&planned),
                    route: ProxyPeerRoute::DurableDisjoint,
                    intents: &intents,
                    program_before: &[],
                },
                initial_evidence: &evidence,
                companions: ReadGuardCompanions::default(),
            })
            .is_err()
        );
    }
}

#[test]
fn bind_initial_rejects_semantically_mismatched_observations() {
    let (_, before, after, topology, planned, intents, roots) = fixture();
    let requirements = requirements_for(&before, &after, &topology, &planned, &intents);
    let mut mismatch = evidence_for(&requirements, "initial");
    mismatch[0] = PeerReadGuardEvidence::new(
        requirements[0].path().clone(),
        Some(PeerFileImage::new("wrong", hash('0'), 3).expect("mismatch image")),
    );
    assert!(
        test_bind_initial(InitialReadGuardInput {
            canonical_game_root: ROOT,
            transition: ReadGuardTransition {
                sealed_roots: &roots,
                before_peer: Some(&before),
                after_peer: Some(&after),
                before_topology: Some(&topology),
                planned_after_topology: Some(&planned),
                route: ProxyPeerRoute::DurableDisjoint,
                intents: &intents,
                program_before: &[],
            },
            initial_evidence: &mismatch,
            companions: ReadGuardCompanions::default(),
        })
        .is_err()
    );
}

#[test]
fn final_validation_accepts_digest_only_identity_changes_and_rejects_semantic_mismatch() {
    let (_, before, after, topology, planned, intents, roots) = fixture();
    let seed = test_bind_initial(InitialReadGuardInput {
        canonical_game_root: ROOT,
        transition: ReadGuardTransition {
            sealed_roots: &roots,
            before_peer: Some(&before),
            after_peer: Some(&after),
            before_topology: Some(&topology),
            planned_after_topology: Some(&planned),
            route: ProxyPeerRoute::DurableDisjoint,
            intents: &intents,
            program_before: &[],
        },
        initial_evidence: &[],
        companions: ReadGuardCompanions::default(),
    });
    assert!(seed.is_err());
    // The acceptance and negative path are driven through the real derived
    // projection; the helper below intentionally supplies all four guards.
    let requirements = requirements_for(&before, &after, &topology, &planned, &intents);
    let initial = evidence_for(&requirements, "initial");
    test_bind_initial(InitialReadGuardInput {
        canonical_game_root: ROOT,
        transition: ReadGuardTransition {
            sealed_roots: &roots,
            before_peer: Some(&before),
            after_peer: Some(&after),
            before_topology: Some(&topology),
            planned_after_topology: Some(&planned),
            route: ProxyPeerRoute::DurableDisjoint,
            intents: &intents,
            program_before: &[],
        },
        initial_evidence: &initial,
        companions: ReadGuardCompanions::default(),
    })
    .expect("initial evidence");
    let final_evidence = evidence_for(&requirements, "final");
    test_validate_final(FinalReadGuardInput {
        canonical_game_root: &path(ROOT),
        transition: ReadGuardTransition {
            sealed_roots: &roots,
            before_peer: Some(&before),
            after_peer: Some(&after),
            before_topology: Some(&topology),
            planned_after_topology: Some(&planned),
            route: ProxyPeerRoute::DurableDisjoint,
            intents: &intents,
            program_before: &[],
        },
        stored_requirements: &requirements,
        initial_evidence: &initial,
        final_evidence: &final_evidence,
        companions: ReadGuardCompanions::default(),
    })
    .expect("final evidence");
    let mut mismatch = final_evidence;
    mismatch[0] = PeerReadGuardEvidence::new(
        requirements[0].path().clone(),
        Some(PeerFileImage::new("wrong", hash('0'), 3).expect("mismatch image")),
    );
    assert!(
        test_validate_final(FinalReadGuardInput {
            canonical_game_root: &path(ROOT),
            transition: ReadGuardTransition {
                sealed_roots: &roots,
                before_peer: Some(&before),
                after_peer: Some(&after),
                before_topology: Some(&topology),
                planned_after_topology: Some(&planned),
                route: ProxyPeerRoute::DurableDisjoint,
                intents: &intents,
                program_before: &[],
            },
            stored_requirements: &requirements,
            initial_evidence: &initial,
            final_evidence: &mismatch,
            companions: ReadGuardCompanions::default(),
        })
        .is_err()
    );
}

#[test]
fn invalid_final_guards_leave_prepared_row_peer_and_topology_unchanged() {
    let (game_id, before, after, topology, planned, intents, _) = fixture();
    let requirements = requirements_for(&before, &after, &topology, &planned, &intents);
    let initial_read_guards = evidence_for(&requirements, "initial");
    let manifest = serde_json::json!({
        "format_version": 1,
        "roots": [ROOT],
        "snapshots": [{"path": "C:/game/changed.dll", "snapshot": null}],
        "peer_program": {
            "format": 1,
            "transaction_owner": "mutation-guards",
            "execution_class": "ordinary",
            "roots": [ROOT],
            "stage": [],
            "custody": [],
            "created_ancestors": [],
            "endpoints": [{
                "ordinal": 0,
                "path": "C:/game/changed.dll",
                "role": "disjoint",
                "operation": "create",
                "planned_sha256": "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
                "planned_length": 3,
                "before": null,
                "read_guards": ["C:/game:changed.dll"],
                "subtree_publishes": []
            }]
        }
    });
    let manifest_json = serde_json::to_string(&manifest).expect("manifest");
    let storage = SqliteStorage::in_memory().expect("storage");
    let game = GameInstallation::new(
        GameIdentity::new(game_id.clone(), "Guard Test", Launcher::Steam).expect("identity"),
        Platform::Windows,
        GameRuntime::NativeWindows,
        path(ROOT),
    );
    storage.upsert_game(&game).expect("seed game");
    storage.upsert_installed_addon(&before).expect("seed peer");
    let persisted_before = storage
        .get_installed_addon(&game_id)
        .expect("read seeded peer")
        .expect("seeded peer exists");
    storage
        .with_transaction(|transaction| {
            transaction
                .execute(
                    "INSERT INTO game_proxy_topologies
                     (id, game_id, topology_json, created_at, updated_at)
                     VALUES (?1, ?2, ?3, 1, 1)",
                    rusqlite::params![
                        topology.id,
                        game_id.as_str(),
                        serde_json::to_string(&topology).expect("topology json"),
                    ],
                )
                .map_err(crate::error::storage_error)?;
            Ok(())
        })
        .expect("seed topology");
    storage
        .begin_file_mutation_preparation(&BeginFileMutationPreparation {
            id: "mutation-guards".to_owned(),
            game_id: game_id.clone(),
            feature: "luma_install".to_owned(),
            subject_id: None,
            initial_manifest_json: "{}".to_owned(),
        })
        .expect("reserve mutation");

    let runtime = super::permit::PeerStorageRuntime::new(storage);
    let permit = runtime
        .finish_file_peer_preparation(super::permit::PeerCommitPreparation {
            mutation_id: "mutation-guards",
            game_id: &game_id,
            feature: "luma_install",
            subject_id: None,
            manifest_json: &manifest_json,
            canonical_game_root: ROOT,
            initial_read_guards: &initial_read_guards,
            before_peer: Some(&before),
            after_peer: Some(&after),
            before_topology: Some(&topology),
            planned_after_topology: Some(&planned),
            route: ProxyPeerRoute::DurableDisjoint,
            component_set: None,
            baseline_mutations: &[],
            catalog_claim: None,
            renodx_reshade_ini: None,
        })
        .expect("prepare permit");
    let prepared_before = runtime
        .repositories()
        .get_pending_file_mutation("mutation-guards")
        .expect("prepared row")
        .expect("prepared row exists");
    assert_eq!(
        prepared_before.state,
        crate::PendingFileMutationState::Prepared
    );

    let endpoint_evidence = vec![renderpilot_domain::PeerEndpointEvidence::new(
        intents[0].clone(),
        None,
        Some(PeerFileImage::new("changed", hash('f'), 3).expect("endpoint image")),
    )];
    let mut invalid_final = evidence_for(&requirements, "final");
    invalid_final[0] = PeerReadGuardEvidence::new(
        requirements[0].path().clone(),
        Some(PeerFileImage::new("wrong", hash('0'), 3).expect("invalid guard image")),
    );
    assert!(
        runtime
            .seal_and_commit_ordinary_peer(permit, endpoint_evidence, invalid_final)
            .is_err()
    );

    let prepared_after = runtime
        .repositories()
        .get_pending_file_mutation("mutation-guards")
        .expect("prepared row after rejection")
        .expect("prepared row remains");
    assert_eq!(prepared_after, prepared_before);
    assert_eq!(
        runtime
            .repositories()
            .get_installed_addon(&game_id)
            .expect("peer after rejection"),
        Some(persisted_before)
    );
    assert_eq!(
        runtime
            .repositories()
            .get_proxy_topology(&game_id)
            .expect("topology after rejection"),
        Some(topology)
    );
}

#[test]
fn shared_preparation_remains_guard_free_and_uses_shared_program_scope() {
    let game_id = GameId::new("test:shared-guards").expect("game id");
    let game = GameInstallation::new(
        GameIdentity::new(game_id.clone(), "Shared Guard Test", Launcher::Steam).expect("identity"),
        Platform::Windows,
        GameRuntime::NativeWindows,
        path(ROOT),
    );
    let after_peer = InstalledAddon::new(
        game_id.clone(),
        AddonKind::Luma,
        path("C:/game/shared.addon64"),
    );
    let shared_intent = PeerEndpointIntent::create(
        path("C:/game/shared.addon64"),
        renderpilot_domain::PeerEndpointRole::Disjoint,
        Some(hash('f')),
        Some(3),
    )
    .expect("shared intent");
    let manifest = serde_json::json!({
        "peer_program": {
            "format": 1,
            "transaction_owner": "shared-guards",
            "execution_class": "shared",
            "roots": ["C:/game", "C:/shared"],
            "stage": [],
            "custody": [],
            "created_ancestors": [],
            "endpoints": [{
                "ordinal": 0,
                "path": "C:/game/shared.addon64",
                "role": "disjoint",
                "operation": "create",
                "planned_sha256": "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
                "planned_length": 3,
                "before": null,
                "read_guards": ["C:/game:shared.addon64"],
                "subtree_publishes": []
            }]
        }
    });
    let manifest_json = serde_json::to_string(&manifest).expect("manifest");
    let storage = SqliteStorage::in_memory().expect("storage");
    storage.upsert_game(&game).expect("seed game");
    storage
        .try_begin_shared_vulkan_mutation(&BeginSharedVulkanMutation {
            id: "shared-guards".to_owned(),
            scope: SharedVulkanMutationScope::GameShared,
            game_id: Some(game_id.clone()),
            feature: "luma_install".to_owned(),
            initial_manifest_json: "{}".to_owned(),
            root_capabilities_json: serde_json::json!({
                "version": 1,
                "roots": [
                    {"id": "game-0", "kind": "game", "canonical_path": "C:/game"},
                    {"id": "shared", "kind": "shared_vulkan", "canonical_path": "C:/shared"}
                ]
            })
            .to_string(),
        })
        .expect("reserve shared mutation");
    let runtime = super::permit::PeerStorageRuntime::new(storage);
    let permit = runtime
        .finish_shared_peer_preparation(super::permit::SharedPeerCommitPreparation {
            mutation_id: "shared-guards",
            feature: "luma_install",
            game_id: &game_id,
            manifest_json: &manifest_json,
            before_peer: None,
            after_peer: Some(&after_peer),
            before_topology: None,
            planned_after_topology: None,
            route: ProxyPeerRoute::DurableDisjoint,
            renodx_reshade_ini: None,
        })
        .expect("shared permit");
    assert_eq!(
        runtime
            .repositories()
            .get_pending_shared_vulkan_mutation("shared-guards")
            .expect("shared row")
            .expect("shared row exists")
            .state,
        crate::PendingSharedVulkanMutationState::Prepared
    );
    runtime
        .seal_and_commit_shared_peer(
            permit,
            vec![renderpilot_domain::PeerEndpointEvidence::new(
                shared_intent,
                None,
                Some(PeerFileImage::new("shared", hash('f'), 3).expect("shared image")),
            )],
            InstalledAddonMutation::Upsert(&after_peer),
            SharedArtifactMutation::Keep,
        )
        .expect("shared commit");
    assert_eq!(
        runtime
            .repositories()
            .get_pending_shared_vulkan_mutation("shared-guards")
            .expect("shared row after commit")
            .expect("shared row after commit exists")
            .state,
        crate::PendingSharedVulkanMutationState::Committed
    );
    assert_eq!(
        runtime
            .repositories()
            .get_installed_addon(&game_id)
            .expect("shared peer after commit")
            .expect("shared peer exists")
            .addon_file(),
        after_peer.addon_file()
    );
}

#[test]
fn bind_initial_rejects_missing_exact_evidence_for_a_guard_in_sealed_second_root() {
    let (game_id, _, _, topology, planned, intents, roots) = fixture();
    let before = peer(&game_id, "C:/payload/owned.dll");
    let after = before
        .clone()
        .with_created_file(path("C:/game/changed.dll"));
    let requirements = requirements_for(&before, &after, &topology, &planned, &intents);
    assert!(
        requirements
            .iter()
            .any(|requirement| requirement.path().as_str() == "C:/payload/owned.dll.bak")
    );
    let mut incomplete_evidence = evidence_for(&requirements, "cardinality");
    incomplete_evidence.pop();
    assert!(
        test_bind_initial(InitialReadGuardInput {
            canonical_game_root: ROOT,
            transition: ReadGuardTransition {
                sealed_roots: &roots,
                before_peer: Some(&before),
                after_peer: Some(&after),
                before_topology: Some(&topology),
                planned_after_topology: Some(&planned),
                route: ProxyPeerRoute::DurableDisjoint,
                intents: &intents,
                program_before: &[],
            },
            initial_evidence: &incomplete_evidence,
            companions: ReadGuardCompanions::default(),
        })
        .is_err()
    );
}
