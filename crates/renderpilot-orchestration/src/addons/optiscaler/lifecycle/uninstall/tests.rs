use super::super::*;
use super::plan::{UninstallStep, VerifyPreimage, validate_producer_registry};
use super::plan::{canonical_postcommit_directories, validate_directory_emptiness};
use std::{fs, path::Path};
use tempfile::{TempDir, tempdir};

fn owned_receipt(seed: char) -> FileReceipt {
    FileReceipt::owned(
        format!("identity-{seed}"),
        Sha256Hash::new(seed.to_string().repeat(64)).expect("hash"),
    )
    .expect("receipt")
}

#[test]
fn typed_mutation_steps_lower_one_to_one() {
    let outer = PathBuf::from("game/dxgi.dll");
    let downstream = PathBuf::from("game/ReShade64.dll");
    let receipt = owned_receipt('a');
    let steps = [
        UninstallStep::DeleteTopologyOuter {
            path: outer.clone(),
            receipt: receipt.clone(),
        },
        UninstallStep::RelocatePeer {
            source: downstream,
            destination: outer,
            receipt,
        },
    ];
    let operations = steps
        .iter()
        .map(UninstallStep::operation)
        .collect::<Vec<_>>();
    assert_eq!(operations.len(), steps.len());
    assert!(matches!(
        operations[0],
        crate::file_mutation::optiscaler::OptiScalerPlannedOperation::Delete(_)
    ));
    assert!(matches!(
        operations[1],
        crate::file_mutation::optiscaler::OptiScalerPlannedOperation::Relocate { .. }
    ));
}

#[test]
fn managed_cleanup_footprint_excludes_verification_and_deduplicates_paths() {
    let mutation_path = PathBuf::from("game/OptiScaler.dll");
    let receipt = owned_receipt('f');
    let plan = super::plan::UninstallFsPlan {
        precommit: vec![
            UninstallStep::VerifyNoMutation {
                path: mutation_path.clone(),
                preimage: VerifyPreimage::Absent,
            },
            UninstallStep::DeleteOwned {
                path: mutation_path.clone(),
                receipt: receipt.clone(),
            },
            UninstallStep::DeleteOwned {
                path: mutation_path.clone(),
                receipt,
            },
        ],
        postcommit_directories: Vec::new(),
        peer_transition: None,
        preserved_paths: Vec::new(),
    };

    let footprint = super::execution::footprint(&plan);

    assert_eq!(footprint.exact_mutations, vec![mutation_path]);
    assert!(footprint.removed_directories.is_empty());
}

#[test]
fn configuration_restore_lowers_to_one_identity_preserving_write() {
    let path = PathBuf::from("game/OptiScaler.ini");
    let receipt = owned_receipt('e');
    let step = UninstallStep::RestoreConfiguration {
        path: path.clone(),
        receipt: receipt.clone(),
        bytes: b"baseline".to_vec(),
    };
    assert!(matches!(
        step.operation(),
        crate::file_mutation::optiscaler::OptiScalerPlannedOperation::Write(
            crate::file_mutation::optiscaler::PlannedParticipant {
                path: operation_path,
                preimage: crate::file_mutation::optiscaler::PlannedPreimage::Exact {
                    current,
                    prior_owned: Some(_),
                },
            }
        ) if operation_path == path && current == receipt
    ));
}

#[test]
fn root_delete_then_relocation_is_the_only_allowed_duplicate_producer() {
    let root = PathBuf::from("game/dxgi.dll");
    let receipt = owned_receipt('b');
    let allowed = vec![
        UninstallStep::DeleteTopologyOuter {
            path: root.clone(),
            receipt: receipt.clone(),
        },
        UninstallStep::RelocatePeer {
            source: PathBuf::from("game/ReShade64.dll"),
            destination: root,
            receipt,
        },
    ];
    assert!(validate_producer_registry(&allowed).is_ok());
}

#[test]
fn peer_sidecar_must_follow_main_relocation() {
    let receipt = owned_receipt('d');
    let main = UninstallStep::RelocatePeer {
        source: PathBuf::from("game/ReShade64.dll"),
        destination: PathBuf::from("game/dxgi.dll"),
        receipt: receipt.clone(),
    };
    let sidecar = UninstallStep::RelocatePeerSidecar {
        source: PathBuf::from("game/ReShade64.dll.bak"),
        destination: PathBuf::from("game/dxgi.dll.bak"),
        receipt,
    };
    assert!(validate_producer_registry(&[main.clone(), sidecar.clone()]).is_ok());
    assert!(validate_producer_registry(&[sidecar, main]).is_err());
}

#[test]
fn unrelated_duplicate_producers_fail_closed() {
    let path = PathBuf::from("game/OptiScaler.dll");
    let receipt = owned_receipt('c');
    let duplicate = vec![
        UninstallStep::DeleteOwned {
            path: path.clone(),
            receipt: receipt.clone(),
        },
        UninstallStep::DeleteOwned { path, receipt },
    ];
    assert!(validate_producer_registry(&duplicate).is_err());
}

struct PlanFixture {
    _root: TempDir,
    state: OptiScalerInstallState,
    topology: GameProxyTopology,
    config: PathBuf,
}

fn plan_fixture() -> PlanFixture {
    let root = tempdir().expect("game root");
    let game_id = GameId::new("manual:typed-uninstall").expect("game id");
    let root_path = root.path().to_path_buf();
    let proxy = root_path.join("dxgi.dll");
    let downstream = root_path.join("ReShade64.dll");
    let config = root_path.join("OptiScaler.ini");
    let runtime = root_path.join("OptiScaler").join("runtime.dll");
    let reused_runtime = root_path.join("plugins").join("foreign.dll");
    fs::create_dir_all(runtime.parent().expect("runtime parent")).expect("runtime parent");
    fs::create_dir_all(reused_runtime.parent().expect("foreign parent")).expect("foreign parent");
    fs::write(&proxy, b"opti").expect("proxy");
    fs::write(&downstream, b"reshade").expect("downstream");
    fs::write(&config, b"[OptiScaler]\n").expect("config");
    fs::write(&runtime, b"runtime").expect("runtime");
    fs::write(&reused_runtime, b"foreign").expect("foreign runtime");
    let proxy_receipt =
        super::super::exact_receipt_from_live(&proxy, FileOwnership::Owned).expect("proxy receipt");
    let downstream_receipt =
        super::super::exact_receipt_from_live(&downstream, FileOwnership::Reused)
            .expect("downstream receipt");
    let config_receipt = super::super::exact_receipt_from_live(&config, FileOwnership::Owned)
        .expect("config receipt");
    let config_baseline_receipt =
        super::super::exact_receipt_from_live(&config, FileOwnership::Reused)
            .expect("config baseline receipt");
    let runtime_receipt = super::super::exact_receipt_from_live(&runtime, FileOwnership::Owned)
        .expect("runtime receipt");
    let reused_receipt =
        super::super::exact_receipt_from_live(&reused_runtime, FileOwnership::Reused)
            .expect("reused runtime receipt");
    let state = renderpilot_domain::from_new_install(
        renderpilot_domain::OptiScalerInstallStateParts {
            game_id: game_id.clone(),
            release_id: "v1.0.0".to_owned(),
            manifest_revision: "test".to_owned(),
            archive_sha256: None,
            source: None,
            target_exe_path: PathRef::new(
                root_path.join("game.exe").to_string_lossy().into_owned(),
            )
            .expect("exe path"),
            target_dir: PathRef::new(root_path.to_string_lossy().into_owned()).expect("target dir"),
            modules: vec!["core".to_owned(), "foreign".to_owned()],
            release_files: vec![
                OptiScalerFileReceipt {
                    path: PathRef::new(config.to_string_lossy().into_owned()).expect("config path"),
                    installed: config_receipt,
                    role: OptiScalerFileRole::Configuration,
                    cleanup: OptiScalerFileCleanup::PreserveCurrentThenRestoreBaseline,
                    baseline: renderpilot_domain::OptiScalerReleaseFileBaseline::Absent,
                },
                OptiScalerFileReceipt {
                    path: PathRef::new(runtime.to_string_lossy().into_owned())
                        .expect("runtime path"),
                    installed: runtime_receipt,
                    role: OptiScalerFileRole::Runtime,
                    cleanup: OptiScalerFileCleanup::RemoveIfUnchanged,
                    baseline: renderpilot_domain::OptiScalerReleaseFileBaseline::Absent,
                },
            ],
            runtime_bindings: vec![renderpilot_domain::OptiScalerModuleRuntimeBinding {
                module: "foreign".to_owned(),
                path: PathRef::new(reused_runtime.to_string_lossy().into_owned())
                    .expect("foreign path"),
                installed: reused_receipt.clone(),
                baseline: OptiScalerFileBaseline::Present {
                    receipt: reused_receipt,
                },
            }],
            directory_receipts: Vec::new(),
            proxy_topology_id: Some("optiscaler:manual:typed-uninstall".to_owned()),
            config_schema: 1,
            config_base_release: "v1.0.0".to_owned(),
            adoption_state: OptiScalerAdoptionState::Managed,
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
    let topology = GameProxyTopology {
        id: "optiscaler:manual:typed-uninstall".to_owned(),
        game_id,
        root_slot: PathRef::new(proxy.to_string_lossy().into_owned()).expect("root slot"),
        outer: ProxyLink {
            implementation: ProxyImplementation::OptiScaler,
            path: PathRef::new(proxy.to_string_lossy().into_owned()).expect("outer path"),
            receipt: proxy_receipt,
        },
        downstream: Some(ProxyLink {
            implementation: ProxyImplementation::ReShade,
            path: PathRef::new(downstream.to_string_lossy().into_owned()).expect("downstream path"),
            receipt: downstream_receipt,
        }),
        downstream_origin: Some(
            PathRef::new(proxy.to_string_lossy().into_owned()).expect("origin"),
        ),
        root_prestate: ProxyRootPrestate::RelocatedDownstream,
    };
    PlanFixture {
        _root: root,
        state,
        topology,
        config,
    }
}

#[test]
fn real_builder_orders_configuration_runtime_outer_and_peer_steps() {
    let fixture = plan_fixture();
    let plan =
        super::plan::build_uninstall_plan(&fixture.state, &fixture.topology, None, &HashSet::new())
            .expect("typed uninstall plan");
    let config_write = plan
        .precommit
        .iter()
        .position(|step| matches!(step, UninstallStep::PreserveConfiguration { .. }))
        .expect("config recovery write");
    let config_restore = plan
        .precommit
        .iter()
        .position(|step| {
            matches!(step, UninstallStep::RestoreConfiguration { path, .. } if crate::paths::same_path(path, &fixture.config))
        })
        .expect("config source restore");
    let runtime_delete = plan
        .precommit
        .iter()
        .position(|step| matches!(step, UninstallStep::DeleteOwned { path, .. } if path.ends_with("runtime.dll")))
        .expect("runtime delete");
    let outer_delete = plan
        .precommit
        .iter()
        .position(|step| matches!(step, UninstallStep::DeleteTopologyOuter { .. }))
        .expect("outer delete");
    let peer_relocate = plan
        .precommit
        .iter()
        .position(|step| matches!(step, UninstallStep::RelocatePeer { .. }))
        .expect("peer relocation");
    assert!(config_write < config_restore);
    assert!(config_restore < runtime_delete);
    assert_eq!(outer_delete + 1, peer_relocate);
    let reused_runtime_delete = plan
        .precommit
        .iter()
        .position(|step| {
            matches!(
                step,
                UninstallStep::DeleteReusedArtifact { path, .. }
                    if path.ends_with("foreign.dll")
            )
        })
        .expect("exact-adopted runtime delete");
    assert!(
        plan.precommit[..reused_runtime_delete]
            .iter()
            .all(|step| { !matches!(step, UninstallStep::VerifyNoMutation { .. }) })
    );
    assert!(
        plan.precommit[reused_runtime_delete..]
            .iter()
            .all(|step| !matches!(step, UninstallStep::VerifyNoMutation { .. }))
    );
}

#[test]
fn real_builder_restores_present_owned_configuration_without_a_source_delete() {
    let fixture = plan_fixture();
    let plan =
        super::plan::build_uninstall_plan(&fixture.state, &fixture.topology, None, &HashSet::new())
            .expect("typed uninstall plan");
    let config_steps = plan
        .precommit
        .iter()
        .filter(|step| {
            matches!(
                step,
                UninstallStep::PreserveConfiguration { plan }
                    if crate::paths::same_path(&plan.source, &fixture.config)
            ) || matches!(
                step,
                UninstallStep::RestoreConfiguration { path, .. }
                    if crate::paths::same_path(path, &fixture.config)
            )
        })
        .collect::<Vec<_>>();
    assert!(matches!(
        config_steps.as_slice(),
        [
            UninstallStep::PreserveConfiguration { .. },
            UninstallStep::RestoreConfiguration { .. }
        ]
    ));
    assert!(!plan.precommit.iter().any(|step| {
        matches!(
            step,
            UninstallStep::DeleteOwned { path, .. }
                if crate::paths::same_path(path, &fixture.config)
        )
    }));
}

#[test]
fn real_builder_verifies_exact_reused_configuration_without_recovery() {
    let mut fixture = plan_fixture();
    let configuration = fixture
        .state
        .release_files
        .iter_mut()
        .find(|receipt| receipt.role == OptiScalerFileRole::Configuration)
        .expect("configuration receipt");
    configuration.installed = FileReceipt::reused(
        configuration.installed.identity().to_owned(),
        configuration.installed.digest().clone(),
    )
    .expect("reused receipt");
    configuration.cleanup = OptiScalerFileCleanup::PreserveUnchanged;

    let plan =
        super::plan::build_uninstall_plan(&fixture.state, &fixture.topology, None, &HashSet::new())
            .expect("typed uninstall plan");
    assert!(plan.precommit.iter().any(|step| {
        matches!(
            step,
            UninstallStep::VerifyNoMutation { path, .. }
                if crate::paths::same_path(path, &fixture.config)
        )
    }));
    assert!(!plan.precommit.iter().any(|step| {
        matches!(step, UninstallStep::PreserveConfiguration { .. })
            || matches!(step, UninstallStep::RestoreConfiguration { .. })
            || matches!(
                step,
                UninstallStep::DeleteOwned { path, .. }
                    if crate::paths::same_path(path, &fixture.config)
            )
    }));
}

#[test]
fn reused_configuration_uninstall_observes_a_user_edit_without_claiming_or_restoring_it() {
    let mut fixture = plan_fixture();
    let configuration = fixture
        .state
        .release_files
        .iter_mut()
        .find(|receipt| receipt.role == OptiScalerFileRole::Configuration)
        .expect("configuration receipt");
    configuration.installed = FileReceipt::reused(
        configuration.installed.identity().to_owned(),
        configuration.installed.digest().clone(),
    )
    .expect("reused receipt");
    configuration.cleanup = OptiScalerFileCleanup::PreserveUnchanged;
    let user_bytes = b"[OptiScaler]\nUserOverride=true\n";
    let saved_original = fixture.config.with_file_name("OptiScaler.user-save");
    fs::rename(&fixture.config, &saved_original).expect("move adopted config aside");
    fs::write(&fixture.config, user_bytes).expect("recreate user config");

    let plan =
        super::plan::build_uninstall_plan(&fixture.state, &fixture.topology, None, &HashSet::new())
            .expect("uninstall observes user configuration");
    let persisted = fixture
        .state
        .configuration_receipt()
        .expect("persisted configuration")
        .installed
        .clone();
    assert!(plan.precommit.iter().any(|step| {
        matches!(
            step,
            UninstallStep::VerifyNoMutation {
                path,
                preimage: VerifyPreimage::ReusedConfiguration(live),
            } if crate::paths::same_path(path, &fixture.config)
                && live.identity() != persisted.identity()
                && live.digest() != persisted.digest()
        )
    }));
    assert!(!plan.precommit.iter().any(|step| {
        step.mutation_paths()
            .into_iter()
            .any(|path| crate::paths::same_path(path, &fixture.config))
    }));
    assert_eq!(
        fs::read(&fixture.config).expect("user configuration after planning"),
        user_bytes,
        "uninstall planning must not mutate an atomically replaced Reused configuration"
    );
}

#[test]
fn reused_configuration_uninstall_accepts_an_already_missing_user_file() {
    let mut fixture = plan_fixture();
    let configuration = fixture
        .state
        .release_files
        .iter_mut()
        .find(|receipt| receipt.role == OptiScalerFileRole::Configuration)
        .expect("configuration receipt");
    configuration.installed = FileReceipt::reused(
        configuration.installed.identity().to_owned(),
        configuration.installed.digest().clone(),
    )
    .expect("reused receipt");
    configuration.cleanup = OptiScalerFileCleanup::PreserveUnchanged;
    fs::remove_file(&fixture.config).expect("remove user configuration");

    let plan =
        super::plan::build_uninstall_plan(&fixture.state, &fixture.topology, None, &HashSet::new())
            .expect("uninstall accepts missing reused configuration");
    assert!(plan.precommit.iter().any(|step| {
        matches!(
            step,
            UninstallStep::VerifyNoMutation {
                path,
                preimage: VerifyPreimage::Absent,
            } if crate::paths::same_path(path, &fixture.config)
        )
    }));
}

#[test]
fn real_builder_restores_later_created_peer_from_absent_root_prestate() {
    let mut fixture = plan_fixture();
    fixture.topology.root_prestate = ProxyRootPrestate::Absent;
    fixture
        .topology
        .validate()
        .expect("later-created peer topology");
    let plan =
        super::plan::build_uninstall_plan(&fixture.state, &fixture.topology, None, &HashSet::new())
            .expect("typed later-created peer uninstall plan");
    let outer_delete = plan
        .precommit
        .iter()
        .position(|step| matches!(step, UninstallStep::DeleteTopologyOuter { .. }))
        .expect("outer delete");
    assert!(matches!(
        plan.precommit.get(outer_delete + 1),
        Some(UninstallStep::RelocatePeer { destination, .. })
            if crate::paths::same_path(destination, Path::new(fixture.topology.root_slot.as_str()))
    ));
}

#[test]
fn real_builder_groups_configuration_actions_before_other_release_files() {
    let mut fixture = plan_fixture();
    fixture.state.release_files.reverse();
    let plan =
        super::plan::build_uninstall_plan(&fixture.state, &fixture.topology, None, &HashSet::new())
            .expect("typed uninstall plan");
    let config_write = plan
        .precommit
        .iter()
        .position(|step| matches!(step, UninstallStep::PreserveConfiguration { .. }))
        .expect("config recovery write");
    let config_restore = plan
        .precommit
        .iter()
        .position(|step| {
            matches!(step, UninstallStep::RestoreConfiguration { path, .. } if crate::paths::same_path(
                path,
                &fixture.config
            ))
        })
        .expect("config source restore");
    let runtime_delete = plan
        .precommit
        .iter()
        .position(|step| {
            matches!(step, UninstallStep::DeleteOwned { path, .. } if path.ends_with("runtime.dll"))
        })
        .expect("runtime delete");
    assert!(config_write < config_restore);
    assert!(config_restore < runtime_delete);
}

#[test]
fn owned_downstream_requires_a_peer_receipt_transition() {
    let mut fixture = plan_fixture();
    let downstream = fixture.topology.downstream.as_mut().expect("downstream");
    downstream.receipt = FileReceipt::owned(
        downstream.receipt.identity().to_owned(),
        downstream.receipt.digest().clone(),
    )
    .expect("owned downstream receipt");

    assert!(
        super::plan::build_uninstall_plan(&fixture.state, &fixture.topology, None, &HashSet::new())
            .is_err()
    );
}

#[test]
fn real_builder_relocates_an_owned_downstream_with_its_peer_receipt_transition() {
    let mut fixture = plan_fixture();
    let downstream_receipt = fixture.topology.downstream.as_mut().expect("downstream");
    downstream_receipt.receipt = FileReceipt::owned(
        downstream_receipt.receipt.identity().to_owned(),
        downstream_receipt.receipt.digest().clone(),
    )
    .expect("owned downstream receipt");
    let downstream = fixture
        .topology
        .downstream
        .as_ref()
        .expect("downstream")
        .path
        .clone();
    let root = fixture.topology.root_slot.clone();
    let source = crate::fs::backup_path(Path::new(downstream.as_str())).expect("source sidecar");
    let destination =
        crate::fs::backup_path(Path::new(root.as_str())).expect("destination sidecar");
    fs::write(&source, b"peer baseline").expect("source sidecar bytes");
    let source_receipt = super::super::exact_receipt_from_live(&source, FileOwnership::Owned)
        .expect("owned sidecar receipt");
    let peer = InstalledAddon::new(
        fixture.state.game_id.clone(),
        AddonKind::RenoDx,
        PathRef::new(
            fixture
                ._root
                .path()
                .join("peer.addon64")
                .to_string_lossy()
                .into_owned(),
        )
        .expect("peer addon path"),
    );
    let transition = super::super::adoption::PeerHostTransitionPlan {
        from: downstream,
        to: root,
        live_sha256: fixture
            .topology
            .downstream
            .as_ref()
            .expect("downstream")
            .receipt
            .digest()
            .clone(),
        destination_receipt: None,
        receipt: super::super::adoption::PeerReceiptTransition {
            before: peer.clone(),
            after: peer,
        },
        sidecar: Some(super::super::adoption::PeerSidecarTransition {
            source,
            destination,
            source_receipt,
        }),
        destination_ownership: FileOwnership::Owned,
    };
    let plan = super::plan::build_uninstall_plan(
        &fixture.state,
        &fixture.topology,
        Some(transition),
        &HashSet::new(),
    )
    .expect("typed uninstall plan");
    let sidecar = plan
        .precommit
        .iter()
        .find(|step| matches!(step, UninstallStep::RelocatePeerSidecar { .. }))
        .expect("sidecar relocation");
    let UninstallStep::RelocatePeerSidecar { receipt, .. } = sidecar else {
        unreachable!();
    };
    assert_eq!(receipt.ownership(), FileOwnership::Owned);
    let operation = sidecar.operation();
    let crate::file_mutation::optiscaler::OptiScalerPlannedOperation::Relocate { source, .. } =
        operation
    else {
        panic!("sidecar must lower to relocation");
    };
    let crate::file_mutation::optiscaler::PlannedPreimage::Exact { prior_owned, .. } =
        source.preimage
    else {
        panic!("owned sidecar must lower to exact preimage");
    };
    assert!(prior_owned.is_some());
}

#[test]
fn real_builder_rejects_edited_and_replaced_owned_configuration_before_row() {
    for mutation in [0, 2] {
        let fixture = plan_fixture();
        match mutation {
            0 => fs::write(&fixture.config, b"edited").expect("edit config"),
            _ => {
                fs::remove_file(&fixture.config).expect("remove config");
                fs::write(&fixture.config, b"replacement").expect("replace config");
            }
        }
        assert!(
            super::plan::build_uninstall_plan(
                &fixture.state,
                &fixture.topology,
                None,
                &HashSet::new(),
            )
            .is_err(),
            "configuration mutation {mutation} must fail before planning"
        );
    }
}

#[test]
fn real_builder_accepts_an_already_missing_owned_configuration() {
    let fixture = plan_fixture();
    fs::remove_file(&fixture.config).expect("remove config");
    let plan =
        super::plan::build_uninstall_plan(&fixture.state, &fixture.topology, None, &HashSet::new())
            .expect("missing configuration is already absent");
    assert!(plan.precommit.iter().any(|step| matches!(
        step,
        UninstallStep::VerifyNoMutation {
            path,
            preimage: VerifyPreimage::Absent,
        } if crate::paths::same_path(path, &fixture.config)
    )));
}

#[test]
fn real_builder_rejects_drifted_and_replaced_owned_runtime_before_row() {
    for mutation in [0, 2] {
        let fixture = plan_fixture();
        let runtime = fixture
            .state
            .release_files
            .iter()
            .find(|receipt| receipt.role == OptiScalerFileRole::Runtime)
            .expect("runtime receipt")
            .path
            .clone();
        let runtime = PathBuf::from(runtime.as_str());
        match mutation {
            0 => fs::write(&runtime, b"edited runtime").expect("edit runtime"),
            _ => {
                fs::remove_file(&runtime).expect("remove runtime");
                fs::write(&runtime, b"replacement runtime").expect("replace runtime");
            }
        }
        assert!(
            super::plan::build_uninstall_plan(
                &fixture.state,
                &fixture.topology,
                None,
                &HashSet::new(),
            )
            .is_err(),
            "runtime mutation {mutation} must fail before planning"
        );
    }
}

#[test]
fn real_builder_accepts_an_already_missing_owned_runtime() {
    let fixture = plan_fixture();
    let runtime = fixture
        .state
        .release_files
        .iter()
        .find(|receipt| receipt.role == OptiScalerFileRole::Runtime)
        .expect("runtime receipt")
        .path
        .clone();
    let runtime = PathBuf::from(runtime.as_str());
    fs::remove_file(&runtime).expect("remove runtime");
    let plan =
        super::plan::build_uninstall_plan(&fixture.state, &fixture.topology, None, &HashSet::new())
            .expect("missing runtime is already absent");
    assert!(plan.precommit.iter().any(|step| matches!(
        step,
        UninstallStep::VerifyNoMutation {
            path,
            preimage: VerifyPreimage::Absent,
        } if crate::paths::same_path(path, &runtime)
    )));
}

#[test]
fn real_builder_places_shared_path_verification_after_all_mutations() {
    let fixture = plan_fixture();
    let shared = fixture
        .state
        .release_files
        .iter()
        .find(|receipt| receipt.role == OptiScalerFileRole::Runtime)
        .expect("runtime receipt")
        .path
        .clone();
    let shared_key = crate::paths::normalized_key(Path::new(shared.as_str()));
    let plan = super::plan::build_uninstall_plan(
        &fixture.state,
        &fixture.topology,
        None,
        &HashSet::from([shared_key]),
    )
    .expect("shared typed uninstall plan");
    let verify = plan
        .precommit
        .iter()
        .position(|step| matches!(step, UninstallStep::VerifyNoMutation { .. }))
        .expect("shared verification");
    assert!(
        plan.precommit[..verify]
            .iter()
            .all(|step| !matches!(step, UninstallStep::VerifyNoMutation { .. }))
    );
    assert!(
        plan.precommit[verify..]
            .iter()
            .all(|step| matches!(step, UninstallStep::VerifyNoMutation { .. }))
    );
}

#[test]
fn canonical_nested_postcommit_directories_are_deepest_first_and_normalized() {
    let root = tempdir().expect("root");
    let nested = root.path().join("nested");
    fs::create_dir(&nested).expect("nested");
    let root_receipt = renderpilot_domain::OptiScalerDirectoryReceipt {
        path: PathRef::new(root.path().to_string_lossy().into_owned()).expect("root path"),
        identity: crate::fs::VerifiedDir::open(root.path())
            .expect("root authority")
            .identity()
            .to_owned(),
    };
    let nested_receipt = renderpilot_domain::OptiScalerDirectoryReceipt {
        path: PathRef::new(nested.to_string_lossy().into_owned()).expect("nested path"),
        identity: crate::fs::VerifiedDir::open(&nested)
            .expect("nested authority")
            .identity()
            .to_owned(),
    };
    let sorted = canonical_postcommit_directories(&[root_receipt, nested_receipt])
        .expect("canonical directories");
    assert_eq!(
        crate::paths::normalized_key(Path::new(sorted[0].path.as_str())),
        crate::paths::normalized_key(&nested)
    );
    assert_eq!(
        crate::paths::normalized_key(Path::new(sorted[1].path.as_str())),
        crate::paths::normalized_key(root.path())
    );
}

#[test]
fn unmanaged_directory_child_blocks_closed_cleanup_projection() {
    let root = tempdir().expect("root");
    let child = root.path().join("unmanaged.bin");
    fs::write(&child, b"foreign").expect("child");
    let receipt = renderpilot_domain::OptiScalerDirectoryReceipt {
        path: PathRef::new(root.path().to_string_lossy().into_owned()).expect("root path"),
        identity: crate::fs::VerifiedDir::open(root.path())
            .expect("root authority")
            .identity()
            .to_owned(),
    };
    assert!(validate_directory_emptiness(&[receipt], &[]).is_err());
}

#[test]
fn scheduled_owned_delete_is_accepted_by_closed_cleanup_projection() {
    let root = tempdir().expect("root");
    let child = root.path().join("owned.bin");
    fs::write(&child, b"owned").expect("child");
    let directory = renderpilot_domain::OptiScalerDirectoryReceipt {
        path: PathRef::new(root.path().to_string_lossy().into_owned()).expect("root path"),
        identity: crate::fs::VerifiedDir::open(root.path())
            .expect("root authority")
            .identity()
            .to_owned(),
    };
    let receipt =
        super::super::exact_receipt_from_live(&child, FileOwnership::Owned).expect("child receipt");
    let step = UninstallStep::DeleteOwned {
        path: child,
        receipt,
    };
    assert!(validate_directory_emptiness(&[directory], &[step]).is_ok());
}

#[test]
fn created_directory_inside_scheduled_cleanup_is_rejected() {
    let root = tempdir().expect("root");
    let created = root.path().join("created-later");
    let directory = renderpilot_domain::OptiScalerDirectoryReceipt {
        path: PathRef::new(root.path().to_string_lossy().into_owned()).expect("root path"),
        identity: crate::fs::VerifiedDir::open(root.path())
            .expect("root authority")
            .identity()
            .to_owned(),
    };
    let step = UninstallStep::CreateDirectory { path: created };
    assert!(validate_directory_emptiness(&[directory], &[step]).is_err());
}

#[test]
fn transaction_namespace_is_an_unmanaged_child_of_closed_cleanup_projection() {
    let root = tempdir().expect("root");
    fs::create_dir(root.path().join(".renderpilot-optiscaler-private-test"))
        .expect("transaction namespace");
    let directory = renderpilot_domain::OptiScalerDirectoryReceipt {
        path: PathRef::new(root.path().to_string_lossy().into_owned()).expect("root path"),
        identity: crate::fs::VerifiedDir::open(root.path())
            .expect("root authority")
            .identity()
            .to_owned(),
    };
    assert!(validate_directory_emptiness(&[directory], &[]).is_err());
}
