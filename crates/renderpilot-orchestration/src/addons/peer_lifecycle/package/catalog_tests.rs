use super::*;
use renderpilot_domain::{
    ComponentFile, ComponentId, ComponentKind, ComponentRollbackBaseline, LibraryTechnology,
    PathRef, PeerCatalogDeletedBaseline, PeerCatalogPhysicalContract, PeerCatalogRollbackClaim,
    Swappability, required_read_guards_with_catalog,
};

fn component(game_id: &GameId, id: &str, path: &PathRef, digest: Sha256Hash) -> LibraryComponent {
    LibraryComponent::new(
        ComponentId::new(id).expect("component id"),
        game_id.clone(),
        ComponentKind::NativeLibrary,
        LibraryTechnology::NvidiaStreamline,
        Swappability::Swappable,
    )
    .with_file(ComponentFile::new(path.clone()).with_sha256(digest))
}

fn empty_rollback_claim(game_id: &GameId) -> PeerCatalogRollbackClaim {
    let component = LibraryComponent::new(
        ComponentId::new("component:claim-consistency").expect("component id"),
        game_id.clone(),
        ComponentKind::NativeLibrary,
        LibraryTechnology::NvidiaStreamline,
        Swappability::Swappable,
    );
    PeerCatalogRollbackClaim::new(
        vec![component.clone()],
        vec![PeerCatalogDeletedBaseline::new(
            component.id().clone(),
            ComponentRollbackBaseline::new(Vec::new()),
        )],
    )
    .expect("empty rollback claim")
}

fn consistency_request<'a>(
    topology: &'a GameProxyTopology,
    planned: &'a PlannedGameProxyTopology,
    target: &'a PathRef,
    claim: &'a PeerCatalogRollbackClaim,
    component_set: Option<&'a [LibraryComponent]>,
    baseline_mutations: &'a [ComponentBaselineMutation<'a>],
) -> PeerMutationRequest<'a> {
    PeerMutationRequest {
        peer_kind: AddonKind::Luma,
        before_peer: None,
        after_peer: None,
        before_topology: topology,
        planned_after_topology: planned,
        program: file_program(target, hash(b"payload")),
        payloads: vec![Some(b"payload".to_vec())],
        game_root: std::path::PathBuf::from(
            std::path::Path::new(topology.root_slot.as_str())
                .parent()
                .expect("game root"),
        ),
        payload_root: None,
        component_set,
        baseline_mutations,
        catalog_claim: Some(claim),
    }
}

#[test]
fn active_package_binds_exact_catalog_rollback_to_o1_and_domain_derivation() {
    let game = tempfile::tempdir().expect("game");
    let topology = outer_topology(game.path());
    let live_path = path(&game.path().join("nvngx_dlss.dll"));
    let sidecar_path = renderpilot_domain::managed_sidecar_path(&live_path).expect("sidecar");
    let live_bytes = b"active library";
    let baseline_bytes = b"baseline library";
    std::fs::write(std::path::Path::new(live_path.as_str()), live_bytes).expect("live file");
    std::fs::write(sidecar_path.as_str(), baseline_bytes).expect("baseline sidecar");

    let live_digest = hash(live_bytes);
    let baseline_digest = hash(baseline_bytes);
    let before_component = component(
        &topology.game_id,
        "component:streamline",
        &live_path,
        live_digest,
    );
    let component_id = before_component.id().clone();
    let claim = PeerCatalogRollbackClaim::new(
        vec![before_component],
        vec![PeerCatalogDeletedBaseline::new(
            component_id.clone(),
            ComponentRollbackBaseline::new(vec![
                ComponentFile::new(live_path.clone()).with_sha256(baseline_digest.clone()),
            ]),
        )],
    )
    .expect("claim");

    let live_snapshot =
        crate::peer_mutation_executor::observe_peer_path_snapshot(&live_path, &path(game.path()))
            .expect("live snapshot");
    let sidecar_snapshot = crate::peer_mutation_executor::observe_peer_path_snapshot(
        &sidecar_path,
        &path(game.path()),
    )
    .expect("sidecar snapshot");
    let live_before = match &live_snapshot {
        crate::peer_mutation_executor::PeerPathSnapshot::File(_) => {
            EndpointExpectation::File(live_snapshot.file().expect("live metadata").clone())
        }
        crate::peer_mutation_executor::PeerPathSnapshot::Absent => panic!("live file absent"),
    };
    let sidecar_before = match &sidecar_snapshot {
        crate::peer_mutation_executor::PeerPathSnapshot::File(_) => {
            EndpointExpectation::File(sidecar_snapshot.file().expect("sidecar metadata").clone())
        }
        crate::peer_mutation_executor::PeerPathSnapshot::Absent => panic!("sidecar absent"),
    };
    let program = ExactEndpointProgram::new(vec![
        crate::peer_mutation_executor::ExactEndpoint::new(
            live_path,
            renderpilot_domain::PeerEndpointRole::Disjoint,
            live_before,
            EndpointPostcondition::File(baseline_digest),
        ),
        crate::peer_mutation_executor::ExactEndpoint::new(
            sidecar_path,
            renderpilot_domain::PeerEndpointRole::Disjoint,
            sidecar_before,
            EndpointPostcondition::Absent,
        ),
    ])
    .expect("program");
    let planned = PlannedGameProxyTopology::Exact(topology.clone());
    let deletes = [ComponentBaselineMutation::Delete {
        component_id: &component_id,
    }];
    let package = PeerMutationPackage::plan_active(PeerMutationRequest {
        peer_kind: AddonKind::Luma,
        before_peer: None,
        after_peer: None,
        before_topology: &topology,
        planned_after_topology: &planned,
        program,
        payloads: vec![Some(baseline_bytes.to_vec()), None],
        game_root: game.path().to_path_buf(),
        payload_root: None,
        component_set: Some(claim.after_components()),
        baseline_mutations: &deletes,
        catalog_claim: Some(&claim),
    })
    .expect("catalog package");

    let physical_program = domain_program(package.plan().program(), package.plan().payloads())
        .expect("physical program");
    let preimages =
        crate::peer_mutation_executor::domain_preimages(package.plan(), package.preflight())
            .expect("preimages");
    let catalog =
        PeerCatalogPhysicalContract::derive(&claim, None, None, &physical_program, &preimages)
            .expect("catalog physical contract");
    let expected_guards = required_read_guards_with_catalog(
        None,
        None,
        Some(&topology),
        Some(&planned),
        package.route(),
        &physical_program,
        Some(&catalog),
    )
    .expect("read guards");
    let expected_contract = PeerTransitionContract::derive_physical_with_catalog(
        None,
        None,
        Some(&topology),
        Some(&planned),
        package.route(),
        physical_program,
        Some(&catalog),
    )
    .expect("transition contract");

    assert_eq!(package.catalog_claim(), Some(&claim));
    assert_eq!(package.read_guards(), expected_guards);
    assert_eq!(package.contract(), &expected_contract);
}

#[test]
fn active_package_rejects_inconsistent_catalog_projection_or_baseline_mutations() {
    let game = tempfile::tempdir().expect("game");
    let topology = outer_topology(game.path());
    let planned = PlannedGameProxyTopology::Exact(topology.clone());
    let target = path(&game.path().join("peer.addon"));
    let claim = empty_rollback_claim(&topology.game_id);
    let component_id = claim.deleted_baselines()[0].component_id().clone();
    let valid = [ComponentBaselineMutation::Delete {
        component_id: &component_id,
    }];

    assert!(
        PeerMutationPackage::plan_active(consistency_request(
            &topology, &planned, &target, &claim, None, &valid,
        ))
        .is_err()
    );

    let wrong_component = component(
        &topology.game_id,
        "component:wrong",
        &target,
        hash(b"wrong"),
    );
    let wrong_set = [wrong_component];
    assert!(
        PeerMutationPackage::plan_active(consistency_request(
            &topology,
            &planned,
            &target,
            &claim,
            Some(&wrong_set),
            &valid,
        ))
        .is_err()
    );

    assert!(
        PeerMutationPackage::plan_active(consistency_request(
            &topology,
            &planned,
            &target,
            &claim,
            Some(claim.after_components()),
            &[],
        ))
        .is_err()
    );

    let other_id = ComponentId::new("component:other").expect("component id");
    let extra = [ComponentBaselineMutation::Delete {
        component_id: &other_id,
    }];
    assert!(
        PeerMutationPackage::plan_active(consistency_request(
            &topology,
            &planned,
            &target,
            &claim,
            Some(claim.after_components()),
            &extra,
        ))
        .is_err()
    );

    let duplicate = [
        ComponentBaselineMutation::Delete {
            component_id: &component_id,
        },
        ComponentBaselineMutation::Delete {
            component_id: &component_id,
        },
    ];
    assert!(
        PeerMutationPackage::plan_active(consistency_request(
            &topology,
            &planned,
            &target,
            &claim,
            Some(claim.after_components()),
            &duplicate,
        ))
        .is_err()
    );

    let capture_baseline = ComponentRollbackBaseline::new(Vec::new());
    let capture = [ComponentBaselineMutation::Capture {
        component_id: &component_id,
        baseline: &capture_baseline,
    }];
    assert!(
        PeerMutationPackage::plan_active(consistency_request(
            &topology,
            &planned,
            &target,
            &claim,
            Some(claim.after_components()),
            &capture,
        ))
        .is_err()
    );
}

#[test]
fn active_package_accepts_catalog_guards_under_explicit_payload_root() {
    let game = tempfile::tempdir().expect("game");
    let payload = tempfile::tempdir().expect("payload");
    let topology = outer_topology(game.path());
    let live_path = path(&payload.path().join("nvngx_dlss.dll"));
    let sidecar_path = renderpilot_domain::managed_sidecar_path(&live_path).expect("sidecar");
    let bytes = b"baseline";
    std::fs::write(std::path::Path::new(live_path.as_str()), bytes).expect("live file");
    std::fs::write(std::path::Path::new(sidecar_path.as_str()), bytes).expect("sidecar");
    let digest = hash(bytes);
    let before_component = component(
        &topology.game_id,
        "component:payload-guard",
        &live_path,
        digest.clone(),
    );
    let component_id = before_component.id().clone();
    let claim = PeerCatalogRollbackClaim::new(
        vec![before_component],
        vec![PeerCatalogDeletedBaseline::new(
            component_id.clone(),
            ComponentRollbackBaseline::new(vec![
                ComponentFile::new(live_path.clone()).with_sha256(digest),
            ]),
        )],
    )
    .expect("claim");
    let snapshot = crate::peer_mutation_executor::observe_peer_path_snapshot(
        &sidecar_path,
        &path(payload.path()),
    )
    .expect("sidecar snapshot");
    let before = match &snapshot {
        crate::peer_mutation_executor::PeerPathSnapshot::File(_) => {
            EndpointExpectation::File(snapshot.file().expect("sidecar metadata").clone())
        }
        crate::peer_mutation_executor::PeerPathSnapshot::Absent => panic!("sidecar absent"),
    };
    let program =
        ExactEndpointProgram::new(vec![crate::peer_mutation_executor::ExactEndpoint::new(
            sidecar_path,
            renderpilot_domain::PeerEndpointRole::Disjoint,
            before,
            EndpointPostcondition::Absent,
        )])
        .expect("program");
    let planned = PlannedGameProxyTopology::Exact(topology.clone());
    let deletes = [ComponentBaselineMutation::Delete {
        component_id: &component_id,
    }];

    let package = PeerMutationPackage::plan_active(PeerMutationRequest {
        peer_kind: AddonKind::Luma,
        before_peer: None,
        after_peer: None,
        before_topology: &topology,
        planned_after_topology: &planned,
        program,
        payloads: vec![None],
        game_root: game.path().to_path_buf(),
        payload_root: Some(payload.path().to_path_buf()),
        component_set: Some(claim.after_components()),
        baseline_mutations: &deletes,
        catalog_claim: Some(&claim),
    });

    let package = package.expect("explicit payload root seals catalog guards");
    let guards = package.read_guards();
    assert!(guards.iter().any(|guard| guard.path() == &live_path));
}
