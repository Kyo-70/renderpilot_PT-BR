use renderpilot_domain::{
    ControlNamespaceBinding, DurableObservation, Endpoint, ExpectedAfter, MaterializationState,
    NamespaceCapability, OperationEffect, OperationEndpoint, OperationRecord, OptiScalerJournal,
    Preimage, PrivateArtifactSlots, PrivateWorkspaceBinding, RemoveDirectoryEffect,
    RemoveDirectoryState, Sha256Hash, WriteEffect, WriteState,
};

pub(super) fn capability() -> NamespaceCapability {
    NamespaceCapability::new("a".repeat(64)).expect("capability")
}

pub(super) fn hash() -> Sha256Hash {
    Sha256Hash::new("b".repeat(64)).expect("hash")
}

pub(super) fn test_journal(
    roots: Vec<String>,
    control: ControlNamespaceBinding,
    operations: Vec<OperationRecord>,
) -> OptiScalerJournal {
    let capability = control.capability().clone();
    let workspaces: Vec<PrivateWorkspaceBinding> = operations
        .iter()
        .filter_map(renderpilot_domain::OperationRecord::workspace_id)
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .map(|workspace_id| {
            PrivateWorkspaceBinding::new(
                workspace_id,
                0,
                format!(
                    "{}/.renderpilot-optiscaler-workspace-test-{workspace_id}-{}",
                    roots[0],
                    capability.as_str()
                ),
                None,
                capability.clone(),
            )
            .expect("workspace")
        })
        .collect();
    OptiScalerJournal::new(roots, control, workspaces, operations).expect("journal")
}

pub(super) fn journal() -> OptiScalerJournal {
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
    test_journal(
        vec!["game".to_owned()],
        ControlNamespaceBinding::new(
            format!("game/control-abc-{}", capability.as_str()),
            None,
            capability,
        )
        .expect("control namespace"),
        vec![operation],
    )
}

pub(super) fn materialized_applied() -> OptiScalerJournal {
    let mut journal = journal();
    journal
        .control_namespace_mut()
        .set_identity("control-id")
        .expect("control identity");
    journal.private_workspaces_mut()[0]
        .set_identity("participant-id")
        .expect("participant identity");
    if let OperationEffect::Write(effect) = journal.operations_mut()[0].effect_mut() {
        let live = DurableObservation::File {
            identity: "live-id".to_owned(),
            digest: hash(),
        };
        effect
            .endpoint_mut()
            .set_expected_after(ExpectedAfter::Known(live.clone()));
        *effect.state_mut() = WriteState::Applied {
            live,
            custody: DurableObservation::Absent,
        };
    }
    journal.set_materialization(MaterializationState::Ready);
    journal
}

pub(super) fn write_publish_transition(
    preimage: Preimage,
    stage: &DurableObservation,
    custody: DurableObservation,
    live: DurableObservation,
) -> (OptiScalerJournal, OptiScalerJournal) {
    let capability = capability();
    let endpoint = OperationEndpoint::new(
        Endpoint::Single,
        "game/publish.dll",
        preimage,
        ExpectedAfter::Pending,
    )
    .expect("publish endpoint");
    let operation = OperationRecord::new(
        0,
        Vec::new(),
        Some(0),
        PrivateArtifactSlots::new(custody.clone(), stage.clone(), DurableObservation::Absent)
            .expect("publish slots"),
        OperationEffect::Write(
            WriteEffect::new(
                endpoint,
                WriteState::PublishIntent {
                    stage: stage.clone(),
                    custody: custody.clone(),
                },
            )
            .expect("publish intent"),
        ),
    )
    .expect("publish operation");
    let mut current = test_journal(
        vec!["game".to_owned()],
        ControlNamespaceBinding::new(
            format!("game/control-publish-{}", capability.as_str()),
            None,
            capability,
        )
        .expect("publish control namespace"),
        vec![operation],
    );
    current
        .control_namespace_mut()
        .set_identity("control-publish")
        .expect("control identity");
    current.private_workspaces_mut()[0]
        .set_identity("participant-publish")
        .expect("participant identity");
    current.set_materialization(MaterializationState::Ready);

    let mut next = current.clone();
    if let OperationEffect::Write(effect) = next.operations_mut()[0].effect_mut() {
        effect
            .endpoint_mut()
            .set_expected_after(ExpectedAfter::Known(live.clone()));
        *effect.state_mut() = WriteState::Applied { live, custody };
    }
    *next.operations_mut()[0].slots_mut().stage_mut() = DurableObservation::Absent;
    (current, next)
}

pub(super) fn write_capture_transition(
    preimage: Preimage,
    custody: DurableObservation,
) -> (OptiScalerJournal, OptiScalerJournal) {
    let capability = capability();
    let stage = DurableObservation::File {
        identity: "capture-stage".to_owned(),
        digest: hash(),
    };
    let endpoint = OperationEndpoint::new(
        Endpoint::Single,
        "game/capture.dll",
        preimage,
        ExpectedAfter::Pending,
    )
    .expect("capture endpoint");
    let operation = OperationRecord::new(
        0,
        Vec::new(),
        Some(0),
        PrivateArtifactSlots::new(
            DurableObservation::Absent,
            stage.clone(),
            DurableObservation::Absent,
        )
        .expect("capture slots"),
        OperationEffect::Write(
            WriteEffect::new(
                endpoint,
                WriteState::CaptureIntent {
                    stage: stage.clone(),
                },
            )
            .expect("capture intent"),
        ),
    )
    .expect("capture operation");
    let current = test_journal(
        vec!["game".to_owned()],
        ControlNamespaceBinding::new(
            format!("game/control-capture-{}", capability.as_str()),
            None,
            capability,
        )
        .expect("capture control namespace"),
        vec![operation],
    );
    let mut next = current.clone();
    if let OperationEffect::Write(effect) = next.operations_mut()[0].effect_mut() {
        *effect.state_mut() = WriteState::Captured {
            stage,
            custody: custody.clone(),
        };
    }
    *next.operations_mut()[0].slots_mut().custody_mut() = custody;
    (current, next)
}

pub(super) fn three_operation_cleanup_journal() -> OptiScalerJournal {
    let mut journal = journal();
    let second_endpoint = OperationEndpoint::new(
        Endpoint::Single,
        "game/other.dll",
        Preimage::Initial {
            observation: DurableObservation::Absent,
            receipt: None,
            owned_basis: None,
        },
        ExpectedAfter::Pending,
    )
    .expect("second endpoint");
    let second = OperationRecord::new(
        1,
        Vec::new(),
        Some(1),
        PrivateArtifactSlots::new(
            DurableObservation::Absent,
            DurableObservation::Absent,
            DurableObservation::Absent,
        )
        .expect("second slots"),
        OperationEffect::Write(
            WriteEffect::new(second_endpoint, WriteState::Planned).expect("second write"),
        ),
    )
    .expect("second operation");
    let cleanup_endpoint = OperationEndpoint::new(
        Endpoint::Single,
        "game/cleanup-dir",
        Preimage::Initial {
            observation: DurableObservation::Directory {
                identity: "cleanup-dir-id".to_owned(),
            },
            receipt: None,
            owned_basis: None,
        },
        ExpectedAfter::Pending,
    )
    .expect("cleanup endpoint");
    let cleanup = OperationRecord::new(
        2,
        Vec::new(),
        None,
        PrivateArtifactSlots::new(
            DurableObservation::Absent,
            DurableObservation::Absent,
            DurableObservation::Absent,
        )
        .expect("cleanup slots"),
        OperationEffect::PostCommitRemoveDirectory(
            RemoveDirectoryEffect::new(
                cleanup_endpoint,
                RemoveDirectoryState::Planned {
                    directory: DurableObservation::Directory {
                        identity: "cleanup-dir-id".to_owned(),
                    },
                },
            )
            .expect("cleanup effect"),
        ),
    )
    .expect("cleanup operation");
    let mut operations = journal.operations().to_vec();
    operations.extend([second, cleanup]);
    journal = test_journal(
        journal.roots().to_vec(),
        journal.control_namespace().clone(),
        operations,
    );
    journal
        .control_namespace_mut()
        .set_identity("control-id")
        .expect("control identity");
    for (index, workspace) in journal.private_workspaces_mut().iter_mut().enumerate() {
        workspace
            .set_identity(format!("participant-{index}"))
            .expect("participant identity");
    }
    for (index, operation) in journal.operations_mut().iter_mut().enumerate() {
        match (index, operation.effect_mut()) {
            (0 | 1, OperationEffect::Write(effect)) => {
                let live = DurableObservation::File {
                    identity: format!("live-{index}"),
                    digest: hash(),
                };
                effect
                    .endpoint_mut()
                    .set_expected_after(ExpectedAfter::Known(live.clone()));
                *effect.state_mut() = WriteState::Applied {
                    live,
                    custody: DurableObservation::Absent,
                };
            }
            (2, OperationEffect::PostCommitRemoveDirectory(effect)) => {
                effect
                    .endpoint_mut()
                    .set_expected_after(ExpectedAfter::Known(DurableObservation::Absent));
                *effect.state_mut() = RemoveDirectoryState::Applied {
                    directory: DurableObservation::Directory {
                        identity: "cleanup-dir-id".to_owned(),
                    },
                };
            }
            _ => panic!("unexpected cleanup journal effect"),
        }
    }
    journal.set_materialization(MaterializationState::Ready);
    journal
}

pub(super) fn applied_postcommit_operation(
    operation_id: u32,
    path: &str,
    directory_identity: &str,
) -> OperationRecord {
    let directory = DurableObservation::Directory {
        identity: directory_identity.to_owned(),
    };
    let endpoint = OperationEndpoint::new(
        Endpoint::Single,
        path,
        Preimage::Initial {
            observation: directory.clone(),
            receipt: None,
            owned_basis: None,
        },
        ExpectedAfter::Known(DurableObservation::Absent),
    )
    .expect("postcommit endpoint");
    OperationRecord::new(
        operation_id,
        Vec::new(),
        None,
        PrivateArtifactSlots::new(
            DurableObservation::Absent,
            DurableObservation::Absent,
            DurableObservation::Absent,
        )
        .expect("postcommit slots"),
        OperationEffect::PostCommitRemoveDirectory(
            RemoveDirectoryEffect::new(endpoint, RemoveDirectoryState::Applied { directory })
                .expect("postcommit effect"),
        ),
    )
    .expect("postcommit operation")
}

pub(super) fn chained_journal(
    producer_state: WriteState,
    consumer_state: WriteState,
    materialized: bool,
) -> OptiScalerJournal {
    let mut journal = journal();
    if let OperationEffect::Write(effect) = journal.operations_mut()[0].effect_mut() {
        if matches!(&producer_state, WriteState::Applied { .. }) {
            let live = DurableObservation::File {
                identity: "producer-live".to_owned(),
                digest: hash(),
            };
            effect
                .endpoint_mut()
                .set_expected_after(ExpectedAfter::Known(live.clone()));
            *effect.state_mut() = WriteState::Applied {
                live,
                custody: DurableObservation::Absent,
            };
        } else {
            *effect.state_mut() = producer_state;
        }
    }
    let endpoint = OperationEndpoint::new(
        Endpoint::Single,
        "game/dxgi.dll",
        Preimage::PriorPostimage {
            operation_id: 0,
            endpoint: Endpoint::Single,
        },
        ExpectedAfter::Pending,
    )
    .expect("consumer endpoint");
    let consumer = OperationRecord::new(
        1,
        Vec::new(),
        Some(1),
        PrivateArtifactSlots::new(
            DurableObservation::Absent,
            DurableObservation::Absent,
            DurableObservation::Absent,
        )
        .expect("consumer slots"),
        OperationEffect::Write(
            WriteEffect::new(endpoint, consumer_state).expect("consumer effect"),
        ),
    )
    .expect("consumer operation");
    let mut operations = journal.operations().to_vec();
    operations.push(consumer);
    journal = test_journal(
        journal.roots().to_vec(),
        journal.control_namespace().clone(),
        operations,
    );
    if materialized {
        journal
            .control_namespace_mut()
            .set_identity("control-id")
            .expect("control identity");
        for (index, workspace) in journal.private_workspaces_mut().iter_mut().enumerate() {
            workspace
                .set_identity(format!("participant-{index}"))
                .expect("participant identity");
        }
        journal.set_materialization(MaterializationState::Ready);
    }
    journal
}
