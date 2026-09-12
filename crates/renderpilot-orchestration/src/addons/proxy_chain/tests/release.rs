use super::*;
use tempfile::tempdir;

#[test]
fn release_restores_the_relocated_downstream_without_creating_a_sidecar() {
    let root = tempdir().expect("root");
    let proxy = root.path().join("dxgi.dll");
    let downstream = root.path().join("ReShade64.dll");
    let config = root.path().join("OptiScaler.ini");
    std::fs::write(&proxy, b"outer").expect("proxy");
    std::fs::write(&downstream, b"peer").expect("downstream");
    std::fs::write(&config, b"[OptiScaler]\nEnabled=true\n").expect("config");
    let root_slot = PathRef::new(proxy.to_string_lossy().into_owned()).expect("root path");
    let downstream_path =
        PathRef::new(downstream.to_string_lossy().into_owned()).expect("downstream path");
    let topology = GameProxyTopology {
        id: "optiscaler:test-release".to_owned(),
        game_id: GameId::new("manual:proxy-release").expect("game id"),
        root_slot: root_slot.clone(),
        outer: ProxyLink {
            implementation: ProxyImplementation::OptiScaler,
            path: root_slot,
            receipt: exact_receipt_from_live(&proxy, FileOwnership::Reused).expect("outer receipt"),
        },
        downstream: Some(ProxyLink {
            implementation: ProxyImplementation::ReShade,
            path: downstream_path,
            receipt: exact_receipt_from_live(&downstream, FileOwnership::Reused)
                .expect("peer receipt"),
        }),
        downstream_origin: Some(
            PathRef::new(proxy.to_string_lossy().into_owned()).expect("origin path"),
        ),
        root_prestate: ProxyRootPrestate::RelocatedDownstream,
    };
    let mut changed = Vec::new();
    let downstream_identity = topology
        .downstream
        .as_ref()
        .expect("downstream")
        .receipt
        .identity()
        .to_owned();
    let context = Context::open_at(root.path().join("catalog.db")).expect("context");
    seed_game(&context, &topology.game_id, root.path());
    let configuration =
        exact_receipt_from_live(&config, FileOwnership::Reused).expect("configuration receipt");
    let state = renderpilot_domain::from_new_adoption(
        renderpilot_domain::OptiScalerInstallStateParts {
            game_id: topology.game_id.clone(),
            release_id: "proxy-chain-test".to_owned(),
            manifest_revision: "proxy-chain-test".to_owned(),
            archive_sha256: None,
            source: None,
            target_exe_path: path_ref(&root.path().join("Game.exe")).expect("exe path"),
            target_dir: path_ref(root.path()).expect("target path"),
            modules: vec!["core".to_owned()],
            release_files: vec![renderpilot_domain::OptiScalerFileReceipt {
                path: path_ref(&config).expect("configuration path"),
                installed: configuration.clone(),
                role: renderpilot_domain::OptiScalerFileRole::Configuration,
                cleanup: renderpilot_domain::OptiScalerFileCleanup::PreserveUnchanged,
                baseline: renderpilot_domain::OptiScalerReleaseFileBaseline::Absent,
            }],
            runtime_bindings: Vec::new(),
            directory_receipts: Vec::new(),
            proxy_topology_id: Some(topology.id.clone()),
            config_schema: 1,
            config_base_release: "proxy-chain-test".to_owned(),
            adoption_state: renderpilot_domain::OptiScalerAdoptionState::AdoptedExact,
            prerequisite_binding: renderpilot_domain::OptiScalerPrerequisiteBinding::None,
            created_at: None,
            updated_at: None,
        },
        renderpilot_domain::OptiScalerConfigurationBaseline::present(
            configuration,
            b"[OptiScaler]\nEnabled=true\n".to_vec(),
        )
        .expect("configuration baseline"),
    )
    .expect("adopted state");
    context
        .storage()
        .commit_game_mutation(renderpilot_storage_sqlite::GameMutationCommit {
            game_id: &topology.game_id,
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
        .expect("seed aggregate");
    let guard = crate::game_mutation_lock::try_lock(&topology.game_id).expect("guard");
    let scope = crate::file_mutation::MutationScope::single(root.path()).expect("scope");
    let uninstall_operations = vec![
        crate::file_mutation::optiscaler::OptiScalerPlannedOperation::Verify(
            crate::file_mutation::optiscaler::PlannedParticipant {
                path: config.clone(),
                preimage: crate::file_mutation::optiscaler::PlannedPreimage::ExactReused {
                    current: state.release_files[0].installed.clone(),
                    authority:
                        crate::file_mutation::optiscaler::ReusedMutationAuthority::ObservationOnly,
                },
            },
        ),
        crate::file_mutation::optiscaler::OptiScalerPlannedOperation::Delete(
            crate::file_mutation::optiscaler::PlannedParticipant {
                path: proxy.clone(),
                preimage: crate::file_mutation::optiscaler::PlannedPreimage::ExactReused {
                    current: topology.outer.receipt.clone(),
                    authority:
                        crate::file_mutation::optiscaler::ReusedMutationAuthority::OptiScalerArtifact,
                },
            },
        ),
        crate::file_mutation::optiscaler::OptiScalerPlannedOperation::Relocate {
            source: crate::file_mutation::optiscaler::PlannedParticipant {
                path: downstream.clone(),
                preimage: crate::file_mutation::optiscaler::PlannedPreimage::ExactReused {
                    current: topology
                        .downstream
                        .as_ref()
                        .expect("downstream")
                        .receipt
                        .clone(),
                    authority:
                        crate::file_mutation::optiscaler::ReusedMutationAuthority::RelocationSource,
                },
            },
            destination: crate::file_mutation::optiscaler::PlannedParticipant {
                path: proxy.clone(),
                // The preceding Delete is the producer for this root
                // endpoint; the relocation itself requires an explicit
                // absent destination and resolves that producer through
                // the journal ordinal chain.
                preimage: crate::file_mutation::optiscaler::PlannedPreimage::Absent,
            },
        },
    ];
    crate::file_mutation::optiscaler::run_optiscaler_mutation(
        &crate::file_mutation::optiscaler::OptiScalerMutation {
            context: &context,
            guard: &guard,
            scope: &scope,
            feature: renderpilot_domain::mutation_features::OPTISCALER_UNINSTALL,
            subject_id: Some(&topology.id),
            operations: uninstall_operations,
            managed_endpoint_roots: Vec::new(),
            threat_model: crate::file_mutation::optiscaler::ThreatModel::CooperativeSameUid,
        },
        |mutation| {
            mutation.verify_unchanged(&config)?;
            mutation.delete_file_exact(&proxy, &topology.outer.receipt)?;
            changed.push(proxy.to_string_lossy().into_owned());
            let downstream = topology.downstream.as_ref().expect("downstream");
            let downstream_path = Path::new(downstream.path.as_str());
            let restored = mutation.relocate_file_exact(
                downstream_path,
                &proxy,
                &downstream.receipt,
                downstream.receipt.ownership(),
            )?;
            assert_eq!(restored.identity(), downstream.receipt.identity());
            assert_eq!(restored.digest(), downstream.receipt.digest());
            changed.push(proxy.to_string_lossy().into_owned());
            changed.push(downstream_path.to_string_lossy().into_owned());
            Ok::<_, ServiceError>(())
        },
        |mutation, ()| commit_journal_aggregate(mutation, &context, &topology.game_id, None, None),
        |()| {},
        || {},
    )
    .expect("release");
    assert_eq!(std::fs::read(&proxy).expect("restored proxy"), b"peer");
    assert_eq!(
        exact_receipt_from_live(&proxy, FileOwnership::Reused)
            .expect("restored receipt")
            .identity(),
        downstream_identity.as_str()
    );
    assert!(!downstream.exists());
    assert!(!root.path().join("dxgi.dll.bak").exists());
    assert_eq!(
        changed,
        vec![
            proxy.to_string_lossy().into_owned(),
            proxy.to_string_lossy().into_owned(),
            Path::new(
                topology
                    .downstream
                    .as_ref()
                    .expect("downstream")
                    .path
                    .as_str(),
            )
            .to_string_lossy()
            .into_owned(),
        ]
    );
}
