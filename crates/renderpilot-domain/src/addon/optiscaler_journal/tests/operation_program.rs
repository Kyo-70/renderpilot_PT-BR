#[test]
fn repeated_paths_require_the_immediate_prior_postimage() {
    let first = planned_operation_record(
        0,
        "game/dxgi.dll",
        Preimage::Initial {
            observation: DurableObservation::Absent,
            receipt: None,
            owned_basis: None,
        },
        |endpoint| {
            OperationEffect::Write(WriteEffect::new(endpoint, WriteState::Planned).expect("write"))
        },
    );
    let second = planned_operation_record(
        1,
        "GAME\\DXGI.DLL",
        Preimage::PriorPostimage {
            operation_id: 0,
            endpoint: Endpoint::Single,
        },
        |endpoint| {
            OperationEffect::Verify(
                VerifyEffect::new(endpoint, VerifyState::Planned).expect("verify"),
            )
        },
    );
    assert!(
        journal_with_operations(
            vec!["game".to_owned()],
            namespace(),
            vec![first.clone(), second]
        )
        .is_ok()
    );

    let invalid = planned_operation_record(
        1,
        "game/dxgi.dll",
        Preimage::Initial {
            observation: DurableObservation::Absent,
            receipt: None,
            owned_basis: None,
        },
        |endpoint| {
            OperationEffect::Verify(
                VerifyEffect::new(endpoint, VerifyState::Planned).expect("verify"),
            )
        },
    );
    assert!(
        journal_with_operations(
            vec!["game".to_owned()],
            namespace(),
            vec![first.clone(), invalid]
        )
        .is_err()
    );

    let stale = planned_operation_record(
        2,
        "game/dxgi.dll",
        Preimage::PriorPostimage {
            operation_id: 0,
            endpoint: Endpoint::Single,
        },
        |endpoint| {
            OperationEffect::Verify(
                VerifyEffect::new(endpoint, VerifyState::Planned).expect("verify"),
            )
        },
    );
    assert!(
        journal_with_operations(
            vec!["game".to_owned()],
            namespace(),
            vec![first, second_for_path(1), stale]
        )
        .is_err()
    );
}

fn second_for_path(operation_id: u32) -> OperationRecord {
    planned_operation_record(
        operation_id,
        "game/dxgi.dll",
        Preimage::PriorPostimage {
            operation_id: operation_id - 1,
            endpoint: Endpoint::Single,
        },
        |endpoint| {
            OperationEffect::Verify(
                VerifyEffect::new(endpoint, VerifyState::Planned).expect("verify"),
            )
        },
    )
}

#[test]
fn one_relocation_cannot_touch_one_normalized_path_twice() {
    let source = planned_endpoint(
        Endpoint::Source,
        r"C:\Games\dxgi.dll",
        Preimage::Initial {
            observation: DurableObservation::Absent,
            receipt: None,
            owned_basis: None,
        },
    );
    let destination = planned_endpoint(
        Endpoint::Destination,
        "c:/games/dxgi.dll",
        Preimage::Initial {
            observation: DurableObservation::Absent,
            receipt: None,
            owned_basis: None,
        },
    );
    assert!(RelocateEffect::new(source, destination, RelocateState::Planned).is_err());
}

#[test]
fn parent_dependencies_must_be_previous_direct_directory_creators() {
    let directory = planned_operation_record(
        0,
        r"game\config",
        Preimage::Initial {
            observation: DurableObservation::Absent,
            receipt: None,
            owned_basis: None,
        },
        |endpoint| {
            OperationEffect::CreateDirectory(
                CreateDirectoryEffect::new(endpoint, CreateDirectoryState::Planned)
                    .expect("directory"),
            )
        },
    );
    let mut child = planned_operation_record(
        1,
        r"GAME/config\OptiScaler.ini",
        Preimage::Initial {
            observation: DurableObservation::Absent,
            receipt: None,
            owned_basis: None,
        },
        |endpoint| {
            OperationEffect::Write(WriteEffect::new(endpoint, WriteState::Planned).expect("write"))
        },
    );
    child.parent_dependencies = vec![0];
    assert!(
        journal_with_operations(vec!["game".to_owned()], namespace(), vec![directory, child])
            .is_ok()
    );

    let mut wrong_kind = planned_operation_record(
        1,
        "game/config/OptiScaler.ini",
        Preimage::Initial {
            observation: DurableObservation::Absent,
            receipt: None,
            owned_basis: None,
        },
        |endpoint| {
            OperationEffect::Write(WriteEffect::new(endpoint, WriteState::Planned).expect("write"))
        },
    );
    wrong_kind.parent_dependencies = vec![0];
    let mut write_first = planned_operation_record(
        0,
        "game/other",
        Preimage::Initial {
            observation: DurableObservation::Absent,
            receipt: None,
            owned_basis: None,
        },
        |endpoint| {
            OperationEffect::Write(WriteEffect::new(endpoint, WriteState::Planned).expect("write"))
        },
    );
    write_first.parent_dependencies = Vec::new();
    assert!(
        journal_with_operations(
            vec!["game".to_owned()],
            namespace(),
            vec![write_first, wrong_kind]
        )
        .is_err()
    );

    let mut wrong_parent = planned_operation_record(
        0,
        "game/other",
        Preimage::Initial {
            observation: DurableObservation::Absent,
            receipt: None,
            owned_basis: None,
        },
        |endpoint| {
            OperationEffect::CreateDirectory(
                CreateDirectoryEffect::new(endpoint, CreateDirectoryState::Planned)
                    .expect("directory"),
            )
        },
    );
    wrong_parent.parent_dependencies = Vec::new();
    let mut child = planned_operation_record(
        1,
        "game/config/OptiScaler.ini",
        Preimage::Initial {
            observation: DurableObservation::Absent,
            receipt: None,
            owned_basis: None,
        },
        |endpoint| {
            OperationEffect::Write(WriteEffect::new(endpoint, WriteState::Planned).expect("write"))
        },
    );
    child.parent_dependencies = vec![0];
    assert!(
        journal_with_operations(
            vec!["game".to_owned()],
            namespace(),
            vec![wrong_parent, child]
        )
        .is_err()
    );

    let duplicate = OperationRecord::new(
        1,
        vec![0, 0],
        Some(0),
        slots(),
        OperationEffect::Write(
            WriteEffect::new(
                planned_endpoint(
                    Endpoint::Single,
                    "game/config/OptiScaler.ini",
                    Preimage::Initial {
                        observation: DurableObservation::Absent,
                        receipt: None,
                        owned_basis: None,
                    },
                ),
                WriteState::Planned,
            )
            .expect("write"),
        ),
    );
    assert!(duplicate.is_err());
}

#[test]
fn roots_and_materialization_frontier_are_lexically_strict() {
    let operation = operation();
    assert!(
        journal_with_operations(
            vec![r"C:\Games".to_owned(), "c:/games".to_owned()],
            namespace(),
            vec![operation.clone()]
        )
        .is_err()
    );

    let mut planned = journal_with_operations(
        vec!["game".to_owned()],
        namespace(),
        vec![operation.clone()],
    )
    .expect("planned journal");
    planned
        .control_namespace_mut()
        .set_identity("control")
        .expect("control identity");
    assert!(planned.validate().is_err());

    let mut control_ready = journal_with_operations(
        vec!["game".to_owned()],
        namespace(),
        vec![operation.clone()],
    )
    .expect("planned journal");
    control_ready
        .control_namespace_mut()
        .set_identity("control")
        .expect("control identity");
    control_ready.set_materialization(MaterializationState::Workspaces {
        next_workspace_id: 0,
    });
    assert!(control_ready.validate().is_ok());

    let mut workspaces =
        journal_with_operations(vec!["game".to_owned()], namespace(), vec![operation])
            .expect("planned journal");
    workspaces
        .control_namespace_mut()
        .set_identity("control")
        .expect("control identity");
    workspaces.set_materialization(MaterializationState::WorkspaceCreateIntent { workspace_id: 0 });
    assert!(workspaces.validate().is_ok());
    workspaces.private_workspaces_mut()[0]
        .set_identity("workspace-0")
        .expect("workspace identity");
    assert!(workspaces.validate().is_err());
    workspaces.set_materialization(MaterializationState::Workspaces {
        next_workspace_id: 1,
    });
    assert!(workspaces.validate().is_ok());
}
