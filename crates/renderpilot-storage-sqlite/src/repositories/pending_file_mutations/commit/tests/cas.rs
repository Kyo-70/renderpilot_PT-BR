use super::*;

#[test]
fn shared_journal_is_the_only_accepted_storage_wire() {
    let journal = journal();
    validate_optiscaler_journal_json(&serde_json::to_string(&journal).expect("json"))
        .expect("journal");
    let mut unknown = serde_json::to_value(journal).expect("value");
    unknown["unexpected"] = serde_json::json!(true);
    assert!(validate_optiscaler_journal_json(&unknown.to_string()).is_err());
}

#[test]
fn prepared_boundary_requires_applied_precommit_and_planned_cleanup() {
    let journal = materialized_applied();
    validate_optiscaler_journal_for_prepared(&serde_json::to_string(&journal).expect("json"))
        .expect("prepared boundary");
    let mut preserved = journal;
    if let OperationEffect::Write(effect) = preserved.operations_mut()[0].effect_mut() {
        *effect.state_mut() = WriteState::Preserved;
    }
    assert!(
        validate_optiscaler_journal_for_prepared(&serde_json::to_string(&preserved).expect("json"))
            .is_err()
    );
}

#[test]
fn committed_terminal_accepts_cleared_historical_custody() {
    let mut journal = materialized_applied();
    let custody = DurableObservation::File {
        identity: "historical-custody".to_owned(),
        digest: hash(),
    };
    if let OperationEffect::Write(effect) = journal.operations_mut()[0].effect_mut()
        && let WriteState::Applied {
            custody: applied_custody,
            ..
        } = effect.state_mut()
    {
        *applied_custody = custody;
    }
    *journal.operations_mut()[0].slots_mut().custody_mut() = DurableObservation::Absent;
    journal.set_cleanup(renderpilot_domain::CleanupState::Complete);

    validate_optiscaler_journal_for_committed_terminal(
        &serde_json::to_string(&journal).expect("committed terminal json"),
    )
    .expect("committed cleanup may clear historical custody");
}

#[test]
fn committed_terminal_rejects_absence_before_the_terminal_action() {
    let mut journal = materialized_applied();
    if let OperationEffect::Write(effect) = journal.operations_mut()[0].effect_mut()
        && let WriteState::Applied { live, custody } = effect.state()
    {
        *effect.state_mut() = WriteState::DiscardIntent {
            postimage: live.clone(),
            custody: custody.clone(),
        };
    }
    *journal.operations_mut()[0].slots_mut().custody_mut() = DurableObservation::Absent;
    journal.set_cleanup(renderpilot_domain::CleanupState::Complete);

    assert!(
        validate_optiscaler_journal_for_committed_terminal(
            &serde_json::to_string(&journal).expect("invalid terminal json"),
        )
        .is_err(),
        "cleanup cannot hide an artifact before its terminal action"
    );
}

#[test]
fn publish_write_uses_stage_for_absent_and_preserves_existing_identity() {
    let stage = DurableObservation::File {
        identity: "stage-token".to_owned(),
        digest: Sha256Hash::new("c".repeat(64)).expect("stage digest"),
    };
    let (current, next) = write_publish_transition(
        Preimage::Initial {
            observation: DurableObservation::Absent,
            receipt: None,
            owned_basis: None,
        },
        &stage,
        DurableObservation::Absent,
        stage.clone(),
    );
    validate_optiscaler_journal_for_cas(
        &serde_json::to_string(&current).expect("absent current json"),
        &serde_json::to_string(&next).expect("absent next json"),
        "preparing",
    )
    .expect("absent publish uses exact stage");

    let prior = FileReceipt::owned(
        "target-identity",
        Sha256Hash::new("b".repeat(64)).expect("prior digest"),
    )
    .expect("prior receipt");
    let custody = DurableObservation::File {
        identity: "capture-token".to_owned(),
        digest: prior.digest().clone(),
    };
    let overwritten = DurableObservation::File {
        identity: prior.identity().to_owned(),
        digest: Sha256Hash::new("c".repeat(64)).expect("written digest"),
    };
    let (current, next) = write_publish_transition(
        Preimage::Initial {
            observation: file_observation(&prior),
            receipt: Some(prior.clone()),
            owned_basis: Some(prior.clone()),
        },
        &stage,
        custody,
        overwritten,
    );
    validate_optiscaler_journal_for_cas(
        &serde_json::to_string(&current).expect("existing current json"),
        &serde_json::to_string(&next).expect("existing next json"),
        "preparing",
    )
    .expect("existing publish preserves target identity");
}

#[test]
fn publish_write_rejects_wrong_existing_identity_or_digest_and_wrong_absent_live() {
    let prior = FileReceipt::owned(
        "target-identity",
        Sha256Hash::new("b".repeat(64)).expect("prior digest"),
    )
    .expect("prior receipt");
    let stage = DurableObservation::File {
        identity: "stage-token".to_owned(),
        digest: Sha256Hash::new("c".repeat(64)).expect("stage digest"),
    };
    let custody = DurableObservation::File {
        identity: "capture-token".to_owned(),
        digest: prior.digest().clone(),
    };
    for live in [
        DurableObservation::File {
            identity: "wrong-identity".to_owned(),
            digest: match &stage {
                DurableObservation::File { digest, .. } => digest.clone(),
                _ => unreachable!(),
            },
        },
        DurableObservation::File {
            identity: prior.identity().to_owned(),
            digest: Sha256Hash::new("d".repeat(64)).expect("wrong digest"),
        },
    ] {
        let (current, next) = write_publish_transition(
            Preimage::Initial {
                observation: file_observation(&prior),
                receipt: Some(prior.clone()),
                owned_basis: Some(prior.clone()),
            },
            &stage,
            custody.clone(),
            live.clone(),
        );
        assert!(
            validate_optiscaler_journal_for_cas(
                &serde_json::to_string(&current).expect("wrong existing current json"),
                &serde_json::to_string(&next).expect("wrong existing next json"),
                "preparing",
            )
            .is_err(),
            "wrong existing live observation must fail: {live:?}"
        );
    }

    let (current, next) = write_publish_transition(
        Preimage::Initial {
            observation: DurableObservation::Absent,
            receipt: None,
            owned_basis: None,
        },
        &stage,
        DurableObservation::Absent,
        DurableObservation::File {
            identity: "wrong-identity".to_owned(),
            digest: match &stage {
                DurableObservation::File { digest, .. } => digest.clone(),
                _ => unreachable!(),
            },
        },
    );
    assert!(
        validate_optiscaler_journal_for_cas(
            &serde_json::to_string(&current).expect("wrong absent current json"),
            &serde_json::to_string(&next).expect("wrong absent next json"),
            "preparing",
        )
        .is_err(),
        "absent preimage must publish the exact stage observation"
    );
}

#[test]
fn capture_write_requires_custody_to_match_the_resolved_preimage() {
    let absent = DurableObservation::Absent;
    let (current, next) = write_capture_transition(
        Preimage::Initial {
            observation: absent.clone(),
            receipt: None,
            owned_basis: None,
        },
        absent.clone(),
    );
    validate_optiscaler_journal_for_cas(
        &serde_json::to_string(&current).expect("absent capture current json"),
        &serde_json::to_string(&next).expect("absent capture next json"),
        "preparing",
    )
    .expect("absent preimage may capture absent custody");

    let preimage = DurableObservation::File {
        identity: "prior-file".to_owned(),
        digest: hash(),
    };
    let (current, next) = write_capture_transition(
        Preimage::Initial {
            observation: preimage.clone(),
            receipt: None,
            owned_basis: None,
        },
        preimage.clone(),
    );
    validate_optiscaler_journal_for_cas(
        &serde_json::to_string(&current).expect("file capture current json"),
        &serde_json::to_string(&next).expect("file capture next json"),
        "preparing",
    )
    .expect("file preimage must retain an exact custody copy");

    let distinct_custody = DurableObservation::File {
        identity: "private-custody".to_owned(),
        digest: hash(),
    };
    let (current, next) = write_capture_transition(
        Preimage::Initial {
            observation: preimage.clone(),
            receipt: None,
            owned_basis: None,
        },
        distinct_custody,
    );
    validate_optiscaler_journal_for_cas(
        &serde_json::to_string(&current).expect("distinct custody current json"),
        &serde_json::to_string(&next).expect("distinct custody next json"),
        "preparing",
    )
    .expect("private custody may use a distinct identity with the same digest");

    for (preimage, custody) in [
        (
            absent,
            DurableObservation::File {
                identity: "unexpected-custody".to_owned(),
                digest: hash(),
            },
        ),
        (preimage.clone(), DurableObservation::Absent),
        (
            preimage.clone(),
            DurableObservation::File {
                identity: "wrong-digest".to_owned(),
                digest: Sha256Hash::new("c".repeat(64)).expect("wrong digest"),
            },
        ),
        (
            preimage.clone(),
            DurableObservation::Directory {
                identity: "wrong-shape".to_owned(),
            },
        ),
        (preimage.clone(), DurableObservation::NonRegular),
        (preimage, DurableObservation::Unreadable),
    ] {
        let (current, next) = write_capture_transition(
            Preimage::Initial {
                observation: preimage,
                receipt: None,
                owned_basis: None,
            },
            custody,
        );
        assert!(
            validate_optiscaler_journal_for_cas(
                &serde_json::to_string(&current).expect("invalid capture current json"),
                &serde_json::to_string(&next).expect("invalid capture next json"),
                "preparing",
            )
            .is_err()
        );
    }
}

#[test]
fn prepared_forward_progress_is_rejected_but_reverse_cursor_is_allowed() {
    let current = materialized_applied();
    let mut forward = current.clone();
    if let OperationEffect::Write(effect) = forward.operations_mut()[0].effect_mut() {
        *effect.state_mut() = WriteState::DiscardIntent {
            postimage: DurableObservation::File {
                identity: "live-id".to_owned(),
                digest: hash(),
            },
            custody: DurableObservation::Absent,
        };
    }
    assert!(
        validate_optiscaler_journal_for_cas(
            &serde_json::to_string(&current).expect("json"),
            &serde_json::to_string(&forward).expect("json"),
            "prepared",
        )
        .is_ok()
    );
}

#[test]
fn committed_cleanup_accepts_the_ordinal_artifact_frontier() {
    let mut current = materialized_applied();
    let custody = DurableObservation::File {
        identity: "custody-id".to_owned(),
        digest: hash(),
    };
    if let OperationEffect::Write(effect) = current.operations_mut()[0].effect_mut()
        && let WriteState::Applied {
            custody: effect_custody,
            ..
        } = effect.state_mut()
    {
        *effect_custody = custody.clone();
    }
    *current.operations_mut()[0].slots_mut().custody_mut() = custody.clone();
    let mut next = current.clone();
    next.set_cleanup(renderpilot_domain::CleanupState::ArtifactRemoveIntent {
        operation_id: 0,
        artifact: renderpilot_domain::ArtifactSlot::Custody,
        expected: custody,
    });
    let result = validate_optiscaler_journal_for_cas(
        &serde_json::to_string(&current).expect("current json"),
        &serde_json::to_string(&next).expect("next json"),
        "committed",
    );
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn committed_cleanup_retains_identity_after_unlink_and_finishes_namespace_cleanup() {
    let mut current = materialized_applied();
    let custody = DurableObservation::File {
        identity: "custody-id".to_owned(),
        digest: hash(),
    };
    if let OperationEffect::Write(effect) = current.operations_mut()[0].effect_mut()
        && let WriteState::Applied {
            custody: effect_custody,
            ..
        } = effect.state_mut()
    {
        *effect_custody = custody.clone();
    }
    *current.operations_mut()[0].slots_mut().custody_mut() = custody.clone();

    let mut artifact_intent = current.clone();
    artifact_intent.set_cleanup(renderpilot_domain::CleanupState::ArtifactRemoveIntent {
        operation_id: 0,
        artifact: renderpilot_domain::ArtifactSlot::Custody,
        expected: custody,
    });
    validate_optiscaler_journal_for_cas(
        &serde_json::to_string(&current).expect("current json"),
        &serde_json::to_string(&artifact_intent).expect("intent json"),
        "committed",
    )
    .expect("artifact intent");

    let mut cleared = artifact_intent.clone();
    *cleared.operations_mut()[0].slots_mut().custody_mut() = DurableObservation::Absent;
    cleared.set_cleanup(renderpilot_domain::CleanupState::Inactive);
    validate_optiscaler_journal_for_cas(
        &serde_json::to_string(&artifact_intent).expect("intent json"),
        &serde_json::to_string(&cleared).expect("cleared json"),
        "committed",
    )
    .expect("crash-after-unlink recovery");

    let mut participant_intent = cleared.clone();
    participant_intent.set_cleanup(renderpilot_domain::CleanupState::WorkspaceRemoveIntent {
        workspace_id: 0,
        expected_identity: "participant-id".to_owned(),
    });
    validate_optiscaler_journal_for_cas(
        &serde_json::to_string(&cleared).expect("cleared json"),
        &serde_json::to_string(&participant_intent).expect("participant intent json"),
        "committed",
    )
    .expect("participant intent after artifact unlink");

    let mut control_intent = participant_intent.clone();
    control_intent.set_cleanup(renderpilot_domain::CleanupState::ControlRemoveIntent {
        expected_identity: "control-id".to_owned(),
    });
    validate_optiscaler_journal_for_cas(
        &serde_json::to_string(&participant_intent).expect("participant intent json"),
        &serde_json::to_string(&control_intent).expect("control intent json"),
        "committed",
    )
    .expect("control intent after participant cleanup");

    let mut complete = control_intent.clone();
    complete.set_cleanup(renderpilot_domain::CleanupState::Complete);
    validate_optiscaler_journal_for_cas(
        &serde_json::to_string(&control_intent).expect("control intent json"),
        &serde_json::to_string(&complete).expect("complete json"),
        "committed",
    )
    .expect("control cleanup completion");
}

#[test]
fn workspace_cleanup_uses_reverse_workspace_order_and_skips_postcommit_operations() {
    let inactive = three_operation_cleanup_journal();
    let mut workspace_one = inactive.clone();
    workspace_one.set_cleanup(renderpilot_domain::CleanupState::WorkspaceRemoveIntent {
        workspace_id: 1,
        expected_identity: "participant-1".to_owned(),
    });
    validate_optiscaler_journal_for_cas(
        &serde_json::to_string(&inactive).expect("inactive json"),
        &serde_json::to_string(&workspace_one).expect("workspace one json"),
        "committed",
    )
    .expect("highest workspace cleanup");

    let mut workspace_zero = workspace_one.clone();
    workspace_zero.set_cleanup(renderpilot_domain::CleanupState::WorkspaceRemoveIntent {
        workspace_id: 0,
        expected_identity: "participant-0".to_owned(),
    });
    validate_optiscaler_journal_for_cas(
        &serde_json::to_string(&workspace_one).expect("workspace one json"),
        &serde_json::to_string(&workspace_zero).expect("workspace zero json"),
        "committed",
    )
    .expect("decrement workspace cleanup cursor");

    let mut control = workspace_zero.clone();
    control.set_cleanup(renderpilot_domain::CleanupState::ControlRemoveIntent {
        expected_identity: "control-id".to_owned(),
    });
    validate_optiscaler_journal_for_cas(
        &serde_json::to_string(&workspace_zero).expect("workspace zero json"),
        &serde_json::to_string(&control).expect("control json"),
        "committed",
    )
    .expect("workspace zero to control cleanup");

    let mut complete = control.clone();
    complete.set_cleanup(renderpilot_domain::CleanupState::Complete);
    validate_optiscaler_journal_for_cas(
        &serde_json::to_string(&control).expect("control json"),
        &serde_json::to_string(&complete).expect("complete json"),
        "committed",
    )
    .expect("control cleanup completion");

    let mut workspace_one_inactive = workspace_one.clone();
    workspace_one_inactive.set_cleanup(renderpilot_domain::CleanupState::Inactive);
    assert!(
        validate_optiscaler_journal_for_cas(
            &serde_json::to_string(&workspace_one).expect("workspace one json"),
            &serde_json::to_string(&workspace_one_inactive).expect("inactive json"),
            "committed",
        )
        .is_err(),
        "workspace cursor must not return to inactive"
    );

    let mut workspace_one_repeat = workspace_one.clone();
    workspace_one_repeat.set_cleanup(renderpilot_domain::CleanupState::WorkspaceRemoveIntent {
        workspace_id: 1,
        expected_identity: "participant-1".to_owned(),
    });
    validate_optiscaler_journal_for_cas(
        &serde_json::to_string(&workspace_one).expect("workspace one json"),
        &serde_json::to_string(&workspace_one_repeat).expect("repeat json"),
        "committed",
    )
    .expect("workspace cleanup intent must be idempotent after a torn syscall");

    let mut workspace_one_skip = workspace_one.clone();
    workspace_one_skip.set_cleanup(renderpilot_domain::CleanupState::ControlRemoveIntent {
        expected_identity: "control-id".to_owned(),
    });
    assert!(
        validate_optiscaler_journal_for_cas(
            &serde_json::to_string(&workspace_one).expect("workspace one json"),
            &serde_json::to_string(&workspace_one_skip).expect("skip json"),
            "committed",
        )
        .is_err(),
        "workspace cursor must not skip"
    );

    let mut workspace_zero_increase = workspace_zero.clone();
    workspace_zero_increase.set_cleanup(renderpilot_domain::CleanupState::WorkspaceRemoveIntent {
        workspace_id: 1,
        expected_identity: "participant-1".to_owned(),
    });
    assert!(
        validate_optiscaler_journal_for_cas(
            &serde_json::to_string(&workspace_zero).expect("workspace zero json"),
            &serde_json::to_string(&workspace_zero_increase).expect("increase json"),
            "committed",
        )
        .is_err(),
        "workspace cursor must not increase"
    );

    let mut forged_identity = serde_json::to_value(&workspace_zero).expect("forged value");
    forged_identity["private_workspaces"][1]["identity"] = serde_json::json!("forged-workspace");
    assert!(
        validate_optiscaler_journal_for_cas(
            &serde_json::to_string(&workspace_one).expect("workspace one json"),
            &forged_identity.to_string(),
            "committed",
        )
        .is_err(),
        "workspace binding identity must remain immutable"
    );
}

#[test]
fn materialized_control_identity_cannot_be_replaced_by_cas() {
    let current = materialized_applied();
    let mut next = serde_json::to_value(current.clone()).expect("current value");
    next["control_namespace"]["identity"] = serde_json::json!("forged-control");
    assert!(
        validate_optiscaler_journal_for_cas(
            &serde_json::to_string(&current).expect("current json"),
            &next.to_string(),
            "prepared",
        )
        .is_err()
    );
}
