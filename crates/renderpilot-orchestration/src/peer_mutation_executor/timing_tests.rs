use std::fs;
use std::path::{Path, PathBuf};

use renderpilot_application::{
    ComponentRepository, GameRepository, InstalledAddonRepository, ProxyTopologyRepository,
};
use renderpilot_domain::{
    AddonKind, FileReceipt, GameId, GameIdentity, GameInstallation, GameProxyTopology, GameRuntime,
    InstalledAddon, Launcher, ManagedAddonFile, ManagedFileBaseline, OptiScalerAdoptionState,
    OptiScalerConfigurationBaseline, OptiScalerFileCleanup, OptiScalerFileReceipt,
    OptiScalerFileRole, OptiScalerInstallStateParts, PathRef, Platform, ProxyImplementation,
    ProxyLink, ProxyRootPrestate, Sha256Hash,
};
use renderpilot_storage_sqlite::{
    GameMutationCommit, InstalledAddonMutation, OptiScalerAggregateMutation,
    PendingFileMutationState,
};
use sha2::Digest as _;

use super::{EndpointExpectation, EndpointPostcondition, ExactEndpoint, ExactEndpointProgram};
use crate::Context;
use crate::addons::engine::InstallChanges;
use crate::addons::peer_lifecycle::package::{PeerMutationPackage, PeerMutationRequest};
use crate::game_mutation_lock;

pub(super) struct Fixture {
    pub(super) _database: tempfile::TempDir,
    pub(super) _game: tempfile::TempDir,
    pub(super) game_root: PathBuf,
    pub(super) context: Context,
    pub(super) game_id: GameId,
    pub(super) before_peer: InstalledAddon,
    pub(super) topology: GameProxyTopology,
    pub(super) planned: renderpilot_domain::PlannedGameProxyTopology,
    pub(super) endpoint: PathBuf,
    pub(super) managed_sidecar: PathBuf,
}

pub(super) fn hash(bytes: &[u8]) -> Sha256Hash {
    Sha256Hash::new(hex::encode(sha2::Sha256::digest(bytes))).expect("hash")
}

pub(super) fn path_ref(path: &Path) -> PathRef {
    PathRef::new(path.to_string_lossy().replace('\\', "/")).expect("path ref")
}

fn exact_receipt(path: &Path, bytes: &[u8]) -> FileReceipt {
    let (parent, leaf) = crate::fs::verified_parent(path).expect("verified parent");
    let observation = parent
        .observe_leaf(&leaf)
        .expect("observe receipt")
        .expect("receipt file");
    FileReceipt::reused(observation.identity, hash(bytes)).expect("receipt")
}

pub(super) fn seed_game(context: &Context, game_id: &GameId, root: &Path) {
    let game = GameInstallation::new(
        GameIdentity::new(game_id.clone(), "peer timing test", Launcher::Manual).expect("identity"),
        Platform::Windows,
        GameRuntime::NativeWindows,
        path_ref(root),
    );
    context.storage().upsert_game(&game).expect("game");
}

pub(super) fn seed_optiscaler(
    context: &Context,
    game_id: &GameId,
    root: &Path,
) -> GameProxyTopology {
    let proxy = root.join("dxgi.dll");
    let config = root.join("OptiScaler.ini");
    let executable = root.join("Game.exe");
    fs::write(&proxy, b"outer").expect("proxy");
    fs::write(&config, b"[OptiScaler]\nEnabled=true\n").expect("config");
    fs::write(&executable, b"exe").expect("executable");

    let config_receipt = exact_receipt(&config, b"[OptiScaler]\nEnabled=true\n");

    let topology_id = format!("optiscaler:peer-timing:{}", game_id.as_str());
    let topology = GameProxyTopology {
        id: topology_id.clone(),
        game_id: game_id.clone(),
        root_slot: path_ref(&proxy),
        outer: ProxyLink {
            implementation: ProxyImplementation::OptiScaler,
            path: path_ref(&proxy),
            receipt: exact_receipt(&proxy, b"outer"),
        },
        downstream: None,
        downstream_origin: None,
        root_prestate: ProxyRootPrestate::Absent,
    };
    let state = renderpilot_domain::from_persisted(
        OptiScalerInstallStateParts {
            game_id: game_id.clone(),
            release_id: "peer-timing".to_owned(),
            manifest_revision: "peer-timing".to_owned(),
            archive_sha256: None,
            source: None,
            target_exe_path: path_ref(&executable),
            target_dir: path_ref(root),
            modules: vec!["core".to_owned()],
            release_files: vec![OptiScalerFileReceipt {
                path: path_ref(&config),
                installed: config_receipt.clone(),
                role: OptiScalerFileRole::Configuration,
                cleanup: OptiScalerFileCleanup::PreserveUnchanged,
                baseline: renderpilot_domain::OptiScalerReleaseFileBaseline::Absent,
            }],
            runtime_bindings: Vec::new(),
            directory_receipts: Vec::new(),
            proxy_topology_id: Some(topology_id),
            config_schema: 1,
            config_base_release: "peer-timing".to_owned(),
            adoption_state: OptiScalerAdoptionState::AdoptedExact,
            prerequisite_binding: renderpilot_domain::OptiScalerPrerequisiteBinding::None,
            created_at: None,
            updated_at: None,
        },
        OptiScalerConfigurationBaseline::present(
            config_receipt,
            b"[OptiScaler]\nEnabled=true\n".to_vec(),
        )
        .expect("configuration baseline"),
    )
    .expect("state");
    context
        .storage()
        .commit_game_mutation(GameMutationCommit {
            game_id,
            component_set: None,
            baseline_mutations: &[],
            addon: InstalledAddonMutation::OptiScaler(
                OptiScalerAggregateMutation::AdoptExactMetadata {
                    state: &state,
                    topology: &topology,
                },
            ),
            mutation_id: None,
        })
        .expect("OptiScaler aggregate");
    topology
}

pub(super) fn fixture() -> Fixture {
    let database = tempfile::tempdir().expect("database");
    let game = tempfile::tempdir().expect("game");
    let context = Context::open_at(database.path().join("catalog.sqlite")).expect("context");
    let game_id =
        GameId::new(format!("manual:peer-timing:{}", ulid::Ulid::generate())).expect("game id");
    let game_root = fs::canonicalize(game.path()).expect("canonical game root");
    seed_game(&context, &game_id, &game_root);
    let managed_path = game_root.join("managed.dll");
    fs::write(&managed_path, b"managed").expect("managed file");
    let managed_sidecar = PathBuf::from(
        renderpilot_domain::managed_sidecar_path(&path_ref(&managed_path))
            .expect("managed sidecar")
            .as_str(),
    );
    let before_peer = InstalledAddon::new(
        game_id.clone(),
        AddonKind::Luma,
        path_ref(&game.path().join("luma.addon64")),
    )
    .try_with_managed_files(vec![ManagedAddonFile::owned(
        path_ref(&managed_path),
        ManagedFileBaseline::Absent,
        hash(b"managed"),
    )])
    .expect("peer");
    context
        .storage()
        .upsert_installed_addon(&before_peer)
        .expect("peer preimage");
    let topology = seed_optiscaler(&context, &game_id, &game_root);
    let endpoint = game_root.join("nested").join("peer.addon64");
    Fixture {
        _database: database,
        _game: game,
        game_root,
        context,
        game_id,
        before_peer,
        planned: renderpilot_domain::PlannedGameProxyTopology::Exact(topology.clone()),
        topology,
        endpoint,
        managed_sidecar,
    }
}

fn after_peer(fixture: &Fixture) -> InstalledAddon {
    fixture
        .before_peer
        .clone()
        .with_created_file(path_ref(&fixture.endpoint))
}

fn package_for<'a>(
    fixture: &'a Fixture,
    after_peer: &'a InstalledAddon,
) -> PeerMutationPackage<'a> {
    let program = ExactEndpointProgram::new(vec![ExactEndpoint::new(
        path_ref(&fixture.endpoint),
        renderpilot_domain::PeerEndpointRole::Disjoint,
        EndpointExpectation::Absent,
        EndpointPostcondition::File(hash(b"addon")),
    )])
    .expect("program");
    PeerMutationPackage::plan_active(PeerMutationRequest {
        peer_kind: AddonKind::Luma,
        before_peer: Some(&fixture.before_peer),
        after_peer: Some(after_peer),
        before_topology: &fixture.topology,
        planned_after_topology: &fixture.planned,
        program,
        payloads: vec![Some(b"addon".to_vec())],
        game_root: fixture.game_root.clone(),
        payload_root: None,
        component_set: None,
        baseline_mutations: &[],
        catalog_claim: None,
    })
    .expect("package")
}

pub(super) fn assert_prepared(fixture: &Fixture) {
    let rows = fixture
        .context
        .storage()
        .pending_file_mutations_for_game(&fixture.game_id)
        .expect("pending rows");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].state, PendingFileMutationState::Prepared);
}

#[test]
fn ordinary_initial_guard_drift_abandons_before_prepared() {
    let fixture = fixture();
    let after_peer = after_peer(&fixture);
    let package = package_for(&fixture, &after_peer);
    fs::write(
        Path::new(fixture.topology.root_slot.as_str()),
        b"foreign outer",
    )
    .expect("drift outer");
    let guard = game_mutation_lock::try_lock(&fixture.game_id).expect("guard");

    let result = fixture
        .context
        .peer_mutation_executor()
        .prepare_ordinary_file_peer(
            &fixture.context,
            &guard,
            renderpilot_domain::mutation_features::LUMA_INSTALL,
            None,
            package,
        );

    assert!(
        result.is_err(),
        "initial read-guard drift must reject preparation"
    );
    assert!(
        fixture
            .context
            .storage()
            .pending_file_mutations_for_game(&fixture.game_id)
            .expect("pending rows")
            .is_empty()
    );
}

#[test]
fn ordinary_final_guard_drift_keeps_prepared_row_and_projection_preimages() {
    let fixture = fixture();
    let after_peer = after_peer(&fixture);
    let package = package_for(&fixture, &after_peer);
    let before_peer = fixture
        .context
        .storage()
        .get_installed_addon(&fixture.game_id)
        .expect("peer preimage");
    let before_topology = fixture
        .context
        .storage()
        .get_proxy_topology(&fixture.game_id)
        .expect("topology preimage");
    let before_components = fixture
        .context
        .storage()
        .list_components_for_game(&fixture.game_id)
        .expect("catalog preimage");
    let guard = game_mutation_lock::try_lock(&fixture.game_id).expect("guard");
    let prepared = fixture
        .context
        .peer_mutation_executor()
        .prepare_ordinary_file_peer(
            &fixture.context,
            &guard,
            renderpilot_domain::mutation_features::LUMA_INSTALL,
            None,
            package,
        )
        .expect("prepared ordinary peer");
    let mut changes = InstallChanges::default();
    let applied = prepared.apply(&mut changes).expect("applied peer");
    assert!(fixture.endpoint.is_file());
    fs::write(&fixture.managed_sidecar, b"drift").expect("guard drift");

    assert!(
        applied.commit().is_err(),
        "final guard drift must reject CAS"
    );
    assert_prepared(&fixture);
    assert_eq!(
        fixture
            .context
            .storage()
            .get_installed_addon(&fixture.game_id)
            .expect("peer after failed commit"),
        before_peer
    );
    assert_eq!(
        fixture
            .context
            .storage()
            .get_proxy_topology(&fixture.game_id)
            .expect("topology after failed commit"),
        before_topology
    );
    assert_eq!(
        fixture
            .context
            .storage()
            .list_components_for_game(&fixture.game_id)
            .expect("catalog after failed commit"),
        before_components
    );
}
