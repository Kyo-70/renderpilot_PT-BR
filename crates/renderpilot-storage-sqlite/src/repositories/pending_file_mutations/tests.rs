use renderpilot_domain::{
    CleanupState, ControlNamespaceBinding, CreateDirectoryEffect, CreateDirectoryState,
    DurableObservation, Endpoint, ExpectedAfter, MaterializationState, NamespaceCapability,
    OperationEffect, OperationEndpoint, OperationRecord, OptiScalerJournal, Preimage,
    PrivateArtifactSlots, PrivateWorkspaceBinding, RemoveDirectoryEffect, RemoveDirectoryState,
    Sha256Hash, WriteEffect, WriteState,
};

use super::commit::{
    validate_optiscaler_journal_for_begin, validate_optiscaler_journal_for_cas,
    validate_optiscaler_journal_for_prepared,
};

fn capability() -> NamespaceCapability {
    NamespaceCapability::new("a".repeat(64)).expect("capability")
}

fn digest(hex: char) -> Sha256Hash {
    Sha256Hash::new(hex.to_string().repeat(64)).expect("digest")
}

fn journal() -> OptiScalerJournal {
    let capability = capability();
    let endpoint = OperationEndpoint::new(
        Endpoint::Single,
        "game/dxgi.dll",
        Preimage::Initial {
            observation: DurableObservation::Absent,
            receipt: None,
            owned_basis: None,
        },
        ExpectedAfter::Pending,
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
            WriteEffect::new(endpoint, WriteState::Planned).expect("write effect"),
        ),
    )
    .expect("operation");
    OptiScalerJournal::new(
        vec!["game".to_owned()],
        ControlNamespaceBinding::new(
            format!("game/control-abc-{}", capability.as_str()),
            None,
            capability.clone(),
        )
        .expect("control namespace"),
        vec![
            PrivateWorkspaceBinding::new(
                0,
                0,
                format!(
                    "game/.renderpilot-optiscaler-workspace-abc-0-{}",
                    capability.as_str()
                ),
                None,
                capability,
            )
            .expect("workspace"),
        ],
        vec![operation],
    )
    .expect("journal")
}

fn rollback_terminal_journal() -> OptiScalerJournal {
    let mut journal = journal();
    if let OperationEffect::Write(effect) = journal.operations_mut()[0].effect_mut() {
        *effect.state_mut() = WriteState::Preserved;
    }
    journal
        .control_namespace_mut()
        .set_identity("control-identity")
        .expect("control identity");
    journal.private_workspaces_mut()[0]
        .set_identity("participant-identity")
        .expect("workspace identity");
    journal.set_materialization(MaterializationState::Ready);
    journal.set_cleanup(CleanupState::Complete);
    journal
}

fn created_directory_journal() -> OptiScalerJournal {
    let mut journal = journal();
    let directory = DurableObservation::Directory {
        identity: "directory-after".to_owned(),
    };
    let endpoint = OperationEndpoint::new(
        Endpoint::Single,
        "game/created",
        Preimage::Initial {
            observation: DurableObservation::Absent,
            receipt: None,
            owned_basis: None,
        },
        ExpectedAfter::Known(directory.clone()),
    )
    .expect("directory endpoint");
    *journal.operations_mut()[0].effect_mut() = OperationEffect::CreateDirectory(
        CreateDirectoryEffect::new(endpoint, CreateDirectoryState::Applied { live: directory })
            .expect("directory effect"),
    );
    journal
}

#[test]
fn begin_accepts_only_the_unmaterialized_shared_journal() {
    let json = serde_json::to_string(&journal()).expect("json");
    validate_optiscaler_journal_for_begin(&json).expect("begin journal");
    let mut value = serde_json::from_str::<serde_json::Value>(&json).expect("value");
    value["unexpected"] = serde_json::json!(true);
    assert!(validate_optiscaler_journal_for_begin(&value.to_string()).is_err());
}

#[test]
fn prepared_validation_requires_materialized_applied_program() {
    let mut journal = journal();
    journal
        .control_namespace_mut()
        .set_identity("control")
        .expect("control identity");
    journal.private_workspaces_mut()[0]
        .set_identity("participant")
        .expect("workspace identity");
    journal.set_materialization(MaterializationState::Ready);
    assert!(
        validate_optiscaler_journal_for_prepared(&serde_json::to_string(&journal).expect("json"))
            .is_err()
    );
}

#[test]
fn prepared_cas_rejects_json_without_a_legal_state_edge() {
    let current = journal();
    let next = current.clone();
    assert!(
        validate_optiscaler_journal_for_cas(
            &serde_json::to_string(&current).expect("json"),
            &serde_json::to_string(&next).expect("json"),
            "prepared",
        )
        .is_err()
    );
}

#[test]
fn rollback_abort_adopts_only_authorized_exact_write_slots() {
    let mut stage_intent = journal();
    if let OperationEffect::Write(effect) = stage_intent.operations_mut()[0].effect_mut() {
        *effect.state_mut() = WriteState::StageIntent {
            target_digest: digest('a'),
        };
    }
    let mut stage_preserved = stage_intent.clone();
    if let OperationEffect::Write(effect) = stage_preserved.operations_mut()[0].effect_mut() {
        *effect.state_mut() = WriteState::Preserved;
    }
    *stage_preserved.operations_mut()[0].slots_mut().stage_mut() = DurableObservation::File {
        identity: "partial-stage".to_owned(),
        digest: digest('b'),
    };
    validate_optiscaler_journal_for_cas(
        &serde_json::to_string(&stage_intent).expect("stage intent json"),
        &serde_json::to_string(&stage_preserved).expect("stage preserved json"),
        "preparing",
    )
    .expect("stage abort may retain a third digest");

    let mut capture_intent = journal();
    if let OperationEffect::Write(effect) = capture_intent.operations_mut()[0].effect_mut() {
        *effect.state_mut() = WriteState::CaptureIntent {
            stage: DurableObservation::File {
                identity: "recorded-stage".to_owned(),
                digest: digest('a'),
            },
        };
    }
    *capture_intent.operations_mut()[0].slots_mut().stage_mut() = DurableObservation::File {
        identity: "recorded-stage".to_owned(),
        digest: digest('a'),
    };
    let mut capture_preserved = capture_intent.clone();
    if let OperationEffect::Write(effect) = capture_preserved.operations_mut()[0].effect_mut() {
        *effect.state_mut() = WriteState::Preserved;
    }
    *capture_preserved.operations_mut()[0]
        .slots_mut()
        .custody_mut() = DurableObservation::File {
        identity: "partial-custody".to_owned(),
        digest: digest('c'),
    };
    validate_optiscaler_journal_for_cas(
        &serde_json::to_string(&capture_intent).expect("capture intent json"),
        &serde_json::to_string(&capture_preserved).expect("capture preserved json"),
        "preparing",
    )
    .expect("capture abort may retain a partial exact custody file");
}

#[test]
fn rollback_abort_rejects_wrong_types_and_unrelated_slots() {
    let mut current = journal();
    if let OperationEffect::Write(effect) = current.operations_mut()[0].effect_mut() {
        *effect.state_mut() = WriteState::StageIntent {
            target_digest: digest('a'),
        };
    }
    let mut wrong_type = current.clone();
    if let OperationEffect::Write(effect) = wrong_type.operations_mut()[0].effect_mut() {
        *effect.state_mut() = WriteState::Preserved;
    }
    *wrong_type.operations_mut()[0].slots_mut().stage_mut() = DurableObservation::Directory {
        identity: "not-a-file".to_owned(),
    };
    assert!(
        validate_optiscaler_journal_for_cas(
            &serde_json::to_string(&current).expect("current json"),
            &serde_json::to_string(&wrong_type).expect("wrong type json"),
            "preparing",
        )
        .is_err()
    );

    let mut unrelated = current.clone();
    if let OperationEffect::Write(effect) = unrelated.operations_mut()[0].effect_mut() {
        *effect.state_mut() = WriteState::Preserved;
    }
    *unrelated.operations_mut()[0].slots_mut().custody_mut() = DurableObservation::File {
        identity: "unrelated".to_owned(),
        digest: digest('d'),
    };
    assert!(
        validate_optiscaler_journal_for_cas(
            &serde_json::to_string(&current).expect("current json"),
            &serde_json::to_string(&unrelated).expect("unrelated json"),
            "preparing",
        )
        .is_err()
    );
}

#[test]
fn rollback_abort_adopts_one_exact_create_directory_stage_only() {
    let mut current = journal();
    let endpoint = OperationEndpoint::new(
        Endpoint::Single,
        "game/private-dir",
        Preimage::Initial {
            observation: DurableObservation::Absent,
            receipt: None,
            owned_basis: None,
        },
        ExpectedAfter::Pending,
    )
    .expect("directory endpoint");
    *current.operations_mut()[0].effect_mut() = OperationEffect::CreateDirectory(
        CreateDirectoryEffect::new(endpoint, CreateDirectoryState::StageIntent)
            .expect("directory stage intent"),
    );

    let mut preserved = current.clone();
    if let OperationEffect::CreateDirectory(effect) = preserved.operations_mut()[0].effect_mut() {
        *effect.state_mut() = CreateDirectoryState::Preserved;
    }
    *preserved.operations_mut()[0].slots_mut().stage_mut() = DurableObservation::Directory {
        identity: "partial-directory-stage".to_owned(),
    };
    validate_optiscaler_journal_for_cas(
        &serde_json::to_string(&current).expect("stage intent json"),
        &serde_json::to_string(&preserved).expect("preserved json"),
        "preparing",
    )
    .expect("rollback may adopt one exact directory stage");

    let mut wrong_type = preserved.clone();
    *wrong_type.operations_mut()[0].slots_mut().stage_mut() = DurableObservation::File {
        identity: "not-a-directory".to_owned(),
        digest: digest('a'),
    };
    assert!(
        validate_optiscaler_journal_for_cas(
            &serde_json::to_string(&current).expect("stage intent json"),
            &serde_json::to_string(&wrong_type).expect("wrong type json"),
            "preparing",
        )
        .is_err()
    );

    let mut custody = preserved;
    *custody.operations_mut()[0].slots_mut().custody_mut() = DurableObservation::Directory {
        identity: "unauthorized-custody".to_owned(),
    };
    assert!(
        validate_optiscaler_journal_for_cas(
            &serde_json::to_string(&current).expect("stage intent json"),
            &serde_json::to_string(&custody).expect("custody json"),
            "preparing",
        )
        .is_err()
    );
}

#[test]
fn post_commit_directory_removal_has_only_exact_intent_and_applied_edges() {
    let directory = DurableObservation::Directory {
        identity: "post-commit-directory".to_owned(),
    };
    let capability = capability();
    let endpoint = OperationEndpoint::new(
        Endpoint::Single,
        "game/private-dir",
        Preimage::Initial {
            observation: directory.clone(),
            receipt: None,
            owned_basis: None,
        },
        ExpectedAfter::Pending,
    )
    .expect("directory endpoint");
    let operation = OperationRecord::new(
        0,
        Vec::new(),
        None,
        PrivateArtifactSlots::new(
            DurableObservation::Absent,
            DurableObservation::Absent,
            DurableObservation::Absent,
        )
        .expect("slots"),
        OperationEffect::PostCommitRemoveDirectory(
            RemoveDirectoryEffect::new(
                endpoint,
                RemoveDirectoryState::Planned {
                    directory: directory.clone(),
                },
            )
            .expect("planned directory removal"),
        ),
    )
    .expect("postcommit operation");
    let mut planned = OptiScalerJournal::new(
        vec!["game".to_owned()],
        ControlNamespaceBinding::new(
            format!("game/control-postcommit-{}", capability.as_str()),
            None,
            capability,
        )
        .expect("control namespace"),
        Vec::new(),
        vec![operation],
    )
    .expect("postcommit journal");
    planned
        .control_namespace_mut()
        .set_identity("control-identity")
        .expect("control identity");
    planned.set_materialization(MaterializationState::Ready);

    let mut intent = planned.clone();
    if let OperationEffect::PostCommitRemoveDirectory(effect) =
        intent.operations_mut()[0].effect_mut()
    {
        *effect.state_mut() = RemoveDirectoryState::RemoveIntent {
            directory: directory.clone(),
            live: directory.clone(),
        };
    }
    validate_optiscaler_journal_for_cas(
        &serde_json::to_string(&planned).expect("planned json"),
        &serde_json::to_string(&intent).expect("intent json"),
        "committed",
    )
    .expect("remove intent edge");

    let mut applied = intent.clone();
    if let OperationEffect::PostCommitRemoveDirectory(effect) =
        applied.operations_mut()[0].effect_mut()
    {
        effect
            .endpoint_mut()
            .set_expected_after(ExpectedAfter::Known(DurableObservation::Absent));
        *effect.state_mut() = RemoveDirectoryState::Applied { directory };
    }
    validate_optiscaler_journal_for_cas(
        &serde_json::to_string(&intent).expect("intent json"),
        &serde_json::to_string(&applied).expect("applied json"),
        "committed",
    )
    .expect("remove applied edge");
}

#[test]
fn directory_discard_states_never_use_the_private_discard_slot() {
    let current = created_directory_journal();
    let mut discard_intent = current.clone();
    if let OperationEffect::CreateDirectory(effect) =
        discard_intent.operations_mut()[0].effect_mut()
    {
        *effect.state_mut() = CreateDirectoryState::DiscardIntent {
            directory: DurableObservation::Directory {
                identity: "directory-after".to_owned(),
            },
        };
    }
    validate_optiscaler_journal_for_cas(
        &serde_json::to_string(&current).expect("applied json"),
        &serde_json::to_string(&discard_intent).expect("discard intent json"),
        "preparing",
    )
    .expect("directory discard intent without a private discard slot");

    let mut occupied = discard_intent.clone();
    *occupied.operations_mut()[0].slots_mut().discard_mut() = DurableObservation::Directory {
        identity: "must-not-be-recorded".to_owned(),
    };
    assert!(
        validate_optiscaler_journal_for_cas(
            &serde_json::to_string(&current).expect("applied json"),
            &serde_json::to_string(&occupied).expect("occupied discard json"),
            "preparing",
        )
        .is_err()
    );

    let mut discarded = discard_intent.clone();
    if let OperationEffect::CreateDirectory(effect) = discarded.operations_mut()[0].effect_mut() {
        *effect.state_mut() = CreateDirectoryState::PostimageDiscarded {
            discard: DurableObservation::Directory {
                identity: "directory-after".to_owned(),
            },
        };
    }
    validate_optiscaler_journal_for_cas(
        &serde_json::to_string(&discard_intent).expect("discard intent json"),
        &serde_json::to_string(&discarded).expect("postimage discarded json"),
        "preparing",
    )
    .expect("historical directory discard token does not occupy a slot");
}

#[test]
fn applied_verify_can_reverse_with_unchanged_exact_observation() {
    let observed = DurableObservation::File {
        identity: "verify".to_owned(),
        digest: digest('e'),
    };
    let endpoint = OperationEndpoint::new(
        Endpoint::Single,
        "game/dxgi.dll",
        Preimage::Initial {
            observation: observed.clone(),
            receipt: None,
            owned_basis: None,
        },
        ExpectedAfter::Known(observed.clone()),
    )
    .expect("verify endpoint");
    let operation = OperationRecord::new(
        0,
        Vec::new(),
        None,
        PrivateArtifactSlots::new(
            DurableObservation::Absent,
            DurableObservation::Absent,
            DurableObservation::Absent,
        )
        .expect("slots"),
        OperationEffect::Verify(
            renderpilot_domain::VerifyEffect::new(
                endpoint,
                renderpilot_domain::VerifyState::Applied { observed },
            )
            .expect("verify effect"),
        ),
    )
    .expect("operation");
    let current = OptiScalerJournal::new(
        vec!["game".to_owned()],
        ControlNamespaceBinding::new(
            format!("game/control-abc-{}", capability().as_str()),
            None,
            capability(),
        )
        .expect("control namespace"),
        Vec::new(),
        vec![operation],
    )
    .expect("journal");
    let mut next = current.clone();
    if let OperationEffect::Verify(effect) = next.operations_mut()[0].effect_mut() {
        *effect.state_mut() = renderpilot_domain::VerifyState::Preserved;
    }
    validate_optiscaler_journal_for_cas(
        &serde_json::to_string(&current).expect("current verify json"),
        &serde_json::to_string(&next).expect("next verify json"),
        "preparing",
    )
    .expect("verify reverse edge");
}

#[test]
fn rollback_cleanup_is_gated_and_monotonic_for_preparing_and_prepared_rows() {
    for row_state in ["preparing", "prepared"] {
        let mut current = rollback_terminal_journal();
        current.set_cleanup(CleanupState::Inactive);
        *current.operations_mut()[0].slots_mut().stage_mut() = DurableObservation::File {
            identity: "stage-cleanup".to_owned(),
            digest: digest('f'),
        };
        let mut next = current.clone();
        next.set_cleanup(CleanupState::ArtifactRemoveIntent {
            operation_id: 0,
            artifact: renderpilot_domain::ArtifactSlot::Stage,
            expected: DurableObservation::File {
                identity: "stage-cleanup".to_owned(),
                digest: digest('f'),
            },
        });
        validate_optiscaler_journal_for_cas(
            &serde_json::to_string(&current).expect("inactive json"),
            &serde_json::to_string(&next).expect("artifact intent json"),
            row_state,
        )
        .expect("artifact cleanup intent");

        let mut cleared = next.clone();
        cleared.set_cleanup(CleanupState::Inactive);
        *cleared.operations_mut()[0].slots_mut().stage_mut() = DurableObservation::Absent;
        validate_optiscaler_journal_for_cas(
            &serde_json::to_string(&next).expect("artifact intent json"),
            &serde_json::to_string(&cleared).expect("artifact clear json"),
            row_state,
        )
        .expect("artifact cleanup clear");

        let mut workspace = cleared.clone();
        workspace.set_cleanup(CleanupState::WorkspaceRemoveIntent {
            workspace_id: 0,
            expected_identity: "participant-identity".to_owned(),
        });
        validate_optiscaler_journal_for_cas(
            &serde_json::to_string(&cleared).expect("inactive json"),
            &serde_json::to_string(&workspace).expect("workspace intent json"),
            row_state,
        )
        .expect("workspace cleanup starts at max workspace id");

        let mut control = workspace.clone();
        control.set_cleanup(CleanupState::ControlRemoveIntent {
            expected_identity: "control-identity".to_owned(),
        });
        validate_optiscaler_journal_for_cas(
            &serde_json::to_string(&workspace).expect("workspace intent json"),
            &serde_json::to_string(&control).expect("control intent json"),
            row_state,
        )
        .expect("control cleanup follows workspace zero");

        let mut complete = control.clone();
        complete.set_cleanup(CleanupState::Complete);
        validate_optiscaler_journal_for_cas(
            &serde_json::to_string(&control).expect("control intent json"),
            &serde_json::to_string(&complete).expect("complete json"),
            row_state,
        )
        .expect("cleanup complete");

        let current_json = serde_json::to_string(&current).expect("inactive cleanup json");
        let mut premature = current;
        premature.set_cleanup(CleanupState::ArtifactRemoveIntent {
            operation_id: 0,
            artifact: renderpilot_domain::ArtifactSlot::Stage,
            expected: DurableObservation::File {
                identity: "wrong".to_owned(),
                digest: digest('f'),
            },
        });
        assert!(
            validate_optiscaler_journal_for_cas(
                &current_json,
                &serde_json::to_string(&premature).expect("premature json"),
                row_state,
            )
            .is_err()
        );
    }
}
