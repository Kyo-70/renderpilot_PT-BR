use super::*;
use std::cell::RefCell;
use tempfile::tempdir;

#[test]
fn fresh_unmanaged_chain_round_trips_without_claiming_a_foreign_sidecar() {
    let root = tempdir().expect("root");
    let proxy = root.path().join("dxgi.dll");
    let downstream = root.path().join("ReShade64.dll");
    let config = root.path().join("OptiScaler.ini");
    let foreign_sidecar = root.path().join("dxgi.dll.bak");
    let reshade = crate::addons::test_support::build_pe_with_exports(
        crate::addons::test_support::MACHINE_AMD64,
        crate::addons::test_support::PE32_PLUS_MAGIC,
        &[
            "ReShadeVersion",
            "ReShadeRegisterAddon",
            "ReShadeUnregisterAddon",
            "ReShadeRegisterEvent",
            "ReShadeUnregisterEvent",
        ],
    );
    let outer = b"exact OptiScaler outer";
    let outer_source = root.path().join("outer.cache");
    std::fs::write(&proxy, &reshade).expect("ReShade root");
    std::fs::write(&outer_source, outer).expect("outer source");
    let outer_sha256 = crate::fs::sha256_of_non_empty_file(&outer_source).expect("outer hash");
    let reshade_sha256 = crate::fs::sha256_of_non_empty_file(&proxy).expect("ReShade hash");
    std::fs::write(&foreign_sidecar, b"foreign backup").expect("foreign sidecar");

    let context = Context::open_at(root.path().join("catalog.db")).expect("context");
    let game_id = GameId::new("manual:fresh-proxy-roundtrip").expect("game id");
    seed_game(&context, &game_id, root.path());
    let guard = crate::game_mutation_lock::try_lock(&game_id).expect("guard");
    let scope = crate::file_mutation::MutationScope::single(root.path()).expect("scope");
    let subject = format!("optiscaler:{}", game_id.as_str());
    let reshade_receipt =
        exact_receipt_from_live(&proxy, FileOwnership::Reused).expect("ReShade receipt");
    let installed_state = RefCell::new(None);
    let install_operations = vec![
        crate::file_mutation::optiscaler::OptiScalerPlannedOperation::Relocate {
            source: crate::file_mutation::optiscaler::PlannedParticipant {
                path: proxy.clone(),
                preimage: crate::file_mutation::optiscaler::PlannedPreimage::ExactReused {
                    current: reshade_receipt,
                    authority:
                        crate::file_mutation::optiscaler::ReusedMutationAuthority::RelocationSource,
                },
            },
            destination: crate::file_mutation::optiscaler::PlannedParticipant {
                path: downstream.clone(),
                preimage: crate::file_mutation::optiscaler::PlannedPreimage::Absent,
            },
        },
        crate::file_mutation::optiscaler::OptiScalerPlannedOperation::Write(
            crate::file_mutation::optiscaler::PlannedParticipant {
                path: proxy.clone(),
                preimage: crate::file_mutation::optiscaler::PlannedPreimage::Absent,
            },
        ),
        crate::file_mutation::optiscaler::OptiScalerPlannedOperation::Write(
            crate::file_mutation::optiscaler::PlannedParticipant {
                path: config.clone(),
                preimage: crate::file_mutation::optiscaler::PlannedPreimage::Absent,
            },
        ),
    ];
    let mut installed_topology = None;
    crate::file_mutation::optiscaler::run_optiscaler_mutation(
        &crate::file_mutation::optiscaler::OptiScalerMutation {
            context: &context,
            guard: &guard,
            scope: &scope,
            feature: renderpilot_domain::mutation_features::OPTISCALER_INSTALL,
            subject_id: Some(&subject),
            operations: install_operations,
            managed_endpoint_roots: Vec::new(),
            threat_model: crate::file_mutation::optiscaler::ThreatModel::CooperativeSameUid,
        },
        |mutation| {
            let topology = execute_install_plan(
                &context,
                &ProxyInstallPlan {
                    game_id: &game_id,
                    root_slot: &proxy,
                    outer_sha256: outer_sha256.clone(),
                    updating: false,
                    downstream: Some(DownstreamInstallPlan::Transfer {
                        source_path: &proxy,
                        expected_source_sha256: &reshade_sha256,
                        destination_path: &downstream,
                        destination_ownership: FileOwnership::Reused,
                    }),
                },
                outer,
                mutation,
                &mut Vec::new(),
            )?;
            mutation.write_file(&config, b"[OptiScaler]\nEnabled=true\n")?;
            *installed_state.borrow_mut() = Some(state_from_config(
                &game_id,
                root.path(),
                &topology.id,
                &config,
            ));
            Ok::<_, ServiceError>(topology)
        },
        |mutation, topology| {
            let state = installed_state.borrow();
            commit_journal_aggregate(mutation, &context, &game_id, state.as_ref(), Some(topology))
        },
        |topology| installed_topology = Some(topology.clone()),
        || {},
    )
    .expect("install chain");
    let topology = installed_topology.expect("topology");
    let state = installed_state.into_inner().expect("state");
    topology.validate().expect("valid topology");
    assert_eq!(
        topology.root_prestate,
        ProxyRootPrestate::RelocatedDownstream
    );
    assert_eq!(std::fs::read(&proxy).expect("outer"), outer);
    assert_eq!(std::fs::read(&downstream).expect("downstream"), reshade);
    assert_eq!(
        std::fs::read(&foreign_sidecar).expect("foreign sidecar"),
        b"foreign backup"
    );

    let mut changed = Vec::new();
    let uninstall_operations = vec![
        crate::file_mutation::optiscaler::OptiScalerPlannedOperation::Delete(
            crate::file_mutation::optiscaler::PlannedParticipant {
                path: config.clone(),
                preimage: crate::file_mutation::optiscaler::PlannedPreimage::Exact {
                    current: state.release_files[0].installed.clone(),
                    prior_owned: Some(state.release_files[0].installed.clone()),
                },
            },
        ),
        crate::file_mutation::optiscaler::OptiScalerPlannedOperation::Delete(
            crate::file_mutation::optiscaler::PlannedParticipant {
                path: proxy.clone(),
                preimage: crate::file_mutation::optiscaler::PlannedPreimage::Exact {
                    current: topology.outer.receipt.clone(),
                    prior_owned: Some(topology.outer.receipt.clone()),
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
            subject_id: Some(&subject),
            operations: uninstall_operations,
            managed_endpoint_roots: Vec::new(),
            threat_model: crate::file_mutation::optiscaler::ThreatModel::CooperativeSameUid,
        },
        |mutation| {
            mutation.delete_file_exact(&config, &state.release_files[0].installed)?;
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
        |mutation, ()| commit_journal_aggregate(mutation, &context, &game_id, None, None),
        |()| {},
        || {},
    )
    .expect("uninstall chain");
    assert_eq!(std::fs::read(&proxy).expect("restored root"), reshade);
    assert!(!downstream.exists());
    assert_eq!(
        std::fs::read(&foreign_sidecar).expect("foreign sidecar"),
        b"foreign backup"
    );
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
