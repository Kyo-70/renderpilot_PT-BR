use super::*;
use renderpilot_domain::{
    FileReceipt, GameId, PathRef, PeerReadGuardSource, ProxyImplementation, ProxyLink,
    ProxyRootPrestate, Sha256Hash,
};
use sha2::Digest as _;

#[path = "catalog_tests.rs"]
mod catalog_tests;

fn hash(bytes: &[u8]) -> Sha256Hash {
    Sha256Hash::new(hex::encode(sha2::Sha256::digest(bytes))).expect("hash")
}

fn path(path: &std::path::Path) -> PathRef {
    PathRef::new(path.to_string_lossy().replace('\\', "/")).expect("path")
}

fn outer_topology(root: &std::path::Path) -> GameProxyTopology {
    let root_slot = path(&root.join("dxgi.dll"));
    GameProxyTopology {
        id: "optiscaler:package-test".to_owned(),
        game_id: GameId::new("manual:package-test").expect("game"),
        root_slot: root_slot.clone(),
        outer: ProxyLink {
            implementation: renderpilot_domain::ProxyImplementation::OptiScaler,
            path: root_slot,
            receipt: FileReceipt::owned("outer", hash(b"outer")).expect("receipt"),
        },
        downstream: None,
        downstream_origin: None,
        root_prestate: ProxyRootPrestate::Absent,
    }
}

fn file_program(target: &PathRef, digest: Sha256Hash) -> ExactEndpointProgram {
    ExactEndpointProgram::new(vec![crate::peer_mutation_executor::ExactEndpoint::new(
        target.clone(),
        renderpilot_domain::PeerEndpointRole::Disjoint,
        EndpointExpectation::Absent,
        EndpointPostcondition::File(digest),
    )])
    .expect("program")
}

#[test]
fn payload_validation_rejects_missing_extra_and_wrong_digest() {
    let root = tempfile::tempdir().expect("root");
    let target = path(&root.path().join("payload.addon"));
    let program = file_program(&target, hash(b"payload"));
    assert!(PeerMutationPlan::new(program.clone(), Vec::new()).is_err());
    assert!(PeerMutationPlan::new(program.clone(), vec![None]).is_err());
    assert!(PeerMutationPlan::new(program.clone(), vec![Some(b"wrong".to_vec())]).is_err());
    assert!(PeerMutationPlan::new(program, vec![Some(b"payload".to_vec()), None]).is_err());
}

#[test]
fn metadata_only_guard_rejects_physical_and_topology_changes() {
    let root = tempfile::tempdir().expect("root");
    let game = GameId::new("manual:metadata-test").expect("game");
    let addon_path = path(&root.path().join("luma.addon"));
    let before = InstalledAddon::new(game, AddonKind::Luma, addon_path);
    validate_metadata_only(Some(&before), Some(&before), None, None).expect("metadata");

    let physical = before
        .clone()
        .with_created_file(path(&root.path().join("new.dll")));
    assert!(validate_metadata_only(Some(&before), Some(&physical), None, None).is_err());
    let topology = outer_topology(root.path());
    assert!(validate_metadata_only(Some(&before), Some(&before), None, Some(&topology)).is_err());
}

#[test]
fn active_package_rejects_occupied_create_and_payload_root_topology() {
    let game = tempfile::tempdir().expect("game");
    let payload = tempfile::tempdir().expect("payload");
    let topology = outer_topology(game.path());
    let occupied = game.path().join("host.dll");
    std::fs::write(&occupied, b"foreign").expect("occupied");
    let planned = PlannedGameProxyTopology::ObservedOwnedDownstream {
        id: topology.id.clone(),
        game_id: topology.game_id.clone(),
        root_slot: topology.root_slot.clone(),
        outer: topology.outer.clone(),
        implementation: ProxyImplementation::ReShade,
        downstream_path: path(&occupied),
        downstream_origin: topology.root_slot.clone(),
        root_prestate: topology.root_prestate,
        planned_sha256: hash(b"host"),
        planned_length: 4,
    };
    let program =
        ExactEndpointProgram::new(vec![crate::peer_mutation_executor::ExactEndpoint::new(
            path(&occupied),
            renderpilot_domain::PeerEndpointRole::TopologyDownstream,
            EndpointExpectation::Absent,
            EndpointPostcondition::File(hash(b"host")),
        )])
        .expect("program");
    let occupied_request = PeerMutationRequest {
        peer_kind: AddonKind::Luma,
        before_peer: None,
        after_peer: None,
        before_topology: &topology,
        planned_after_topology: &planned,
        program: program.clone(),
        payloads: vec![Some(b"host".to_vec())],
        game_root: game.path().to_path_buf(),
        payload_root: None,
        component_set: None,
        baseline_mutations: &[],
        catalog_claim: None,
    };
    assert!(PeerMutationPackage::plan_active(occupied_request).is_err());

    let outside = payload.path().join("host.dll");
    let outside_planned = PlannedGameProxyTopology::ObservedOwnedDownstream {
        id: topology.id.clone(),
        game_id: topology.game_id.clone(),
        root_slot: topology.root_slot.clone(),
        outer: topology.outer.clone(),
        implementation: ProxyImplementation::ReShade,
        downstream_path: path(&outside),
        downstream_origin: topology.root_slot.clone(),
        root_prestate: topology.root_prestate,
        planned_sha256: hash(b"host"),
        planned_length: 4,
    };
    let outside_request = PeerMutationRequest {
        peer_kind: AddonKind::RenoDx,
        before_peer: None,
        after_peer: None,
        before_topology: &topology,
        planned_after_topology: &outside_planned,
        program,
        payloads: vec![Some(b"host".to_vec())],
        game_root: game.path().to_path_buf(),
        payload_root: Some(payload.path().to_path_buf()),
        component_set: None,
        baseline_mutations: &[],
        catalog_claim: None,
    };
    assert!(PeerMutationPackage::plan_active(outside_request).is_err());

    let external = tempfile::tempdir().expect("external");
    let external_addon = path(&external.path().join("luma.addon"));
    let external_peer = InstalledAddon::new(
        topology.game_id.clone(),
        AddonKind::Luma,
        external_addon.clone(),
    );
    let external_program = file_program(&external_addon, hash(b"addon"));
    let external_request = PeerMutationRequest {
        peer_kind: AddonKind::Luma,
        before_peer: None,
        after_peer: Some(&external_peer),
        before_topology: &topology,
        planned_after_topology: &PlannedGameProxyTopology::Exact(topology.clone()),
        program: external_program,
        payloads: vec![Some(b"addon".to_vec())],
        game_root: game.path().to_path_buf(),
        payload_root: Some(payload.path().to_path_buf()),
        component_set: None,
        baseline_mutations: &[],
        catalog_claim: None,
    };
    assert!(PeerMutationPackage::plan_active(external_request).is_err());
}

#[test]
fn active_package_accepts_managed_guards_under_explicit_payload_root() {
    let game = tempfile::tempdir().expect("game");
    let payload = tempfile::tempdir().expect("payload");
    let topology = outer_topology(game.path());
    let managed_path = path(&payload.path().join("managed.dll"));
    let reused_path = path(&payload.path().join("reused.dll"));
    let endpoint = path(&game.path().join("luma.addon"));
    let target = path(&game.path().join("new.dll"));
    let before_peer = InstalledAddon::new(topology.game_id.clone(), AddonKind::Luma, endpoint)
        .try_with_managed_files(vec![
            renderpilot_domain::ManagedAddonFile::owned(
                managed_path.clone(),
                renderpilot_domain::ManagedFileBaseline::Absent,
                hash(b"managed"),
            ),
            renderpilot_domain::ManagedAddonFile::reused(reused_path.clone(), hash(b"reused")),
        ])
        .expect("managed peer");
    let planned = PlannedGameProxyTopology::Exact(topology.clone());
    let after_peer = before_peer.clone().with_created_file(target.clone());
    let program = file_program(&target, hash(b"addon"));

    let result = PeerMutationPackage::plan_active(PeerMutationRequest {
        peer_kind: AddonKind::Luma,
        before_peer: Some(&before_peer),
        after_peer: Some(&after_peer),
        before_topology: &topology,
        planned_after_topology: &planned,
        program,
        payloads: vec![Some(b"addon".to_vec())],
        game_root: game.path().to_path_buf(),
        payload_root: Some(payload.path().to_path_buf()),
        component_set: None,
        baseline_mutations: &[],
        catalog_claim: None,
    });

    let package = result.expect("payload root seals managed read guards");
    assert!(package.read_guards().iter().any(|guard| {
        guard.path() == &managed_path
            || guard.path()
                == &renderpilot_domain::managed_sidecar_path(&managed_path).expect("sidecar")
    }));
    assert!(
        package
            .read_guards()
            .iter()
            .any(|guard| guard.path() == &reused_path)
    );

    let without_payload_root = PeerMutationPackage::plan_active(PeerMutationRequest {
        peer_kind: AddonKind::Luma,
        before_peer: Some(&before_peer),
        after_peer: Some(&after_peer),
        before_topology: &topology,
        planned_after_topology: &planned,
        program: file_program(&target, hash(b"addon")),
        payloads: vec![Some(b"addon".to_vec())],
        game_root: game.path().to_path_buf(),
        payload_root: None,
        component_set: None,
        baseline_mutations: &[],
        catalog_claim: None,
    });
    let error = without_payload_root
        .expect_err("the same non-topology guards must not escape the game root");
    assert!(error.to_string().contains("sealed"));
}

#[test]
fn active_package_freezes_outer_downstream_owned_baseline_and_reused_live_guards() {
    let game = tempfile::tempdir().expect("game");
    let root_slot = path(&game.path().join("dxgi.dll"));
    let downstream_path = path(&game.path().join("d3d.dll"));
    let topology = GameProxyTopology {
        id: "optiscaler:guard-package-test".to_owned(),
        game_id: GameId::new("manual:guard-package-test").expect("game id"),
        root_slot: root_slot.clone(),
        outer: ProxyLink {
            implementation: ProxyImplementation::OptiScaler,
            path: root_slot,
            receipt: FileReceipt::owned("outer", hash(b"outer")).expect("outer receipt"),
        },
        downstream: Some(ProxyLink {
            implementation: ProxyImplementation::ReShade,
            path: downstream_path.clone(),
            receipt: FileReceipt::owned("downstream", hash(b"downstream"))
                .expect("downstream receipt"),
        }),
        downstream_origin: Some(path(&game.path().join("dxgi.dll"))),
        root_prestate: ProxyRootPrestate::Absent,
    };
    let addon_path = path(&game.path().join("luma.addon"));
    let owned_path = path(&game.path().join("owned.dll"));
    let reused_path = path(&game.path().join("reused.dll"));
    let before_peer = InstalledAddon::new(topology.game_id.clone(), AddonKind::Luma, addon_path)
        .try_with_managed_files(vec![
            renderpilot_domain::ManagedAddonFile::owned(
                owned_path.clone(),
                renderpilot_domain::ManagedFileBaseline::Absent,
                hash(b"owned"),
            ),
            renderpilot_domain::ManagedAddonFile::reused(reused_path.clone(), hash(b"reused")),
        ])
        .expect("managed peer");
    let target = path(&game.path().join("new.dll"));
    let program = file_program(&target, hash(b"new"));
    let after_peer = before_peer.clone().with_created_file(target.clone());
    let planned = PlannedGameProxyTopology::Exact(topology.clone());
    let package = PeerMutationPackage::plan_active(PeerMutationRequest {
        peer_kind: AddonKind::Luma,
        before_peer: Some(&before_peer),
        after_peer: Some(&after_peer),
        before_topology: &topology,
        planned_after_topology: &planned,
        program,
        payloads: vec![Some(b"new".to_vec())],
        game_root: game.path().to_path_buf(),
        payload_root: None,
        component_set: None,
        baseline_mutations: &[],
        catalog_claim: None,
    })
    .expect("package");

    let guards = package.read_guards();
    assert_eq!(guards.len(), 4);
    assert_eq!(guards[0].path(), &downstream_path);
    assert_eq!(
        guards[0].sources(),
        &[PeerReadGuardSource::TopologyDownstream]
    );
    assert_eq!(guards[1].path(), &topology.root_slot);
    assert_eq!(guards[1].sources(), &[PeerReadGuardSource::TopologyOuter]);
    assert_eq!(
        guards
            .iter()
            .find(|guard| guard.sources() == [PeerReadGuardSource::ManagedOwnedBaseline])
            .expect("owned baseline guard")
            .path()
            .as_str(),
        renderpilot_domain::managed_sidecar_path(&owned_path)
            .expect("sidecar")
            .as_str()
    );
    assert_eq!(
        guards
            .iter()
            .find(|guard| guard.sources() == [PeerReadGuardSource::ManagedReusedLive])
            .expect("reused live guard")
            .path(),
        &reused_path
    );
    assert!(!guards.iter().any(|guard| guard.path() == &target));
}

#[test]
fn valid_package_uses_the_coordinated_create_route() {
    let game = tempfile::tempdir().expect("game");
    let topology = outer_topology(game.path());
    let host = path(&game.path().join("ReShade64.dll"));
    let addon_file = path(&game.path().join("luma.addon"));
    let host_bytes = b"host".to_vec();
    let addon_bytes = b"addon".to_vec();
    let after_peer = InstalledAddon::new(
        topology.game_id.clone(),
        AddonKind::Luma,
        addon_file.clone(),
    )
    .try_with_managed_files(vec![renderpilot_domain::ManagedAddonFile::owned(
        host.clone(),
        renderpilot_domain::ManagedFileBaseline::Absent,
        hash(&host_bytes),
    )])
    .expect("managed peer");
    let planned = PlannedGameProxyTopology::ObservedOwnedDownstream {
        id: topology.id.clone(),
        game_id: topology.game_id.clone(),
        root_slot: topology.root_slot.clone(),
        outer: topology.outer.clone(),
        implementation: ProxyImplementation::ReShade,
        downstream_path: host.clone(),
        downstream_origin: topology.root_slot.clone(),
        root_prestate: topology.root_prestate,
        planned_sha256: hash(&host_bytes),
        planned_length: host_bytes.len() as u64,
    };
    let program = ExactEndpointProgram::new(vec![
        crate::peer_mutation_executor::ExactEndpoint::new(
            host,
            renderpilot_domain::PeerEndpointRole::TopologyDownstream,
            EndpointExpectation::Absent,
            EndpointPostcondition::File(hash(&host_bytes)),
        ),
        crate::peer_mutation_executor::ExactEndpoint::new(
            addon_file,
            renderpilot_domain::PeerEndpointRole::Disjoint,
            EndpointExpectation::Absent,
            EndpointPostcondition::File(hash(&addon_bytes)),
        ),
    ])
    .expect("program");
    let package = PeerMutationPackage::plan_active(PeerMutationRequest {
        peer_kind: AddonKind::Luma,
        before_peer: None,
        after_peer: Some(&after_peer),
        before_topology: &topology,
        planned_after_topology: &planned,
        program,
        payloads: vec![Some(host_bytes), Some(addon_bytes)],
        game_root: game.path().to_path_buf(),
        payload_root: None,
        component_set: None,
        baseline_mutations: &[],
        catalog_claim: None,
    })
    .expect("package");
    assert_eq!(
        package.route(),
        ProxyPeerRoute::Coordinated(renderpilot_domain::CoordinatedPeerOperation::Create)
    );
}

#[test]
fn active_package_rejects_generic_create_with_a_present_preimage() {
    let game = tempfile::tempdir().expect("game");
    let topology = outer_topology(game.path());
    let target = path(&game.path().join("peer.addon"));
    let bytes = b"already present";
    std::fs::write(target.as_str(), bytes).expect("present preimage");
    let after_peer = InstalledAddon::new(topology.game_id.clone(), AddonKind::Luma, target.clone());
    let planned = PlannedGameProxyTopology::Exact(topology.clone());

    let error = PeerMutationPackage::plan_active(PeerMutationRequest {
        peer_kind: AddonKind::Luma,
        before_peer: None,
        after_peer: Some(&after_peer),
        before_topology: &topology,
        planned_after_topology: &planned,
        program: file_program(&target, hash(bytes)),
        payloads: vec![Some(bytes.to_vec())],
        game_root: game.path().to_path_buf(),
        payload_root: None,
        component_set: None,
        baseline_mutations: &[],
        catalog_claim: None,
    })
    .expect_err("a generic create must not adopt a present preimage");

    assert!(matches!(
        error,
        ServiceError::PeerTopologyConflict {
            peer_kind: AddonKind::Luma
        }
    ));
}

#[test]
fn active_package_rejects_byte_identical_generic_replace() {
    let game = tempfile::tempdir().expect("game");
    let topology = outer_topology(game.path());
    let target = path(&game.path().join("peer.addon"));
    let bytes = b"stable payload";
    std::fs::write(target.as_str(), bytes).expect("stable preimage");
    let peer = InstalledAddon::new(topology.game_id.clone(), AddonKind::Luma, target.clone());
    let snapshot =
        crate::peer_mutation_executor::observe_peer_path_snapshot(&target, &path(game.path()))
            .expect("snapshot");
    let before = EndpointExpectation::File(snapshot.file().expect("file").clone());
    let planned = PlannedGameProxyTopology::Exact(topology.clone());
    let program =
        ExactEndpointProgram::new(vec![crate::peer_mutation_executor::ExactEndpoint::new(
            target,
            renderpilot_domain::PeerEndpointRole::Disjoint,
            before,
            EndpointPostcondition::File(hash(bytes)),
        )])
        .expect("program");

    let error = PeerMutationPackage::plan_active(PeerMutationRequest {
        peer_kind: AddonKind::Luma,
        before_peer: Some(&peer),
        after_peer: Some(&peer),
        before_topology: &topology,
        planned_after_topology: &planned,
        program,
        payloads: vec![Some(bytes.to_vec())],
        game_root: game.path().to_path_buf(),
        payload_root: None,
        component_set: None,
        baseline_mutations: &[],
        catalog_claim: None,
    })
    .expect_err("a byte-identical generic replace must be rejected");

    assert!(error.to_string().contains("no-op"));
}
