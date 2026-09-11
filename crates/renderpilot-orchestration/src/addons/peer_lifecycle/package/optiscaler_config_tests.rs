use super::*;
use renderpilot_domain::{
    ExactOptiConfigProjection, FileReceipt, OptiConfigOperation, OptiScalerAdoptionState,
    OptiScalerConfigAuthority, OptiScalerConfigurationBaseline, OptiScalerFileCleanup,
    OptiScalerFileReceipt, OptiScalerFileRole, OptiScalerInstallStateParts, PathRef,
    ProxyImplementation, ProxyLink, ProxyRootPrestate, Sha256Hash,
};
use sha2::Digest as _;

fn hash(bytes: &[u8]) -> Sha256Hash {
    Sha256Hash::new(hex::encode(sha2::Sha256::digest(bytes))).expect("hash")
}

fn path(value: &std::path::Path) -> PathRef {
    PathRef::new(value.to_string_lossy().replace('\\', "/")).expect("path")
}

fn outer_topology(root: &std::path::Path) -> GameProxyTopology {
    let root_slot = path(&root.join("dxgi.dll"));
    GameProxyTopology {
        id: "optiscaler:config-scope".to_owned(),
        game_id: renderpilot_domain::GameId::new("manual:config-scope").expect("game"),
        root_slot: root_slot.clone(),
        outer: ProxyLink {
            implementation: ProxyImplementation::OptiScaler,
            path: root_slot,
            receipt: FileReceipt::owned("outer-id", hash(b"outer")).expect("receipt"),
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

fn companion(
    root: &std::path::Path,
    topology: &GameProxyTopology,
) -> RenoDxOptiScalerConfigCompanion {
    let root = path(root);
    let config = PathRef::new(format!("{}/OptiScaler.ini", root.as_str())).expect("config");
    let state = renderpilot_domain::from_persisted(
        OptiScalerInstallStateParts {
            game_id: topology.game_id.clone(),
            release_id: "test-release".to_owned(),
            manifest_revision: "test-revision".to_owned(),
            archive_sha256: None,
            source: None,
            target_exe_path: PathRef::new(format!("{}/Game.exe", root.as_str())).expect("exe"),
            target_dir: root.clone(),
            modules: vec!["core".to_owned()],
            release_files: vec![OptiScalerFileReceipt {
                path: config,
                installed: FileReceipt::owned("config-id", hash(b"before")).expect("receipt"),
                role: OptiScalerFileRole::Configuration,
                cleanup: OptiScalerFileCleanup::RemoveIfUnchanged,
                baseline: renderpilot_domain::OptiScalerReleaseFileBaseline::Absent,
            }],
            runtime_bindings: Vec::new(),
            directory_receipts: Vec::new(),
            proxy_topology_id: Some(topology.id.clone()),
            config_schema: 1,
            config_base_release: "test-release".to_owned(),
            adoption_state: OptiScalerAdoptionState::Managed,
            prerequisite_binding: renderpilot_domain::OptiScalerPrerequisiteBinding::None,
            created_at: None,
            updated_at: None,
        },
        OptiScalerConfigurationBaseline::absent(),
    )
    .expect("state");
    let mut receipt = state.configuration_receipt().expect("receipt").clone();
    receipt.installed = FileReceipt::owned("config-id", hash(b"after")).expect("post receipt");
    let projection = ExactOptiConfigProjection::new(
        OptiScalerConfigAuthority::new(root).expect("authority"),
        receipt,
        OptiConfigOperation::EnableLoadReshade,
    )
    .expect("projection");
    RenoDxOptiScalerConfigCompanion::new(state, projection).expect("companion")
}

fn request<'a>(
    game: &'a std::path::Path,
    topology: &'a GameProxyTopology,
    planned: &'a PlannedGameProxyTopology,
    peer_kind: AddonKind,
) -> PeerMutationRequest<'a> {
    let addon = path(&game.join("peer.addon"));
    PeerMutationRequest {
        peer_kind,
        before_peer: None,
        after_peer: None,
        before_topology: topology,
        planned_after_topology: planned,
        program: file_program(&addon, hash(b"addon")),
        payloads: vec![Some(b"addon".to_vec())],
        game_root: game.to_path_buf(),
        payload_root: None,
        component_set: None,
        baseline_mutations: &[],
        catalog_claim: None,
    }
}

#[test]
fn optiscaler_config_package_is_reserved_for_renodx_proxy_routes() {
    let game = tempfile::tempdir().expect("game");
    let topology = outer_topology(game.path());
    let planned = PlannedGameProxyTopology::Exact(topology.clone());
    let error = PeerMutationPackage::plan_active_with_renodx_optiscaler_config(
        request(game.path(), &topology, &planned, AddonKind::Luma),
        None,
        companion(game.path(), &topology),
    )
    .expect_err("only RenoDX may coordinate OptiScaler configuration");
    assert!(error.to_string().contains("reserved for RenoDX"));

    let mut wrong_outer = topology;
    wrong_outer.outer.implementation = renderpilot_domain::ProxyImplementation::ReShade;
    let wrong_planned = PlannedGameProxyTopology::Exact(wrong_outer.clone());
    let error = PeerMutationPackage::plan_active_with_renodx_optiscaler_config(
        request(game.path(), &wrong_outer, &wrong_planned, AddonKind::RenoDx),
        None,
        companion(game.path(), &wrong_outer),
    )
    .expect_err("only an OptiScaler outer route may coordinate its configuration");
    assert!(error.to_string().contains("OptiScaler outer proxy"));
}
