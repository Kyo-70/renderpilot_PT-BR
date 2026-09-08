use super::*;

pub(in crate::repositories::game_mutations) fn test_game(game_id: GameId) -> GameInstallation {
    let install_path =
        PathRef::new(format!("C:/Games/{}", game_id.as_str().replace(':', "_"))).expect("path");
    let identity = GameIdentity::new(game_id, "Test Game", Launcher::Steam).expect("identity");
    GameInstallation::new(
        identity,
        Platform::Windows,
        GameRuntime::NativeWindows,
        install_path,
    )
}

pub(in crate::repositories::game_mutations) fn complete_game_scan(
    storage: &SqliteStorage,
    game: &GameInstallation,
) {
    storage
        .save_complete_scan_write_unit(CompleteScanWriteUnit {
            game,
            components: &[],
            artifacts: &[],
            observations: &[],
            authority: AuthorityCas::new(0),
            prune_empty_operations: false,
        })
        .expect("complete scan");
}

pub(in crate::repositories::game_mutations) fn exact_owned(
    digest: Sha256Hash,
    label: &str,
) -> FileReceipt {
    FileReceipt::owned(format!("test:{label}"), digest).expect("owned receipt")
}

pub(in crate::repositories::game_mutations) fn applied_optiscaler_journal(
    root: &str,
    path: &str,
    before: &FileReceipt,
    after: &FileReceipt,
) -> String {
    let capability = NamespaceCapability::new("a".repeat(64)).expect("capability");
    let endpoint = OperationEndpoint::new(
        Endpoint::Single,
        path,
        Preimage::Initial {
            observation: DurableObservation::File {
                identity: before.identity().to_owned(),
                digest: before.digest().clone(),
            },
            receipt: Some(before.clone()),
            owned_basis: (before.ownership() == FileOwnership::Owned).then_some(before.clone()),
        },
        ExpectedAfter::Known(DurableObservation::File {
            identity: after.identity().to_owned(),
            digest: after.digest().clone(),
        }),
    )
    .expect("endpoint");
    let operation = OperationRecord::new(
        0,
        Vec::new(),
        Some(0),
        PrivateArtifactSlots::new(
            DurableObservation::Absent,
            DurableObservation::Absent,
            DurableObservation::Absent,
        )
        .expect("slots"),
        OperationEffect::Write(
            WriteEffect::new(
                endpoint,
                WriteState::Applied {
                    live: DurableObservation::File {
                        identity: after.identity().to_owned(),
                        digest: after.digest().clone(),
                    },
                    custody: DurableObservation::Absent,
                },
            )
            .expect("write effect"),
        ),
    )
    .expect("operation");
    let mut journal = renderpilot_domain::OptiScalerJournal::new(
        vec![root.to_owned()],
        ControlNamespaceBinding::new(
            format!("{root}/control-abc-{}", capability.as_str()),
            None,
            capability.clone(),
        )
        .expect("control namespace"),
        vec![
            PrivateWorkspaceBinding::new(
                0,
                0,
                format!(
                    "{root}/.renderpilot-optiscaler-workspace-abc-0-{}",
                    capability.as_str()
                ),
                None,
                capability,
            )
            .expect("workspace"),
        ],
        vec![operation],
    )
    .expect("journal");
    journal
        .control_namespace_mut()
        .set_identity("control")
        .expect("control identity");
    journal.private_workspaces_mut()[0]
        .set_identity("participant")
        .expect("workspace identity");
    journal.set_materialization(MaterializationState::Ready);
    serde_json::to_string(&journal).expect("journal json")
}

pub(in crate::repositories::game_mutations) fn prepare_mutation(
    storage: &SqliteStorage,
    game_id: &GameId,
    id: &str,
) {
    storage
        .prepare_file_mutation(&PendingFileMutationRow {
            id: id.to_owned(),
            game_id: game_id.clone(),
            feature: renderpilot_domain::mutation_features::LUMA_UPDATE.to_owned(),
            subject_id: None,
            state: PendingFileMutationState::Preparing,
            manifest_json: r#"{"snapshots":[]}"#.to_owned(),
        })
        .expect("reserve mutation");
    storage
        .finish_preparing_file_mutation(id, r#"{"snapshots":[]}"#)
        .expect("prepare mutation");
}

pub(in crate::repositories::game_mutations) fn prepare_pre_catalog_mutation(
    storage: &SqliteStorage,
    game_id: &GameId,
    id: &str,
) {
    storage
        .begin_file_mutation_preparation(&BeginFileMutationPreparation {
            id: id.to_owned(),
            game_id: game_id.clone(),
            feature: renderpilot_domain::mutation_features::LUMA_UPDATE.to_owned(),
            subject_id: None,
            initial_manifest_json: r#"{"snapshots":[]}"#.to_owned(),
        })
        .expect("reserve pre-catalog mutation");
    storage
        .finish_preparing_file_mutation(id, r#"{"snapshots":[]}"#)
        .expect("prepare pre-catalog mutation");
}

pub(in crate::repositories::game_mutations) fn transition_state() -> OptiScalerInstallState {
    renderpilot_domain::from_persisted(
        renderpilot_domain::OptiScalerInstallStateParts {
            game_id: GameId::new("manual:transition-paths").expect("game id"),
            release_id: "release".to_owned(),
            manifest_revision: "manifest".to_owned(),
            archive_sha256: None,
            source: None,
            target_exe_path: PathRef::new("C:/Games/Test/Game.exe").expect("exe"),
            target_dir: PathRef::new("C:/Games/Test").expect("target"),
            modules: vec!["core".to_owned()],
            release_files: vec![OptiScalerFileReceipt {
                path: PathRef::new("C:/Games/Test/OptiScaler.ini").expect("config"),
                installed: exact_owned(Sha256Hash::new("a".repeat(64)).expect("hash"), "config"),
                role: OptiScalerFileRole::Configuration,
                cleanup: OptiScalerFileCleanup::RemoveIfUnchanged,
                baseline: renderpilot_domain::OptiScalerReleaseFileBaseline::Absent,
            }],
            runtime_bindings: vec![OptiScalerModuleRuntimeBinding {
                module: "core".to_owned(),
                path: PathRef::new("C:/Games/Test/core.dll").expect("runtime"),
                installed: exact_owned(Sha256Hash::new("b".repeat(64)).expect("hash"), "runtime"),
                baseline: OptiScalerFileBaseline::Absent,
            }],
            directory_receipts: vec![OptiScalerDirectoryReceipt {
                path: PathRef::new("C:/Games/Test/shaders").expect("directory"),
                identity: "directory-1".to_owned(),
            }],
            proxy_topology_id: Some("optiscaler:manual:transition-paths".to_owned()),
            config_schema: 1,
            config_base_release: "release".to_owned(),
            adoption_state: OptiScalerAdoptionState::Managed,
            prerequisite_binding: renderpilot_domain::OptiScalerPrerequisiteBinding::None,
            created_at: None,
            updated_at: None,
        },
        renderpilot_domain::OptiScalerConfigurationBaseline::absent(),
    )
    .expect("transition state")
}

pub(in crate::repositories::game_mutations) fn transition_topology() -> GameProxyTopology {
    let root = PathRef::new("C:/Games/Test/dxgi.dll").expect("root");
    GameProxyTopology {
        id: "optiscaler:manual:transition-paths".to_owned(),
        game_id: GameId::new("manual:transition-paths").expect("game id"),
        root_slot: root.clone(),
        outer: ProxyLink {
            implementation: ProxyImplementation::OptiScaler,
            path: root,
            receipt: exact_owned(Sha256Hash::new("c".repeat(64)).expect("hash"), "outer"),
        },
        downstream: None,
        downstream_origin: None,
        root_prestate: ProxyRootPrestate::Absent,
    }
}

pub(in crate::repositories::game_mutations) fn topology_with_peer(
    origin: &str,
    downstream: &str,
    root_prestate: ProxyRootPrestate,
    digest: char,
) -> GameProxyTopology {
    let mut topology = transition_topology();
    topology.downstream_origin = Some(PathRef::new(origin).expect("origin"));
    topology.downstream = Some(ProxyLink {
        implementation: ProxyImplementation::ReShade,
        path: PathRef::new(downstream).expect("downstream"),
        receipt: exact_owned(
            Sha256Hash::new(digest.to_string().repeat(64)).expect("digest"),
            "downstream",
        ),
    });
    topology.root_prestate = root_prestate;
    topology
}

pub(in crate::repositories::game_mutations) fn transition_path_keys(
    paths: Vec<String>,
) -> std::collections::BTreeSet<String> {
    paths
        .into_iter()
        .map(|path| normalized_path_key(&path))
        .collect()
}

pub(in crate::repositories::game_mutations) fn adoption_fixture(
    label: &str,
) -> (
    GameId,
    GameInstallation,
    OptiScalerInstallState,
    GameProxyTopology,
    std::path::PathBuf,
) {
    let root = std::env::temp_dir().join(format!(
        "renderpilot-optiscaler-adoption-{label}-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&root).expect("adoption root");
    let ini = root.join("OptiScaler.ini");
    let proxy = root.join("dxgi.dll");
    std::fs::write(&ini, b"[OptiScaler]\nEnabled=1\n").expect("ini");
    std::fs::write(&proxy, b"OptiScaler proxy").expect("proxy");
    let hash = |path: &std::path::Path| renderpilot_detection::sha256_file(path).expect("hash");
    let game_id = GameId::new(format!("manual:adoption-{label}")).expect("game id");
    let root_ref = PathRef::new(root.to_string_lossy().into_owned()).expect("root ref");
    let game = GameInstallation::new(
        GameIdentity::new(game_id.clone(), "Adoption Game", Launcher::Steam).expect("identity"),
        Platform::Windows,
        GameRuntime::NativeWindows,
        root_ref.clone(),
    );
    let topology_id = format!("optiscaler:{}", game_id.as_str());
    let topology = GameProxyTopology {
        id: topology_id.clone(),
        game_id: game_id.clone(),
        root_slot: PathRef::new(proxy.to_string_lossy().into_owned()).expect("proxy ref"),
        outer: ProxyLink {
            implementation: ProxyImplementation::OptiScaler,
            path: PathRef::new(proxy.to_string_lossy().into_owned()).expect("proxy ref"),
            receipt: FileReceipt::reused("test:adopted-proxy", hash(&proxy))
                .expect("adopted proxy receipt"),
        },
        downstream: None,
        downstream_origin: None,
        root_prestate: ProxyRootPrestate::Absent,
    };
    let configuration =
        FileReceipt::reused("test:adopted-ini", hash(&ini)).expect("adopted configuration receipt");
    let configuration_bytes = std::fs::read(&ini).expect("adopted configuration bytes");
    let state = renderpilot_domain::from_new_adoption(
        renderpilot_domain::OptiScalerInstallStateParts {
            game_id: game_id.clone(),
            release_id: "adopted-release".to_owned(),
            manifest_revision: "manifest".to_owned(),
            archive_sha256: None,
            source: None,
            target_exe_path: PathRef::new(root.join("Game.exe").to_string_lossy().into_owned())
                .expect("exe"),
            target_dir: root_ref,
            modules: vec!["core".to_owned()],
            release_files: vec![OptiScalerFileReceipt {
                path: PathRef::new(ini.to_string_lossy().into_owned()).expect("ini ref"),
                installed: configuration.clone(),
                role: OptiScalerFileRole::Configuration,
                cleanup: OptiScalerFileCleanup::PreserveUnchanged,
                baseline: renderpilot_domain::OptiScalerReleaseFileBaseline::Absent,
            }],
            runtime_bindings: Vec::new(),
            directory_receipts: Vec::new(),
            proxy_topology_id: Some(topology_id),
            config_schema: 1,
            config_base_release: "adopted-release".to_owned(),
            adoption_state: OptiScalerAdoptionState::AdoptedExact,
            prerequisite_binding: renderpilot_domain::OptiScalerPrerequisiteBinding::None,
            created_at: None,
            updated_at: None,
        },
        renderpilot_domain::OptiScalerConfigurationBaseline::present(
            configuration,
            configuration_bytes,
        )
        .expect("adopted configuration baseline"),
    )
    .expect("adoption state");
    (game_id, game, state, topology, root)
}
