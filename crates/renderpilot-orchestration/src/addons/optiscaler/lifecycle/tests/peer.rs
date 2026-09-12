use super::*;

#[test]
fn uninstall_relinquishes_an_exact_runtime_claimed_by_the_remaining_peer() {
    let db_root = tempdir().expect("database root");
    let game_root = tempdir().expect("game root");
    let context = Context::open_at(db_root.path().join("catalog.sqlite")).expect("context");
    let game_id = GameId::new("manual:optiscaler-shared-claim").expect("game id");
    let game = GameInstallation::new(
        GameIdentity::new(game_id.clone(), "OptiScaler shared claim", Launcher::Manual)
            .expect("identity"),
        Platform::Windows,
        GameRuntime::NativeWindows,
        PathRef::new(game_root.path().to_string_lossy()).expect("game path"),
    );
    context.storage().upsert_game(&game).expect("game");

    let exe = game_root.path().join("Game.exe");
    let proxy = game_root.path().join("dxgi.dll");
    let config = game_root.path().join("OptiScaler.ini");
    let runtime = game_root.path().join("shared-runtime.dll");
    std::fs::write(&exe, b"exe").expect("exe");
    std::fs::write(&proxy, b"outer").expect("proxy");
    std::fs::write(&config, b"[OptiScaler]\n").expect("config");
    std::fs::write(&runtime, b"shared runtime").expect("runtime");
    let runtime_sha256 = renderpilot_detection::sha256_file(&runtime).expect("runtime hash");

    let peer = InstalledAddon::new(
        game_id.clone(),
        AddonKind::RenoDx,
        PathRef::new(game_root.path().join("peer.addon64").to_string_lossy()).expect("peer addon"),
    )
    .try_with_managed_files(vec![ManagedAddonFile::reused(
        PathRef::new(runtime.to_string_lossy()).expect("runtime path"),
        runtime_sha256,
    )])
    .expect("peer receipt");
    context
        .storage()
        .upsert_installed_addon(&peer)
        .expect("peer state");
    let persisted_peer = context
        .storage()
        .get_installed_addon(&game_id)
        .expect("persisted peer")
        .expect("peer exists");

    let topology_id = format!("optiscaler:{}", game_id.as_str());
    let topology = GameProxyTopology {
        id: topology_id.clone(),
        game_id: game_id.clone(),
        root_slot: path_ref(&proxy).expect("proxy path"),
        outer: ProxyLink {
            implementation: ProxyImplementation::OptiScaler,
            path: path_ref(&proxy).expect("proxy path"),
            receipt: exact_receipt_from_live(&proxy, FileOwnership::Reused)
                .expect("exact adopted proxy receipt"),
        },
        downstream: None,
        downstream_origin: None,
        root_prestate: ProxyRootPrestate::Absent,
    };
    let config_baseline_receipt =
        exact_receipt_from_live(&config, FileOwnership::Reused).expect("config baseline receipt");
    let runtime_receipt =
        exact_receipt_from_live(&runtime, FileOwnership::Reused).expect("runtime receipt");
    let state = renderpilot_domain::from_new_adoption(
        renderpilot_domain::OptiScalerInstallStateParts {
            game_id: game_id.clone(),
            release_id: "shared".to_owned(),
            manifest_revision: "test".to_owned(),
            archive_sha256: None,
            source: None,
            target_exe_path: path_ref(&exe).expect("exe path"),
            target_dir: path_ref(game_root.path()).expect("target path"),
            modules: vec!["core".to_owned()],
            release_files: vec![OptiScalerFileReceipt {
                path: path_ref(&config).expect("config path"),
                installed: exact_receipt_from_live(&config, FileOwnership::Reused)
                    .expect("config receipt"),
                role: OptiScalerFileRole::Configuration,
                cleanup: OptiScalerFileCleanup::PreserveUnchanged,
                baseline: renderpilot_domain::OptiScalerReleaseFileBaseline::Absent,
            }],
            runtime_bindings: vec![renderpilot_domain::OptiScalerModuleRuntimeBinding {
                module: "core".to_owned(),
                path: path_ref(&runtime).expect("runtime path"),
                installed: runtime_receipt.clone(),
                baseline: OptiScalerFileBaseline::Present {
                    receipt: runtime_receipt,
                },
            }],
            directory_receipts: Vec::new(),
            proxy_topology_id: Some(topology_id),
            config_schema: 1,
            config_base_release: "shared".to_owned(),
            adoption_state: OptiScalerAdoptionState::AdoptedExact,
            prerequisite_binding: renderpilot_domain::OptiScalerPrerequisiteBinding::None,
            created_at: None,
            updated_at: None,
        },
        renderpilot_domain::OptiScalerConfigurationBaseline::present(
            config_baseline_receipt,
            b"[OptiScaler]\n".to_vec(),
        )
        .expect("configuration baseline"),
    )
    .expect("state");
    context
        .storage()
        .commit_game_mutation(renderpilot_storage_sqlite::GameMutationCommit {
            game_id: &game_id,
            component_set: None,
            baseline_mutations: &[],
            addon: renderpilot_storage_sqlite::InstalledAddonMutation::OptiScaler(
                renderpilot_storage_sqlite::OptiScalerAggregateMutation::AdoptExactMetadata {
                    state: &state,
                    topology: &topology,
                },
            ),
            mutation_id: None,
        })
        .expect("adopt aggregate");

    let guard = crate::game_mutation_lock::try_lock(&game_id).expect("guard");
    uninstall_locked(&context, &game_id, &guard).expect("uninstall");

    assert_eq!(
        std::fs::read(&runtime).expect("retained shared runtime"),
        b"shared runtime"
    );
    assert!(
        !proxy.exists(),
        "an exact-adopted Reused outer must be removed during uninstall"
    );
    assert!(
        context
            .storage()
            .get_optiscaler_install_state(&game_id)
            .unwrap()
            .is_none()
    );
    assert!(
        context
            .storage()
            .get_proxy_topology(&game_id)
            .unwrap()
            .is_none()
    );
    assert_eq!(
        context
            .storage()
            .get_installed_addon(&game_id)
            .expect("peer after uninstall")
            .as_ref(),
        Some(&persisted_peer)
    );
    assert!(
        context
            .storage()
            .pending_file_mutations_for_game(&game_id)
            .expect("pending rows")
            .is_empty()
    );
}
