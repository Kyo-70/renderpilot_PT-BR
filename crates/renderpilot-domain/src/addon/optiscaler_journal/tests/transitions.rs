#[test]
fn workspace_create_intent_keeps_current_identity_absent_until_ready() {
    let mut intent = journal_with_operations(
        vec!["game".to_owned()],
        namespace(),
        vec![operation(), write(1, Some(1), "game/d3d12.dll")],
    )
    .expect("planned journal");
    intent
        .control_namespace_mut()
        .set_identity("control")
        .expect("control identity");
    intent.private_workspaces_mut()[0]
        .set_identity("workspace-0")
        .expect("first workspace identity");
    intent.set_materialization(MaterializationState::WorkspaceCreateIntent { workspace_id: 1 });
    assert!(intent.validate().is_ok());

    intent.private_workspaces_mut()[1]
        .set_identity("workspace-1")
        .expect("second workspace identity");
    intent.set_materialization(MaterializationState::Workspaces {
        next_workspace_id: 2,
    });
    assert!(intent.validate().is_ok());
}

#[test]
fn workspace_create_intent_rejects_current_identity_before_ready_transition() {
    let mut intent = journal_with_operations(
        vec!["game".to_owned()],
        namespace(),
        vec![operation(), write(1, Some(1), "game/d3d12.dll")],
    )
    .expect("planned journal");
    intent
        .control_namespace_mut()
        .set_identity("control")
        .expect("control identity");
    intent.private_workspaces_mut()[0]
        .set_identity("workspace-0")
        .expect("first workspace identity");
    intent.private_workspaces_mut()[1]
        .set_identity("workspace-1")
        .expect("current workspace identity");
    intent.set_materialization(MaterializationState::WorkspaceCreateIntent { workspace_id: 1 });
    assert!(intent.validate().is_err());
}

#[test]
fn action_states_reject_uncertain_or_unrecorded_postimages() {
    let endpoint = planned_endpoint(
        Endpoint::Single,
        "game/dxgi.dll",
        Preimage::Initial {
            observation: DurableObservation::Absent,
            receipt: None,
            owned_basis: None,
        },
    );
    assert!(
        WriteEffect::new(
            endpoint.clone(),
            WriteState::Staged {
                stage: DurableObservation::Unreadable,
            },
        )
        .is_err()
    );
    assert!(
        WriteEffect::new(
            endpoint,
            WriteState::Applied {
                live: DurableObservation::File {
                    identity: "unix:1".to_owned(),
                    digest: hash(),
                },
                custody: DurableObservation::Absent,
            },
        )
        .is_err()
    );

    let known_absent = OperationEndpoint::new(
        Endpoint::Single,
        "game/dxgi.dll",
        Preimage::Initial {
            observation: DurableObservation::Absent,
            receipt: None,
            owned_basis: None,
        },
        ExpectedAfter::Known(DurableObservation::Absent),
    )
    .expect("endpoint");
    assert!(
        WriteEffect::new(
            known_absent,
            WriteState::Applied {
                live: DurableObservation::File {
                    identity: "unix:1".to_owned(),
                    digest: hash(),
                },
                custody: DurableObservation::Absent,
            },
        )
        .is_err()
    );
    assert!(
        OperationEndpoint::new(
            Endpoint::Single,
            "game/dxgi.dll",
            Preimage::Initial {
                observation: DurableObservation::Unreadable,
                receipt: None,
                owned_basis: None,
            },
            ExpectedAfter::Pending,
        )
        .is_err()
    );
}

#[test]
fn captured_write_accepts_only_exact_file_or_absent_custody() {
    let endpoint = planned_endpoint(
        Endpoint::Single,
        "game/dxgi.dll",
        Preimage::Initial {
            observation: DurableObservation::Absent,
            receipt: None,
            owned_basis: None,
        },
    );
    let stage = DurableObservation::File {
        identity: "stage".to_owned(),
        digest: hash(),
    };
    for custody in [
        DurableObservation::Absent,
        DurableObservation::File {
            identity: "custody".to_owned(),
            digest: hash(),
        },
    ] {
        assert!(
            WriteEffect::new(
                endpoint.clone(),
                WriteState::Captured {
                    stage: stage.clone(),
                    custody,
                },
            )
            .is_ok()
        );
    }
    for custody in [
        DurableObservation::Directory {
            identity: "directory".to_owned(),
        },
        DurableObservation::NonRegular,
        DurableObservation::Unreadable,
    ] {
        assert!(
            WriteEffect::new(
                endpoint.clone(),
                WriteState::Captured {
                    stage: stage.clone(),
                    custody,
                },
            )
            .is_err()
        );
    }

    let mut capture_intent = operation();
    if let OperationEffect::Write(effect) = capture_intent.effect_mut() {
        *effect.state_mut() = WriteState::CaptureIntent {
            stage: stage.clone(),
        };
    }
    *capture_intent.slots_mut().stage_mut() = stage.clone();
    for custody in [
        DurableObservation::Absent,
        DurableObservation::File {
            identity: "captured".to_owned(),
            digest: hash(),
        },
    ] {
        let mut captured = capture_intent.clone();
        if let OperationEffect::Write(effect) = captured.effect_mut() {
            *effect.state_mut() = WriteState::Captured {
                stage: stage.clone(),
                custody: custody.clone(),
            };
        }
        *captured.slots_mut().stage_mut() = stage.clone();
        *captured.slots_mut().custody_mut() = custody;
        assert_eq!(
            validate_optiscaler_operation_transition(&capture_intent, &captured),
            Ok(OptiScalerTransitionDirection::Forward)
        );
    }
    let mut wrong_custody = capture_intent.clone();
    if let OperationEffect::Write(effect) = wrong_custody.effect_mut() {
        *effect.state_mut() = WriteState::Captured {
            stage: stage.clone(),
            custody: DurableObservation::Directory {
                identity: "wrong".to_owned(),
            },
        };
    }
    *wrong_custody.slots_mut().stage_mut() = stage;
    *wrong_custody.slots_mut().custody_mut() = DurableObservation::Directory {
        identity: "wrong".to_owned(),
    };
    assert!(validate_optiscaler_operation_transition(&capture_intent, &wrong_custody).is_err());
}

#[test]
fn transition_authority_covers_partial_adoption_and_rejects_state_skips() {
    let mut stage_intent = operation();
    if let OperationEffect::Write(effect) = stage_intent.effect_mut() {
        *effect.state_mut() = WriteState::StageIntent {
            target_digest: hash(),
        };
    }
    let mut preserved = stage_intent.clone();
    if let OperationEffect::Write(effect) = preserved.effect_mut() {
        *effect.state_mut() = WriteState::Preserved;
    }
    *preserved.slots_mut().stage_mut() = DurableObservation::File {
        identity: "adopted-stage".to_owned(),
        digest: hash(),
    };
    assert_eq!(
        validate_optiscaler_operation_transition(&stage_intent, &preserved),
        Ok(OptiScalerTransitionDirection::Reverse)
    );

    let mut skipped = stage_intent.clone();
    if let OperationEffect::Write(effect) = skipped.effect_mut() {
        *effect.state_mut() = WriteState::Applied {
            live: DurableObservation::File {
                identity: "live".to_owned(),
                digest: hash(),
            },
            custody: DurableObservation::Absent,
        };
    }
    assert!(validate_optiscaler_operation_transition(&stage_intent, &skipped).is_err());
}

#[test]
fn transition_authority_rejects_changed_stage_custody_and_directory_identity() {
    let stage = DurableObservation::File {
        identity: "stage".to_owned(),
        digest: hash(),
    };
    let changed_stage = DurableObservation::File {
        identity: "changed-stage".to_owned(),
        digest: hash(),
    };
    let custody = DurableObservation::File {
        identity: "custody".to_owned(),
        digest: hash(),
    };
    let changed_custody = DurableObservation::File {
        identity: "changed-custody".to_owned(),
        digest: hash(),
    };

    let mut staged = operation();
    if let OperationEffect::Write(effect) = staged.effect_mut() {
        *effect.state_mut() = WriteState::Staged {
            stage: stage.clone(),
        };
    }
    *staged.slots_mut().stage_mut() = stage.clone();
    let mut changed_capture = staged.clone();
    if let OperationEffect::Write(effect) = changed_capture.effect_mut() {
        *effect.state_mut() = WriteState::CaptureIntent {
            stage: changed_stage.clone(),
        };
    }
    *changed_capture.slots_mut().stage_mut() = changed_stage;
    assert!(validate_optiscaler_operation_transition(&staged, &changed_capture).is_err());

    let mut captured = operation();
    if let OperationEffect::Write(effect) = captured.effect_mut() {
        *effect.state_mut() = WriteState::Captured {
            stage: stage.clone(),
            custody: custody.clone(),
        };
    }
    *captured.slots_mut().stage_mut() = stage.clone();
    *captured.slots_mut().custody_mut() = custody;
    let mut changed_publish = captured.clone();
    if let OperationEffect::Write(effect) = changed_publish.effect_mut() {
        *effect.state_mut() = WriteState::PublishIntent {
            stage: stage.clone(),
            custody: changed_custody.clone(),
        };
    }
    *changed_publish.slots_mut().stage_mut() = stage;
    *changed_publish.slots_mut().custody_mut() = changed_custody;
    assert!(validate_optiscaler_operation_transition(&captured, &changed_publish).is_err());

    let directory = DurableObservation::Directory {
        identity: "directory".to_owned(),
    };
    let changed_directory = DurableObservation::Directory {
        identity: "changed-directory".to_owned(),
    };
    let endpoint = OperationEndpoint::new(
        Endpoint::Single,
        "game/private-directory",
        Preimage::Initial {
            observation: DurableObservation::Absent,
            receipt: None,
            owned_basis: None,
        },
        ExpectedAfter::Pending,
    )
    .expect("directory endpoint");
    let mut staged_directory = OperationRecord::new(
        0,
        Vec::new(),
        Some(0),
        PrivateArtifactSlots::new(
            DurableObservation::Absent,
            directory.clone(),
            DurableObservation::Absent,
        )
        .expect("directory slots"),
        OperationEffect::CreateDirectory(CreateDirectoryEffect {
            endpoint,
            state: CreateDirectoryState::Staged {
                stage: directory.clone(),
            },
        }),
    )
    .expect("staged directory");
    let mut published_directory = staged_directory.clone();
    if let OperationEffect::CreateDirectory(effect) = staged_directory.effect_mut() {
        *effect.state_mut() = CreateDirectoryState::PublishIntent { stage: directory };
    }
    if let OperationEffect::CreateDirectory(effect) = published_directory.effect_mut() {
        *effect.state_mut() = CreateDirectoryState::Applied {
            live: changed_directory,
        };
    }
    *published_directory.slots_mut() = slots();
    assert!(
        validate_optiscaler_operation_transition(&staged_directory, &published_directory).is_err()
    );
}
