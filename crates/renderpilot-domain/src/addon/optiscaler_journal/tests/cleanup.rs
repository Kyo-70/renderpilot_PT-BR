#[test]
fn relocate_pending_to_known_applied_preserves_two_endpoint_identity() {
    let source = planned_endpoint(
        Endpoint::Source,
        "game/source.dll",
        Preimage::Initial {
            observation: DurableObservation::Absent,
            receipt: None,
            owned_basis: None,
        },
    );
    let destination = planned_endpoint(
        Endpoint::Destination,
        "game/destination.dll",
        Preimage::Initial {
            observation: DurableObservation::Absent,
            receipt: None,
            owned_basis: None,
        },
    );
    let current = OperationRecord::new(
        0,
        Vec::new(),
        None,
        slots(),
        OperationEffect::Relocate(Box::new(
            RelocateEffect::new(source, destination, RelocateState::MoveIntent)
                .expect("planned relocation"),
        )),
    )
    .expect("planned operation");
    let destination_after = DurableObservation::File {
        identity: "destination-live".to_owned(),
        digest: hash(),
    };
    let mut applied = current.clone();
    if let OperationEffect::Relocate(effect) = applied.effect_mut() {
        effect
            .source_mut()
            .set_expected_after(ExpectedAfter::Known(DurableObservation::Absent));
        effect
            .destination_mut()
            .set_expected_after(ExpectedAfter::Known(destination_after.clone()));
        *effect.state_mut() = RelocateState::Applied {
            source_after: DurableObservation::Absent,
            destination_after,
        };
    }
    assert_eq!(
        validate_optiscaler_effect_transition(current.effect(), applied.effect()),
        Ok(OptiScalerTransitionDirection::Forward)
    );
    assert_eq!(
        validate_optiscaler_operation_transition(&current, &applied),
        Ok(OptiScalerTransitionDirection::Forward)
    );

    let mut changed_path = applied.clone();
    if let OperationEffect::Relocate(effect) = changed_path.effect_mut() {
        *effect.source_mut() = OperationEndpoint::new(
            Endpoint::Source,
            "game/changed-source.dll",
            Preimage::Initial {
                observation: DurableObservation::Absent,
                receipt: None,
                owned_basis: None,
            },
            ExpectedAfter::Known(DurableObservation::Absent),
        )
        .expect("changed source endpoint");
    }
    assert!(
        validate_optiscaler_effect_transition(current.effect(), changed_path.effect()).is_err()
    );

    let mut changed_preimage = applied;
    if let OperationEffect::Relocate(effect) = changed_preimage.effect_mut() {
        *effect.destination_mut() = OperationEndpoint::new(
            Endpoint::Destination,
            "game/destination.dll",
            Preimage::Initial {
                observation: DurableObservation::File {
                    identity: "changed-preimage".to_owned(),
                    digest: hash(),
                },
                receipt: None,
                owned_basis: None,
            },
            ExpectedAfter::Known(DurableObservation::File {
                identity: "destination-live".to_owned(),
                digest: hash(),
            }),
        )
        .expect("changed destination preimage");
    }
    assert!(
        validate_optiscaler_effect_transition(current.effect(), changed_preimage.effect()).is_err()
    );
}

#[test]
fn action_slot_matrix_rejects_wrong_steady_state_and_accepts_exact_cleanup_shapes() {
    let endpoint = OperationEndpoint::new(
        Endpoint::Single,
        "game/dxgi.dll",
        Preimage::Initial {
            observation: DurableObservation::Absent,
            receipt: None,
            owned_basis: None,
        },
        ExpectedAfter::Known(DurableObservation::File {
            identity: "postimage".to_owned(),
            digest: hash(),
        }),
    )
    .expect("postimage endpoint");
    let postimage = DurableObservation::File {
        identity: "postimage".to_owned(),
        digest: hash(),
    };
    let custody = DurableObservation::File {
        identity: "custody".to_owned(),
        digest: hash(),
    };
    assert!(
        WriteEffect::new(
            endpoint.clone(),
            WriteState::DiscardIntent {
                postimage: postimage.clone(),
                custody: custody.clone(),
            },
        )
        .is_err()
    );
    assert!(
        OperationRecord::new(
            0,
            Vec::new(),
            Some(0),
            slots(),
            OperationEffect::Write(WriteEffect {
                endpoint: endpoint.clone(),
                state: WriteState::DiscardIntent {
                    postimage: postimage.clone(),
                    custody: DurableObservation::Absent,
                },
            }),
        )
        .is_ok()
    );

    assert!(
        OperationRecord::new(
            0,
            Vec::new(),
            Some(0),
            slots(),
            OperationEffect::Delete(DeleteEffect {
                endpoint: endpoint.clone(),
                state: DeleteState::RestoreIntent {
                    preimage: postimage.clone(),
                },
            }),
        )
        .is_err()
    );

    let delete_endpoint = OperationEndpoint::new(
        Endpoint::Single,
        "game/delete.dll",
        Preimage::Initial {
            observation: DurableObservation::File {
                identity: "before".to_owned(),
                digest: hash(),
            },
            receipt: Some(FileReceipt::owned("before", hash()).expect("receipt")),
            owned_basis: None,
        },
        ExpectedAfter::Known(DurableObservation::Absent),
    )
    .expect("delete endpoint");
    assert!(
        OperationRecord::new(
            0,
            Vec::new(),
            Some(0),
            PrivateArtifactSlots::new(
                custody.clone(),
                DurableObservation::Absent,
                DurableObservation::Absent,
            )
            .expect("delete restore slots"),
            OperationEffect::Delete(DeleteEffect {
                endpoint: delete_endpoint,
                state: DeleteState::RestoreIntent { preimage: custody },
            }),
        )
        .is_ok()
    );

    let wrong_directory_slots = PrivateArtifactSlots::new(
        DurableObservation::Absent,
        postimage,
        DurableObservation::Absent,
    )
    .expect("exact file slots");
    let directory_endpoint = OperationEndpoint::new(
        Endpoint::Single,
        "game/directory",
        Preimage::Initial {
            observation: DurableObservation::Absent,
            receipt: None,
            owned_basis: None,
        },
        ExpectedAfter::Pending,
    )
    .expect("directory endpoint");
    assert!(
        OperationRecord::new(
            0,
            Vec::new(),
            Some(0),
            PrivateArtifactSlots::new(
                DurableObservation::Absent,
                DurableObservation::Directory {
                    identity: "preserved-directory".to_owned(),
                },
                DurableObservation::Absent,
            )
            .expect("directory preserved slots"),
            OperationEffect::CreateDirectory(CreateDirectoryEffect {
                endpoint: directory_endpoint,
                state: CreateDirectoryState::Preserved,
            }),
        )
        .is_ok()
    );
    assert!(
        OperationRecord::new(
            0,
            Vec::new(),
            Some(0),
            wrong_directory_slots,
            OperationEffect::CreateDirectory(CreateDirectoryEffect {
                endpoint,
                state: CreateDirectoryState::Preserved,
            }),
        )
        .is_err()
    );
}

#[test]
fn cleanup_artifact_authority_is_exhaustive_for_both_lifecycle_modes() {
    let file = DurableObservation::File {
        identity: "file".to_owned(),
        digest: hash(),
    };
    let directory = DurableObservation::Directory {
        identity: "directory".to_owned(),
    };
    let endpoint = OperationEndpoint::new(
        Endpoint::Single,
        "game/cleanup-matrix",
        Preimage::Initial {
            observation: DurableObservation::Absent,
            receipt: None,
            owned_basis: None,
        },
        ExpectedAfter::Pending,
    )
    .expect("cleanup endpoint");
    let write_states = [
        WriteState::Planned,
        WriteState::StageIntent {
            target_digest: hash(),
        },
        WriteState::Staged {
            stage: file.clone(),
        },
        WriteState::CaptureIntent {
            stage: file.clone(),
        },
        WriteState::Captured {
            stage: file.clone(),
            custody: file.clone(),
        },
        WriteState::PublishIntent {
            stage: file.clone(),
            custody: file.clone(),
        },
        WriteState::Applied {
            live: file.clone(),
            custody: file.clone(),
        },
        WriteState::DiscardIntent {
            postimage: file.clone(),
            custody: DurableObservation::Absent,
        },
        WriteState::PostimageDiscarded {
            discard: file.clone(),
            custody: DurableObservation::Absent,
        },
        WriteState::RestoreIntent {
            preimage: file.clone(),
            discard: file.clone(),
        },
        WriteState::Preserved,
    ];
    let delete_states = [
        DeleteState::Planned,
        DeleteState::CaptureIntent,
        DeleteState::Captured {
            custody: file.clone(),
        },
        DeleteState::Applied {
            custody: file.clone(),
        },
        DeleteState::RestoreIntent {
            preimage: file.clone(),
        },
        DeleteState::Preserved,
    ];
    let verify_states = [
        VerifyState::Planned,
        VerifyState::Applied {
            observed: file.clone(),
        },
        VerifyState::Preserved,
    ];
    let relocate_states = [
        RelocateState::Planned,
        RelocateState::MoveIntent,
        RelocateState::Applied {
            source_after: DurableObservation::Absent,
            destination_after: file,
        },
        RelocateState::ReverseIntent,
        RelocateState::Preserved,
    ];
    let directory_states = [
        CreateDirectoryState::Planned,
        CreateDirectoryState::StageIntent,
        CreateDirectoryState::Staged {
            stage: directory.clone(),
        },
        CreateDirectoryState::PublishIntent {
            stage: directory.clone(),
        },
        CreateDirectoryState::Applied {
            live: directory.clone(),
        },
        CreateDirectoryState::DiscardIntent {
            directory: directory.clone(),
        },
        CreateDirectoryState::PostimageDiscarded {
            discard: directory.clone(),
        },
        CreateDirectoryState::Preserved,
    ];
    let remove_states = [
        RemoveDirectoryState::Planned {
            directory: directory.clone(),
        },
        RemoveDirectoryState::RemoveIntent {
            directory: directory.clone(),
            live: directory.clone(),
        },
        RemoveDirectoryState::Applied { directory },
    ];
    let effects = write_states
        .into_iter()
        .map(|state| {
            OperationEffect::Write(WriteEffect {
                endpoint: endpoint.clone(),
                state,
            })
        })
        .chain(delete_states.into_iter().map(|state| {
            OperationEffect::Delete(DeleteEffect {
                endpoint: endpoint.clone(),
                state,
            })
        }))
        .chain(verify_states.into_iter().map(|state| {
            OperationEffect::Verify(VerifyEffect {
                endpoint: endpoint.clone(),
                state,
            })
        }))
        .chain(relocate_states.into_iter().map(|state| {
            OperationEffect::Relocate(Box::new(RelocateEffect {
                source: endpoint.clone(),
                destination: OperationEndpoint::new(
                    Endpoint::Destination,
                    "game/cleanup-destination",
                    Preimage::Initial {
                        observation: DurableObservation::Absent,
                        receipt: None,
                        owned_basis: None,
                    },
                    ExpectedAfter::Pending,
                )
                .expect("relocation destination"),
                state,
            }))
        }))
        .chain(directory_states.into_iter().map(|state| {
            OperationEffect::CreateDirectory(CreateDirectoryEffect {
                endpoint: endpoint.clone(),
                state,
            })
        }))
        .chain(remove_states.into_iter().map(|state| {
            OperationEffect::PostCommitRemoveDirectory(RemoveDirectoryEffect {
                endpoint: endpoint.clone(),
                state,
            })
        }))
        .collect::<Vec<_>>();

    for effect in effects {
        for lifecycle in [
            OptiScalerCleanupLifecycle::Rollback,
            OptiScalerCleanupLifecycle::Committed,
        ] {
            for artifact in [
                ArtifactSlot::Custody,
                ArtifactSlot::Stage,
                ArtifactSlot::Discard,
            ] {
                let expected = matches!(
                    (lifecycle, &effect, artifact),
                    (
                        OptiScalerCleanupLifecycle::Rollback,
                        OperationEffect::Write(effect),
                        ArtifactSlot::Custody | ArtifactSlot::Stage,
                    ) if matches!(effect.state(), WriteState::Preserved)
                ) || matches!(
                    (lifecycle, &effect, artifact),
                    (
                        OptiScalerCleanupLifecycle::Rollback,
                        OperationEffect::CreateDirectory(effect),
                        ArtifactSlot::Stage,
                    ) if matches!(effect.state(), CreateDirectoryState::Preserved)
                ) || matches!(
                    (lifecycle, &effect, artifact),
                    (
                        OptiScalerCleanupLifecycle::Committed,
                        OperationEffect::Write(effect),
                        ArtifactSlot::Custody,
                    ) if matches!(effect.state(), WriteState::Applied { .. })
                ) || matches!(
                    (lifecycle, &effect, artifact),
                    (
                        OptiScalerCleanupLifecycle::Committed,
                        OperationEffect::Delete(effect),
                        ArtifactSlot::Custody,
                    ) if matches!(effect.state(), DeleteState::Applied { .. })
                );
                assert_eq!(
                    validate_optiscaler_cleanup_artifact(&effect, artifact, lifecycle).is_ok(),
                    expected,
                    "unexpected cleanup authority for {lifecycle:?} {artifact:?}"
                );
            }
        }
    }
}
