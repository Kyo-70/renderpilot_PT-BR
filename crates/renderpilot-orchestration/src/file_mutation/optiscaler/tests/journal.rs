use super::*;

#[test]
fn prior_postimage_is_explicit() {
    let value = DomainOperationEndpoint::new(
        DomainEndpoint::Single,
        "x",
        DomainPreimage::PriorPostimage {
            operation_id: 0,
            endpoint: DomainEndpoint::Single,
        },
        JournalAfter::Pending,
    );
    assert!(value.is_ok());
}

#[test]
fn relocate_then_write_uses_the_relocation_destination_postimage() {
    let root = tempfile::tempdir().expect("root");
    let source = root.path().join("source.bin");
    let destination = root.path().join("destination.bin");
    fs::write(&source, b"before").expect("source");
    let receipt = owned_receipt(&source);
    let journal = planned_journal(
        root.path(),
        &[
            OptiScalerPlannedOperation::Relocate {
                source: PlannedParticipant {
                    path: source,
                    preimage: PlannedPreimage::Exact {
                        current: receipt.clone(),
                        prior_owned: Some(receipt),
                    },
                },
                destination: PlannedParticipant {
                    path: destination.clone(),
                    preimage: PlannedPreimage::Absent,
                },
            },
            OptiScalerPlannedOperation::Write(PlannedParticipant {
                path: destination,
                preimage: PlannedPreimage::Absent,
            }),
        ],
    );
    assert!(matches!(
        journal.operations()[0].effect().endpoints()[0].preimage(),
        DomainPreimage::Initial { .. }
    ));
    assert_eq!(
        journal.operations()[1].effect().endpoints()[0].preimage(),
        &DomainPreimage::PriorPostimage {
            operation_id: 0,
            endpoint: DomainEndpoint::Destination,
        }
    );
}

#[test]
fn relocate_source_then_write_uses_the_explicit_absent_postimage() {
    let root = tempfile::tempdir().expect("root");
    let source = root.path().join("source.bin");
    let destination = root.path().join("destination.bin");
    fs::write(&source, b"before").expect("source");
    let receipt = owned_receipt(&source);
    let journal = planned_journal(
        root.path(),
        &[
            OptiScalerPlannedOperation::Relocate {
                source: PlannedParticipant {
                    path: source.clone(),
                    preimage: PlannedPreimage::Exact {
                        current: receipt.clone(),
                        prior_owned: Some(receipt),
                    },
                },
                destination: PlannedParticipant {
                    path: destination,
                    preimage: PlannedPreimage::Absent,
                },
            },
            OptiScalerPlannedOperation::Write(PlannedParticipant {
                path: source,
                preimage: PlannedPreimage::Absent,
            }),
        ],
    );
    assert_eq!(
        journal.operations()[1].effect().endpoints()[0].preimage(),
        &DomainPreimage::PriorPostimage {
            operation_id: 0,
            endpoint: DomainEndpoint::Source,
        }
    );
}

#[test]
fn delete_then_relocate_uses_the_delete_postimage() {
    let root = tempfile::tempdir().expect("root");
    let deleted = root.path().join("deleted.bin");
    let source = root.path().join("source.bin");
    fs::write(&deleted, b"before").expect("delete target");
    fs::write(&source, b"move source").expect("source");
    let deleted_receipt = owned_receipt(&deleted);
    let source_receipt = owned_receipt(&source);
    let journal = planned_journal(
        root.path(),
        &[
            OptiScalerPlannedOperation::Delete(PlannedParticipant {
                path: deleted.clone(),
                preimage: PlannedPreimage::Exact {
                    current: deleted_receipt.clone(),
                    prior_owned: Some(deleted_receipt),
                },
            }),
            OptiScalerPlannedOperation::Relocate {
                source: PlannedParticipant {
                    path: source,
                    preimage: PlannedPreimage::Exact {
                        current: source_receipt.clone(),
                        prior_owned: Some(source_receipt),
                    },
                },
                destination: PlannedParticipant {
                    path: deleted,
                    preimage: PlannedPreimage::Absent,
                },
            },
        ],
    );
    let DomainOperationEffect::Relocate(effect) = journal.operations()[1].effect() else {
        panic!("expected relocation");
    };
    assert_eq!(
        effect.destination().preimage(),
        &DomainPreimage::PriorPostimage {
            operation_id: 0,
            endpoint: DomainEndpoint::Single,
        }
    );
}

#[test]
fn parent_directory_is_the_only_dependency_for_a_nested_write() {
    let root = tempfile::tempdir().expect("root");
    let directory = root.path().join("created");
    let target = directory.join("new.bin");
    let journal = planned_journal(
        root.path(),
        &[
            OptiScalerPlannedOperation::CreateDirectory(PlannedParticipant {
                path: directory,
                preimage: PlannedPreimage::Absent,
            }),
            OptiScalerPlannedOperation::Write(PlannedParticipant {
                path: target,
                preimage: PlannedPreimage::Absent,
            }),
        ],
    );
    assert_eq!(journal.operations()[0].parent_dependencies(), &[] as &[u32]);
    assert_eq!(journal.operations()[1].parent_dependencies(), &[0]);
}

#[test]
fn committed_cleanup_cursor_moves_the_final_workspace_into_control() {
    let root = tempfile::tempdir().expect("root");
    let mut journal = planned_journal(
        root.path(),
        &[OptiScalerPlannedOperation::Write(PlannedParticipant {
            path: root.path().join("one.bin"),
            preimage: PlannedPreimage::Absent,
        })],
    );
    journal
        .control_namespace_mut()
        .set_identity("control-identity".to_owned())
        .expect("control identity");
    journal.private_workspaces_mut()[0]
        .set_identity("workspace-zero")
        .expect("workspace identity");
    assert_eq!(
        next_workspace_cleanup_state(&journal, 0).expect("W0 to control"),
        JournalCleanup::ControlRemoveIntent {
            expected_identity: "control-identity".to_owned(),
        }
    );
}

#[test]
fn artifact_operations_on_one_root_share_one_workspace_in_first_use_order() {
    let root = tempfile::tempdir().expect("root");
    let journal = planned_journal(
        root.path(),
        &[
            OptiScalerPlannedOperation::Write(PlannedParticipant {
                path: root.path().join("first.bin"),
                preimage: PlannedPreimage::Absent,
            }),
            OptiScalerPlannedOperation::CreateDirectory(PlannedParticipant {
                path: root.path().join("created"),
                preimage: PlannedPreimage::Absent,
            }),
            OptiScalerPlannedOperation::Write(PlannedParticipant {
                path: root.path().join("second.bin"),
                preimage: PlannedPreimage::Absent,
            }),
        ],
    );

    assert_eq!(journal.private_workspaces().len(), 1);
    assert_eq!(journal.private_workspaces()[0].workspace_id(), 0);
    assert_eq!(journal.private_workspaces()[0].root_index(), 0);
    assert_eq!(
        journal
            .operations()
            .iter()
            .map(DomainOperationRecord::workspace_id)
            .collect::<Vec<_>>(),
        vec![Some(0), Some(0), Some(0)]
    );
}

#[test]
fn disjoint_roots_on_one_filesystem_keep_separate_workspaces() {
    let root = tempfile::tempdir().expect("root");
    let first = root.path().join("first");
    let second = root.path().join("second");
    fs::create_dir_all(&first).expect("first root");
    fs::create_dir_all(&second).expect("second root");
    let scope = MutationScope::new([first.clone(), second.clone()]).expect("scope");
    let journal = build_journal(
        &scope,
        &root.path().join("transaction-id"),
        &[
            OptiScalerPlannedOperation::Write(PlannedParticipant {
                path: first.join("first.bin"),
                preimage: PlannedPreimage::Absent,
            }),
            OptiScalerPlannedOperation::Write(PlannedParticipant {
                path: second.join("second.bin"),
                preimage: PlannedPreimage::Absent,
            }),
        ],
        ThreatModel::CooperativeSameUid,
    )
    .expect("journal");

    assert_eq!(journal.private_workspaces().len(), 2);
    assert_eq!(
        journal
            .private_workspaces()
            .iter()
            .map(renderpilot_domain::PrivateWorkspaceBinding::root_index)
            .collect::<Vec<_>>(),
        vec![0, 1]
    );
    assert_eq!(
        journal
            .operations()
            .iter()
            .map(DomainOperationRecord::workspace_id)
            .collect::<Vec<_>>(),
        vec![Some(0), Some(1)]
    );
}

#[test]
fn overlapping_roots_select_the_outermost_root_deterministically() {
    let root = tempfile::tempdir().expect("root");
    let inner = root.path().join("inner");
    fs::create_dir_all(&inner).expect("inner root");
    // The input order deliberately puts the inner root first.  Root index must
    // still name the outermost containing root, not the first matching root.
    let scope = MutationScope::new([inner.clone(), root.path().to_path_buf()]).expect("scope");
    let journal = build_journal(
        &scope,
        &root.path().join("transaction-id"),
        &[OptiScalerPlannedOperation::Write(PlannedParticipant {
            path: inner.join("target.bin"),
            preimage: PlannedPreimage::Absent,
        })],
        ThreatModel::CooperativeSameUid,
    )
    .expect("journal");

    assert_eq!(journal.private_workspaces().len(), 1);
    assert_eq!(journal.private_workspaces()[0].root_index(), 1);
    assert_eq!(journal.operations()[0].workspace_id(), Some(0));
}

#[test]
fn materialized_workspace_resolves_exact_operation_slot_leaf() {
    let root = tempfile::tempdir().expect("root");
    let id = "transaction-id";
    let executor = PeerMutationExecutor::new(SqliteStorage::in_memory().expect("storage"));
    let game_id =
        renderpilot_domain::GameId::new("test:optiscaler-artifact-leaf").expect("game id");
    seed_optiscaler_test_game(executor.repositories(), &game_id, root.path());
    let mut journal = planned_journal(
        root.path(),
        &[OptiScalerPlannedOperation::Write(PlannedParticipant {
            path: root.path().join("target.bin"),
            preimage: PlannedPreimage::Absent,
        })],
    );
    let journal_json = serde_json::to_string(&journal).expect("journal json");
    let preparing = executor
        .begin_optiscaler_journal_aggregate(
            OptiScalerJournalAggregateBegin::new(
                id,
                game_id.clone(),
                "optiscaler_install",
                None,
                journal_json.clone(),
            )
            .expect("begin aggregate"),
        )
        .expect("reserve mutation");
    let preparing =
        materialize_namespaces(&executor, preparing, &mut journal, &root.path().join(id))
            .expect("materialize namespaces");
    let prepared = PreparedFileMutation {
        id: id.to_owned(),
        game_id,
        executor: &executor,
        authority: JournalAuthority::ActivePreparing(Box::new(preparing)),
        transaction_root: root.path().to_path_buf(),
        journal_json,
        journal,
        next_operation_id: 0,
        managed_endpoint_roots: std::collections::HashMap::new(),
    };

    let artifact = private_artifact(&prepared, 0, ArtifactSlot::Custody).expect("artifact");
    assert_eq!(artifact.leaf.as_os_str(), std::ffi::OsStr::new("0-custody"));
    let expected_workspace_path =
        derived_private_workspace_path(&prepared.id, &prepared.journal, 0).expect("workspace path");
    assert_eq!(artifact.path, expected_workspace_path.join("0-custody"));
    assert!(crate::paths::same_path(
        &artifact.path,
        &PathBuf::from(prepared.journal.private_workspaces()[0].path()).join("0-custody")
    ));
}

#[test]
fn persisted_private_workspace_path_must_match_derived_closure() {
    let root = tempfile::tempdir().expect("root");
    let journal = planned_journal(
        root.path(),
        &[OptiScalerPlannedOperation::Write(PlannedParticipant {
            path: root.path().join("target.bin"),
            preimage: PlannedPreimage::Absent,
        })],
    );
    let mut wire = serde_json::to_value(&journal).expect("journal wire");
    let original = wire["private_workspaces"][0]["path"]
        .as_str()
        .expect("private path");
    let leaf = Path::new(original).file_name().expect("private leaf");
    let forged = root.path().join("forged").join(leaf).display().to_string();
    wire["private_workspaces"][0]["path"] =
        serde_json::Value::String(renderpilot_domain::normalized_path_key(&forged));
    let forged: OptiScalerJournal = serde_json::from_value(wire).expect("domain wire");
    assert!(validate_private_workspace_paths("transaction-id", &forged).is_err());
}

#[test]
fn write_after_reused_relocation_is_rejected_by_the_planner() {
    let root = tempfile::tempdir().expect("root");
    let source = root.path().join("source.bin");
    let destination = root.path().join("destination.bin");
    fs::write(&source, b"reused").expect("source");
    let receipt = owned_receipt(&source);
    let reused = FileReceipt::reused(receipt.identity().to_owned(), receipt.digest().clone())
        .expect("reused receipt");
    let result = build_journal(
        &MutationScope::single(root.path()).expect("scope"),
        &root.path().join("transaction-id"),
        &[
            OptiScalerPlannedOperation::Relocate {
                source: PlannedParticipant {
                    path: source,
                    preimage: PlannedPreimage::Exact {
                        current: reused,
                        prior_owned: None,
                    },
                },
                destination: PlannedParticipant {
                    path: destination.clone(),
                    preimage: PlannedPreimage::Absent,
                },
            },
            OptiScalerPlannedOperation::Write(PlannedParticipant {
                path: destination,
                preimage: PlannedPreimage::Absent,
            }),
        ],
        ThreatModel::CooperativeSameUid,
    );
    assert!(result.is_err());
}

#[test]
fn generic_exact_never_authorizes_a_reused_participant_and_typed_authority_is_action_scoped() {
    let root = tempfile::tempdir().expect("root");
    let source = root.path().join("reused.bin");
    fs::write(&source, b"reused").expect("source");
    let owned = owned_receipt(&source);
    let reused = FileReceipt::reused(owned.identity().to_owned(), owned.digest().clone())
        .expect("reused receipt");
    let generic = || PlannedPreimage::Exact {
        current: reused.clone(),
        prior_owned: None,
    };

    for operation in [
        OptiScalerPlannedOperation::Write(PlannedParticipant {
            path: source.clone(),
            preimage: generic(),
        }),
        OptiScalerPlannedOperation::Delete(PlannedParticipant {
            path: source.clone(),
            preimage: generic(),
        }),
        OptiScalerPlannedOperation::Verify(PlannedParticipant {
            path: source.clone(),
            preimage: generic(),
        }),
        OptiScalerPlannedOperation::Relocate {
            source: PlannedParticipant {
                path: source.clone(),
                preimage: generic(),
            },
            destination: PlannedParticipant {
                path: root.path().join("generic-destination.bin"),
                preimage: PlannedPreimage::Absent,
            },
        },
    ] {
        assert!(
            build_journal(
                &MutationScope::single(root.path()).expect("scope"),
                &root.path().join("transaction-id"),
                &[operation],
                ThreatModel::CooperativeSameUid,
            )
            .is_err(),
            "generic Exact must reject every Reused action"
        );
    }

    let observation = PlannedPreimage::ExactReused {
        current: reused.clone(),
        authority: ReusedMutationAuthority::ObservationOnly,
    };
    assert!(
        build_journal(
            &MutationScope::single(root.path()).expect("scope"),
            &root.path().join("transaction-observe"),
            &[OptiScalerPlannedOperation::Verify(PlannedParticipant {
                path: source.clone(),
                preimage: observation.clone(),
            })],
            ThreatModel::CooperativeSameUid,
        )
        .is_ok()
    );
    for operation in [
        OptiScalerPlannedOperation::Write(PlannedParticipant {
            path: source.clone(),
            preimage: observation.clone(),
        }),
        OptiScalerPlannedOperation::Delete(PlannedParticipant {
            path: source.clone(),
            preimage: observation.clone(),
        }),
        OptiScalerPlannedOperation::Relocate {
            source: PlannedParticipant {
                path: source.clone(),
                preimage: observation,
            },
            destination: PlannedParticipant {
                path: root.path().join("observe-destination.bin"),
                preimage: PlannedPreimage::Absent,
            },
        },
    ] {
        assert!(
            build_journal(
                &MutationScope::single(root.path()).expect("scope"),
                &root.path().join("transaction-observe-rejected"),
                &[operation],
                ThreatModel::CooperativeSameUid,
            )
            .is_err()
        );
    }

    assert!(
        build_journal(
            &MutationScope::single(root.path()).expect("scope"),
            &root.path().join("transaction-relocate"),
            &[OptiScalerPlannedOperation::Relocate {
                source: PlannedParticipant {
                    path: source,
                    preimage: PlannedPreimage::ExactReused {
                        current: reused,
                        authority: ReusedMutationAuthority::RelocationSource,
                    },
                },
                destination: PlannedParticipant {
                    path: root.path().join("relocation-destination.bin"),
                    preimage: PlannedPreimage::Absent,
                },
            }],
            ThreatModel::CooperativeSameUid,
        )
        .is_ok()
    );
}

#[test]
fn out_of_order_reference_fails_before_a_write_syscall() {
    let root = tempfile::tempdir().expect("root");
    let source = root.path().join("source.bin");
    let destination = root.path().join("destination.bin");
    fs::write(&source, b"before").expect("source");
    let receipt = owned_receipt(&source);
    let journal = planned_journal(
        root.path(),
        &[
            OptiScalerPlannedOperation::Relocate {
                source: PlannedParticipant {
                    path: source,
                    preimage: PlannedPreimage::Exact {
                        current: receipt.clone(),
                        prior_owned: Some(receipt),
                    },
                },
                destination: PlannedParticipant {
                    path: destination.clone(),
                    preimage: PlannedPreimage::Absent,
                },
            },
            OptiScalerPlannedOperation::Write(PlannedParticipant {
                path: destination.clone(),
                preimage: PlannedPreimage::Absent,
            }),
        ],
    );
    let executor = PeerMutationExecutor::new(SqliteStorage::in_memory().expect("storage"));
    let mut prepared = prepared_for(&executor, root.path(), journal, 1);
    set_prior_postimage(
        prepared.journal.operations_mut()[1].effect_mut(),
        &destination,
        DomainEndpoint::Single,
        1,
        DomainEndpoint::Destination,
    )
    .expect("replace preimage");
    assert!(prepared.write_file(&destination, b"after").is_err());
    assert_eq!(observe(&destination), DiskObservation::Absent);
}

#[test]
fn invalid_producer_endpoint_fails_without_path_fallback() {
    let root = tempfile::tempdir().expect("root");
    let source = root.path().join("source.bin");
    let destination = root.path().join("destination.bin");
    fs::write(&source, b"before").expect("source");
    let receipt = owned_receipt(&source);
    let journal = planned_journal(
        root.path(),
        &[
            OptiScalerPlannedOperation::Relocate {
                source: PlannedParticipant {
                    path: source,
                    preimage: PlannedPreimage::Exact {
                        current: receipt.clone(),
                        prior_owned: Some(receipt),
                    },
                },
                destination: PlannedParticipant {
                    path: destination.clone(),
                    preimage: PlannedPreimage::Absent,
                },
            },
            OptiScalerPlannedOperation::Write(PlannedParticipant {
                path: destination.clone(),
                preimage: PlannedPreimage::Absent,
            }),
        ],
    );
    let executor = PeerMutationExecutor::new(SqliteStorage::in_memory().expect("storage"));
    let mut prepared = prepared_for(&executor, root.path(), journal, 1);
    set_prior_postimage(
        prepared.journal.operations_mut()[1].effect_mut(),
        &destination,
        DomainEndpoint::Single,
        0,
        DomainEndpoint::Single,
    )
    .expect("replace preimage");
    assert!(prepared.write_file(&destination, b"after").is_err());
    assert_eq!(observe(&destination), DiskObservation::Absent);
}

#[test]
fn action_crash_windows_round_trip_as_shared_states() {
    let root = tempfile::tempdir().expect("root");
    let write_path = root.path().join("write.bin");
    let mut write_journal = planned_journal(
        root.path(),
        &[OptiScalerPlannedOperation::Write(PlannedParticipant {
            path: write_path,
            preimage: PlannedPreimage::Absent,
        })],
    );
    if let DomainOperationEffect::Write(effect) = write_journal.operations_mut()[0].effect_mut() {
        *effect.state_mut() = DomainWriteState::StageIntent {
            target_digest: hash_bytes(b"after").expect("digest"),
        };
    }

    let delete_path = root.path().join("delete.bin");
    fs::write(&delete_path, b"delete").expect("delete source");
    let delete_receipt = owned_receipt(&delete_path);
    let mut delete_journal = planned_journal(
        root.path(),
        &[OptiScalerPlannedOperation::Delete(PlannedParticipant {
            path: delete_path,
            preimage: PlannedPreimage::Exact {
                current: delete_receipt.clone(),
                prior_owned: Some(delete_receipt),
            },
        })],
    );
    if let DomainOperationEffect::Delete(effect) = delete_journal.operations_mut()[0].effect_mut() {
        *effect.state_mut() = DomainDeleteState::CaptureIntent;
    }

    let relocate_source = root.path().join("relocate.bin");
    let relocate_destination = root.path().join("relocated.bin");
    fs::write(&relocate_source, b"relocate").expect("relocate source");
    let relocate_receipt = owned_receipt(&relocate_source);
    let mut relocate_journal = planned_journal(
        root.path(),
        &[OptiScalerPlannedOperation::Relocate {
            source: PlannedParticipant {
                path: relocate_source,
                preimage: PlannedPreimage::Exact {
                    current: relocate_receipt.clone(),
                    prior_owned: Some(relocate_receipt),
                },
            },
            destination: PlannedParticipant {
                path: relocate_destination,
                preimage: PlannedPreimage::Absent,
            },
        }],
    );
    if let DomainOperationEffect::Relocate(effect) =
        relocate_journal.operations_mut()[0].effect_mut()
    {
        *effect.state_mut() = DomainRelocateState::MoveIntent;
    }

    for journal in [write_journal, delete_journal, relocate_journal] {
        let encoded = serde_json::to_string(&journal).expect("encode state");
        let restored: OptiScalerJournal = serde_json::from_str(&encoded).expect("decode state");
        assert_eq!(
            encoded,
            serde_json::to_string(&restored).expect("re-encode state")
        );
    }
}

#[test]
fn materialization_control_edges_use_real_sqlite_cas() {
    let root = tempfile::tempdir().expect("root");
    let game_id =
        renderpilot_domain::GameId::new("test:optiscaler-materialization").expect("game id");
    let id = "transaction-id";
    let mutation_id_anchor = root.path().join(id);
    let executor = PeerMutationExecutor::new(SqliteStorage::in_memory().expect("storage"));
    let storage = executor.repositories();
    seed_optiscaler_test_game(storage, &game_id, root.path());
    let mut journal = planned_journal(
        root.path(),
        &[OptiScalerPlannedOperation::Verify(PlannedParticipant {
            path: root.path().join("target.bin"),
            preimage: PlannedPreimage::Verify,
        })],
    );
    let initial = serde_json::to_string(&journal).expect("initial journal");
    let preparing = executor
        .begin_optiscaler_journal_aggregate(
            OptiScalerJournalAggregateBegin::new(id, game_id, "optiscaler_install", None, initial)
                .expect("begin aggregate"),
        )
        .expect("reserve mutation");
    materialize_namespaces(&executor, preparing, &mut journal, &mutation_id_anchor)
        .expect("materialize namespaces");
    let persisted = storage
        .get_pending_file_mutation(id)
        .expect("read persisted mutation")
        .expect("persisted row");
    let persisted_journal: OptiScalerJournal =
        serde_json::from_str(&persisted.manifest_json).expect("persisted journal");
    assert_eq!(
        persisted_journal.materialization(),
        &DomainMaterializationState::Ready
    );
    assert!(persisted_journal.private_workspaces().is_empty());
    assert!(persisted_journal.control_namespace().identity().is_some());
    assert!(
        fs::read_dir(root.path())
            .expect("workspace root entries")
            .all(|entry| !entry
                .expect("workspace root entry")
                .file_name()
                .to_string_lossy()
                .starts_with(".renderpilot-optiscaler-workspace-"))
    );

    cleanup_transaction_namespace(root.path(), id, &journal, None)
        .expect("cleanup control namespace");
}

#[test]
fn prepared_resolution_fence_deletes_only_after_restoration_completion() {
    let root = tempfile::tempdir().expect("root");
    let executor = PeerMutationExecutor::new(SqliteStorage::in_memory().expect("storage"));
    let storage = executor.repositories();
    let game_id = renderpilot_domain::GameId::new("test:optiscaler-fence").expect("game id");
    seed_optiscaler_test_game(storage, &game_id, root.path());
    let id = "transaction-fence";
    let mutation_id_anchor = root.path().join(id);
    let mut journal = build_journal(
        &MutationScope::single(root.path()).expect("scope"),
        &mutation_id_anchor,
        &[OptiScalerPlannedOperation::Verify(PlannedParticipant {
            path: root.path().join("target.bin"),
            preimage: PlannedPreimage::Verify,
        })],
        ThreatModel::CooperativeSameUid,
    )
    .expect("journal");
    if let DomainOperationEffect::Verify(effect) = journal.operations_mut()[0].effect_mut() {
        *effect.state_mut() = DomainVerifyState::Applied {
            observed: durable(&DiskObservation::Absent),
        };
        set_endpoint_expected(effect.endpoint_mut(), &DiskObservation::Absent);
    }
    let initial = serde_json::to_string(&journal).expect("initial journal");
    let preparing = executor
        .begin_optiscaler_journal_aggregate(
            OptiScalerJournalAggregateBegin::new(
                id,
                game_id.clone(),
                "optiscaler_install",
                None,
                initial,
            )
            .expect("begin aggregate"),
        )
        .expect("reserve mutation");
    let preparing = materialize_namespaces(&executor, preparing, &mut journal, &mutation_id_anchor)
        .expect("materialize namespaces");
    let prepared_json = serde_json::to_string(&journal).expect("prepared journal");
    executor
        .finish_optiscaler_journal_aggregate(preparing, prepared_json)
        .expect("finish preparation");
    assert_eq!(
        storage
            .get_pending_file_mutation(id)
            .expect("read prepared row")
            .expect("prepared row")
            .state,
        PendingFileMutationState::Prepared
    );
    let mut candidates = executor
        .recover_pending_file_mutation_candidates_for_game(&game_id, |row| row.id == id)
        .expect("acquire recovery proof");
    let proof = match candidates.pop().expect("recovery candidate") {
        PendingFileMutationRecoveryCandidate::OptiScaler(proof) => proof,
        PendingFileMutationRecoveryCandidate::NotAggregate(_) => {
            panic!("expected an OptiScaler recovery proof")
        }
    };
    let journal = proof.journal().clone();
    let journal_json = proof.current_journal_json().to_owned();
    let prepared = PreparedFileMutation {
        id: id.to_owned(),
        game_id,
        executor: &executor,
        authority: JournalAuthority::Recovering(proof),
        transaction_root: root.path().to_path_buf(),
        journal,
        journal_json,
        next_operation_id: 0,
        managed_endpoint_roots: std::collections::HashMap::new(),
    };
    rollback_prepared(prepared).expect("restore and delete recovery proof");
    assert!(
        storage
            .get_pending_file_mutation(id)
            .expect("read completed row")
            .is_none()
    );
}
