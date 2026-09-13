use std::path::Path;

use renderpilot_application::{
    GameRepository, InstalledAddonRepository, OptiScalerStateRepository, ProxyTopologyRepository,
};
use renderpilot_domain::{
    FileReceipt, GameId, GameIdentity, GameInstallation, GameRuntime, InstalledAddonHostKind,
    Launcher, ManagedAddonFile, ManagedFileBaseline, OptiScalerAdoptionState,
    OptiScalerConfigurationBaseline, OptiScalerFileCleanup, OptiScalerFileReceipt,
    OptiScalerFileRole, OptiScalerInstallStateParts, PathRef, PlannedGameProxyTopology, Platform,
    ProxyLink, ProxyRootPrestate, Sha256Hash,
};
use sha2::Digest as _;

use super::*;

fn path(value: &Path) -> PathRef {
    PathRef::new(value.to_string_lossy().replace('\\', "/")).expect("path")
}

fn digest(bytes: &[u8]) -> renderpilot_domain::Sha256Hash {
    renderpilot_domain::Sha256Hash::new(hex::encode(sha2::Sha256::digest(bytes))).expect("digest")
}

fn receipt(path: &Path, bytes: &[u8]) -> FileReceipt {
    let (parent, leaf) = crate::fs::verified_parent(path).expect("verified parent");
    let observed = parent
        .observe_leaf(&leaf)
        .expect("observe file")
        .expect("file present");
    FileReceipt::reused(observed.identity, digest(bytes)).expect("receipt")
}

fn owned_receipt(path: &Path, bytes: &[u8]) -> FileReceipt {
    let observed = receipt(path, bytes);
    FileReceipt::owned(observed.identity(), observed.digest().clone()).expect("owned receipt")
}

fn seed_managed_optiscaler_aggregate(
    database: &Path,
    state: &renderpilot_domain::OptiScalerInstallState,
    topology: &GameProxyTopology,
) {
    let connection = rusqlite::Connection::open(database).expect("fixture connection");
    connection
        .execute(
            "INSERT INTO game_proxy_topologies (
                game_id, id, topology_json, created_at, updated_at
            ) VALUES (?1, ?2, ?3, 1, 1)",
            rusqlite::params![
                topology.game_id.as_str(),
                &topology.id,
                serde_json::to_string(topology).expect("topology json"),
            ],
        )
        .expect("seed topology");
    connection
        .execute(
            "INSERT INTO optiscaler_install_states (
                game_id, release_id, manifest_revision, archive_sha256, source,
                target_exe_path, target_dir, modules_json, release_files_json,
                runtime_bindings_json, directory_receipts_json, proxy_topology_id,
                config_schema, config_base_release, adoption_state, prerequisite_binding,
                created_at, updated_at, configuration_baseline_json
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5,
                ?6, ?7, ?8, ?9,
                ?10, ?11, ?12,
                ?13, ?14, ?15, ?16,
                1, 1, ?17
            )",
            rusqlite::params![
                state.game_id.as_str(),
                &state.release_id,
                &state.manifest_revision,
                state.archive_sha256.as_ref().map(Sha256Hash::as_str),
                state.source.as_deref(),
                state.target_exe_path.as_str(),
                state.target_dir.as_str(),
                serde_json::to_string(&state.modules).expect("modules json"),
                serde_json::to_string(&state.release_files).expect("files json"),
                serde_json::to_string(&state.runtime_bindings).expect("bindings json"),
                serde_json::to_string(&state.directory_receipts).expect("directories json"),
                state.proxy_topology_id.as_deref(),
                state.config_schema,
                &state.config_base_release,
                state.adoption_state.as_str(),
                state.prerequisite_binding.as_str(),
                "{\"format_tag\":\"renderpilot.optiscaler.configuration-baseline\",\"revision\":1,\"kind\":\"absent\"}",
            ],
        )
        .expect("seed OptiScaler state");
}

struct Fixture {
    _database: tempfile::TempDir,
    _root: tempfile::TempDir,
    context: Context,
    game_id: GameId,
    root: std::path::PathBuf,
    topology: GameProxyTopology,
    safety: crate::GameMutationSafetyPermits,
}

fn fixture() -> Fixture {
    let database = tempfile::tempdir().expect("database");
    let root = tempfile::tempdir().expect("game root");
    let root_path = crate::paths::canonicalize_existing(root.path()).expect("canonical root");
    let context = Context::open_at(database.path().join("catalog.sqlite")).expect("context");
    let game_id = GameId::new(format!(
        "manual:renodx-opti-config-command:{}",
        ulid::Ulid::generate()
    ))
    .expect("game id");
    let executable = root_path.join("Game.exe");
    let proxy = root_path.join("dxgi.dll");
    let config = root_path.join("OptiScaler.ini");
    std::fs::write(&executable, b"exe").expect("exe");
    std::fs::write(&proxy, b"opti-proxy").expect("proxy");
    std::fs::write(&config, b"[Plugins]\nLoadReshade=false\n").expect("config");
    context
        .storage()
        .upsert_game(&GameInstallation::new(
            GameIdentity::new(game_id.clone(), "RenoDX Opti companion", Launcher::Manual)
                .expect("identity"),
            Platform::Windows,
            GameRuntime::NativeWindows,
            path(&root_path),
        ))
        .expect("game");
    let proxy_ref = path(&proxy);
    let topology = GameProxyTopology {
        id: "optiscaler:renodx-command".to_owned(),
        game_id: game_id.clone(),
        root_slot: proxy_ref.clone(),
        outer: ProxyLink {
            implementation: ProxyImplementation::OptiScaler,
            path: proxy_ref,
            receipt: receipt(&proxy, b"opti-proxy"),
        },
        downstream: None,
        downstream_origin: None,
        root_prestate: ProxyRootPrestate::Absent,
    };
    let state = renderpilot_domain::from_new_install(
        OptiScalerInstallStateParts {
            game_id: game_id.clone(),
            release_id: "test-release".to_owned(),
            manifest_revision: "test-revision".to_owned(),
            archive_sha256: None,
            source: None,
            target_exe_path: path(&executable),
            target_dir: path(&root_path),
            modules: vec!["core".to_owned()],
            release_files: vec![OptiScalerFileReceipt {
                path: path(&config),
                installed: owned_receipt(&config, b"[Plugins]\nLoadReshade=false\n"),
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
    .expect("OptiScaler state");
    seed_managed_optiscaler_aggregate(&database.path().join("catalog.sqlite"), &state, &topology);
    let authority = crate::FileSafetyAuthority::new();
    let assessment = authority
        .issue_game_assessment(&context, &game_id)
        .expect("assessment");
    let safety = authority
        .game_mutation_permits(game_id.clone(), Some(&assessment.context_token), None)
        .expect("safety");
    Fixture {
        _database: database,
        _root: root,
        context,
        game_id,
        root: root_path,
        topology,
        safety,
    }
}

fn install_input(
    fixture: &Fixture,
) -> (
    InstalledAddon,
    crate::peer_mutation_executor::ExactEndpointProgram,
    Vec<Option<Vec<u8>>>,
    PlannedGameProxyTopology,
    std::path::PathBuf,
) {
    let addon = fixture.root.join("renodx.addon64");
    let host = fixture.root.join("ReShade64.dll");
    let addon_bytes = b"renodx-addon".to_vec();
    let host_bytes = b"renodx-host".to_vec();
    let addon_ref = path(&addon);
    let host_ref = path(&host);
    let record = InstalledAddon::new(
        fixture.game_id.clone(),
        AddonKind::RenoDx,
        addon_ref.clone(),
    )
    .with_host_kind(InstalledAddonHostKind::Proxy)
    .with_reshade_channel("stable")
    .try_with_managed_files(vec![ManagedAddonFile::owned(
        host_ref.clone(),
        ManagedFileBaseline::Absent,
        digest(&host_bytes),
    )])
    .expect("record");
    let program = crate::peer_mutation_executor::ExactEndpointProgram::new(vec![
        crate::peer_mutation_executor::ExactEndpoint::new(
            addon_ref,
            renderpilot_domain::PeerEndpointRole::Disjoint,
            crate::peer_mutation_executor::EndpointExpectation::Absent,
            crate::peer_mutation_executor::EndpointPostcondition::File(digest(&addon_bytes)),
        ),
        crate::peer_mutation_executor::ExactEndpoint::new(
            host_ref.clone(),
            renderpilot_domain::PeerEndpointRole::TopologyDownstream,
            crate::peer_mutation_executor::EndpointExpectation::Absent,
            crate::peer_mutation_executor::EndpointPostcondition::File(digest(&host_bytes)),
        ),
    ])
    .expect("program");
    let planned = PlannedGameProxyTopology::ObservedOwnedDownstream {
        id: fixture.topology.id.clone(),
        game_id: fixture.game_id.clone(),
        root_slot: fixture.topology.root_slot.clone(),
        outer: fixture.topology.outer.clone(),
        implementation: ProxyImplementation::ReShade,
        downstream_path: host_ref,
        downstream_origin: fixture.topology.root_slot.clone(),
        root_prestate: fixture.topology.root_prestate,
        planned_sha256: digest(&host_bytes),
        planned_length: host_bytes.len() as u64,
    };
    (
        record,
        program,
        vec![Some(addon_bytes), Some(host_bytes)],
        planned,
        addon,
    )
}

#[test]
fn active_proxy_command_commit_and_final_uninstall_toggle_optiscaler_receipt_atomically() {
    let fixture = fixture();
    let (record, program, payloads, planned, addon) = install_input(&fixture);
    let guard = crate::game_mutation_lock::try_lock(&fixture.game_id).expect("guard");
    commit_game(ActiveGameCommit {
        context: &fixture.context,
        game_id: &fixture.game_id,
        feature: renderpilot_domain::mutation_features::RENODX_INSTALL,
        safety: &fixture.safety,
        guard,
        record,
        program,
        payloads,
        before_topology: fixture.topology.clone(),
        planned_topology: planned,
        game_root: fixture.root.clone(),
        payload_root: None,
        reshade_ini_authority: None,
        addon_path: addon,
        source_last_modified: None,
        source_mtime: None,
    })
    .expect("active proxy install");

    let config = std::fs::read(fixture.root.join("OptiScaler.ini")).expect("enabled config");
    assert!(
        config
            .windows(b"LoadReshade=true".len())
            .any(|line| line == b"LoadReshade=true")
    );
    let enabled = fixture
        .context
        .storage()
        .get_optiscaler_install_state(&fixture.game_id)
        .expect("state")
        .expect("enabled state");
    assert_eq!(
        enabled
            .configuration_receipt()
            .expect("receipt")
            .installed
            .digest(),
        &digest(&config)
    );
    assert!(
        fixture
            .context
            .storage()
            .get_proxy_topology(&fixture.game_id)
            .expect("topology")
            .expect("topology")
            .downstream
            .is_some()
    );

    crate::addons::renodx::use_cases::commands::uninstall::uninstall(
        &fixture.context,
        &fixture.game_id,
    )
    .expect("final active proxy uninstall");
    let config = std::fs::read(fixture.root.join("OptiScaler.ini")).expect("disabled config");
    assert!(
        config
            .windows(b"LoadReshade=false".len())
            .any(|line| line == b"LoadReshade=false")
    );
    let disabled = fixture
        .context
        .storage()
        .get_optiscaler_install_state(&fixture.game_id)
        .expect("state")
        .expect("disabled state");
    assert_eq!(
        disabled
            .configuration_receipt()
            .expect("receipt")
            .installed
            .digest(),
        &digest(&config)
    );
    assert!(
        fixture
            .context
            .storage()
            .get_proxy_topology(&fixture.game_id)
            .expect("topology")
            .expect("topology")
            .downstream
            .is_none()
    );
    assert!(
        fixture
            .context
            .storage()
            .get_installed_addon(&fixture.game_id)
            .expect("record")
            .is_none()
    );
}

#[test]
fn active_proxy_command_failure_preserves_optiscaler_bytes_and_receipt() {
    let fixture = fixture();
    let before_bytes = std::fs::read(fixture.root.join("OptiScaler.ini")).expect("config");
    let before_state = fixture
        .context
        .storage()
        .get_optiscaler_install_state(&fixture.game_id)
        .expect("state")
        .expect("state exists");
    std::fs::write(fixture.root.join("ReShade64.dll"), b"foreign host").expect("conflicting host");
    let (record, program, payloads, planned, addon) = install_input(&fixture);
    let guard = crate::game_mutation_lock::try_lock(&fixture.game_id).expect("guard");
    assert!(
        commit_game(ActiveGameCommit {
            context: &fixture.context,
            game_id: &fixture.game_id,
            feature: renderpilot_domain::mutation_features::RENODX_INSTALL,
            safety: &fixture.safety,
            guard,
            record,
            program,
            payloads,
            before_topology: fixture.topology.clone(),
            planned_topology: planned,
            game_root: fixture.root.clone(),
            payload_root: None,
            reshade_ini_authority: None,
            addon_path: addon,
            source_last_modified: None,
            source_mtime: None,
        })
        .is_err()
    );
    assert_eq!(
        std::fs::read(fixture.root.join("OptiScaler.ini")).expect("config"),
        before_bytes
    );
    assert_eq!(
        fixture
            .context
            .storage()
            .get_optiscaler_install_state(&fixture.game_id)
            .expect("state"),
        Some(before_state)
    );
    assert!(
        fixture
            .context
            .storage()
            .get_installed_addon(&fixture.game_id)
            .expect("record")
            .is_none()
    );
}
