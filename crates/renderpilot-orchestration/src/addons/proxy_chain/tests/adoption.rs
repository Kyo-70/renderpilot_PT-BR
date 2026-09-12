use super::*;
use std::cell::RefCell;
use tempfile::tempdir;

#[test]
fn preexisting_exact_outer_is_reused_and_removed_by_explicit_uninstall() {
    let root = tempdir().expect("root");
    let proxy = root.path().join("dxgi.dll");
    let config = root.path().join("OptiScaler.ini");
    let outer = b"preexisting exact outer";
    std::fs::write(&proxy, outer).expect("outer");
    let outer_sha256 = crate::fs::sha256_of_non_empty_file(&proxy).expect("outer hash");
    let reused_outer =
        exact_receipt_from_live(&proxy, FileOwnership::Reused).expect("Reused outer receipt");
    let context = Context::open_at(root.path().join("catalog.db")).expect("context");
    let game_id = GameId::new("manual:preexisting-exact-outer").expect("game id");
    seed_game(&context, &game_id, root.path());
    let guard = crate::game_mutation_lock::try_lock(&game_id).expect("guard");
    let scope = crate::file_mutation::MutationScope::single(root.path()).expect("scope");
    let installed_state = RefCell::new(None);
    let subject = format!("optiscaler:{}", game_id.as_str());
    let verify = || {
        vec![
            crate::file_mutation::optiscaler::OptiScalerPlannedOperation::Verify(
                crate::file_mutation::optiscaler::PlannedParticipant {
                    path: proxy.clone(),
                    preimage: crate::file_mutation::optiscaler::PlannedPreimage::ExactReused {
                        current: reused_outer.clone(),
                        authority:
                            crate::file_mutation::optiscaler::ReusedMutationAuthority::ObservationOnly,
                    },
                },
            ),
        ]
    };
    let mut install_operations = verify();
    install_operations.push(
        crate::file_mutation::optiscaler::OptiScalerPlannedOperation::Write(
            crate::file_mutation::optiscaler::PlannedParticipant {
                path: config.clone(),
                preimage: crate::file_mutation::optiscaler::PlannedPreimage::Absent,
            },
        ),
    );
    let mut topology = None;
    crate::file_mutation::optiscaler::run_optiscaler_mutation(
        &crate::file_mutation::optiscaler::OptiScalerMutation {
            context: &context,
            guard: &guard,
            scope: &scope,
            feature: renderpilot_domain::mutation_features::OPTISCALER_INSTALL,
            subject_id: Some(subject.as_str()),
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
                    downstream: None,
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
        |value| topology = Some(value.clone()),
        || {},
    )
    .expect("adopt exact outer");
    let topology = topology.expect("topology");
    let state = installed_state.into_inner().expect("state");
    assert_eq!(topology.outer.receipt.ownership(), FileOwnership::Reused);

    crate::file_mutation::optiscaler::run_optiscaler_mutation(
        &crate::file_mutation::optiscaler::OptiScalerMutation {
            context: &context,
            guard: &guard,
            scope: &scope,
            feature: renderpilot_domain::mutation_features::OPTISCALER_UNINSTALL,
            subject_id: Some(subject.as_str()),
            operations: vec![
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
                        preimage: crate::file_mutation::optiscaler::PlannedPreimage::ExactReused {
                            current: topology.outer.receipt.clone(),
                            authority:
                                crate::file_mutation::optiscaler::ReusedMutationAuthority::OptiScalerArtifact,
                        },
                    },
                ),
            ],
            managed_endpoint_roots: Vec::new(),
            threat_model: crate::file_mutation::optiscaler::ThreatModel::CooperativeSameUid,
        },
        |mutation| {
            mutation.delete_file_exact(&config, &state.release_files[0].installed)?;
            mutation.delete_file_exact(&proxy, &topology.outer.receipt)?;
            Ok::<_, ServiceError>(())
        },
        |mutation, ()| commit_journal_aggregate(mutation, &context, &topology.game_id, None, None),
        |()| {},
        || {},
    )
    .expect("remove exact Reused outer");
    assert!(!proxy.exists());
}
