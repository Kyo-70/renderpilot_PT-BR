#[cfg(test)]
mod tests {
    use super::*;

    use serde_json::json;

    include!("tests/helpers.rs");
    include!("tests/wire_and_lifecycle.rs");
    include!("tests/operation_program.rs");
    include!("tests/transitions.rs");
    include!("tests/cleanup.rs");

    fn control() -> ControlNamespaceBinding {
        ControlNamespaceBinding::new(
            format!("transactions/control-tx-{}", capability()),
            None,
            capability(),
        )
        .expect("control")
    }

    fn workspace() -> PrivateWorkspaceBinding {
        PrivateWorkspaceBinding::new(
            0,
            0,
            format!(
                "game/.renderpilot-optiscaler-workspace-tx-0-{}",
                capability()
            ),
            None,
            capability(),
        )
        .expect("workspace")
    }

    fn endpoint(path: &str) -> OperationEndpoint {
        OperationEndpoint::new(
            Endpoint::Single,
            path,
            Preimage::Initial {
                observation: DurableObservation::Absent,
                receipt: None,
                owned_basis: None,
            },
            ExpectedAfter::Pending,
        )
        .expect("endpoint")
    }

    fn write(operation_id: u32, workspace_id: Option<u32>, path: &str) -> OperationRecord {
        OperationRecord::new(
            operation_id,
            Vec::new(),
            workspace_id,
            PrivateArtifactSlots::new(
                DurableObservation::Absent,
                DurableObservation::Absent,
                DurableObservation::Absent,
            )
            .expect("slots"),
            OperationEffect::Write(
                WriteEffect::new(endpoint(path), WriteState::Planned).expect("write"),
            ),
        )
        .expect("operation")
    }

    #[test]
    fn many_artifact_operations_can_share_one_workspace() {
        let journal = OptiScalerJournal::new(
            vec!["game".to_owned()],
            control(),
            vec![workspace()],
            vec![
                write(0, Some(0), "game/dxgi.dll"),
                write(1, Some(0), "game/d3d12.dll"),
            ],
        );
        assert!(journal.is_ok());
    }

    #[test]
    fn artifact_free_operations_cannot_claim_a_workspace() {
        let verify = OperationRecord::new(
            0,
            Vec::new(),
            Some(0),
            PrivateArtifactSlots::new(
                DurableObservation::Absent,
                DurableObservation::Absent,
                DurableObservation::Absent,
            )
            .expect("slots"),
            OperationEffect::Verify(
                VerifyEffect::new(endpoint("game/dxgi.dll"), VerifyState::Planned).expect("verify"),
            ),
        );
        assert!(verify.is_err());
    }

    #[test]
    fn workspace_must_be_disjoint_from_every_endpoint_in_both_directions() {
        let mut binding = workspace();
        binding.path = "game/assets".to_owned();
        let child = write(0, Some(0), "game/assets/dxgi.dll");
        assert!(
            OptiScalerJournal::new(
                vec!["game".to_owned()],
                control(),
                vec![binding],
                vec![child]
            )
            .is_err()
        );
    }

    #[test]
    fn control_namespace_must_be_disjoint_from_every_endpoint_in_both_directions() {
        let capability = capability();
        let control_path = format!("transactions/control-tx-{}", capability.as_str());
        let control = ControlNamespaceBinding::new(control_path.clone(), None, capability)
            .expect("control namespace");

        for endpoint_path in [
            control_path.clone(),
            format!("{control_path}/target.bin"),
            "transactions".to_owned(),
        ] {
            let journal = OptiScalerJournal::new(
                vec!["game".to_owned()],
                control.clone(),
                vec![workspace()],
                vec![write(0, Some(0), &endpoint_path)],
            );
            assert!(journal.is_err());
        }
    }

    #[test]
    fn private_workspaces_must_be_lexically_disjoint_in_both_directions() {
        let capability = capability();
        let leaf = format!(
            ".renderpilot-optiscaler-workspace-tx-0-1-{}",
            capability.as_str()
        );
        let parent = format!("game/workspaces/{leaf}");
        let child = format!("{parent}/nested/{leaf}");

        for (first_path, second_path) in [
            (parent.clone(), parent.clone()),
            (parent.clone(), child.clone()),
            (child, parent),
        ] {
            let first = PrivateWorkspaceBinding::new(0, 0, first_path, None, capability.clone())
                .expect("first workspace");
            let second = PrivateWorkspaceBinding::new(1, 0, second_path, None, capability.clone())
                .expect("second workspace");
            let journal = OptiScalerJournal::new(
                vec!["game".to_owned()],
                control(),
                vec![first, second],
                vec![
                    write(0, Some(0), "game/dxgi.dll"),
                    write(1, Some(1), "game/d3d12.dll"),
                ],
            );
            assert!(journal.is_err());
        }
    }

    #[test]
    fn workspace_frontier_is_identity_monotonic_and_zero_workspace_reaches_ready() {
        let verify = OperationRecord::new(
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
                VerifyEffect::new(endpoint("game/dxgi.dll"), VerifyState::Planned).expect("verify"),
            ),
        )
        .expect("operation");
        let mut journal =
            OptiScalerJournal::new(vec!["game".to_owned()], control(), Vec::new(), vec![verify])
                .expect("journal");
        journal
            .control_namespace_mut()
            .set_identity("control")
            .expect("identity");
        journal.set_materialization(MaterializationState::Workspaces {
            next_workspace_id: 0,
        });
        assert!(journal.validate().is_ok());
        journal.set_materialization(MaterializationState::Ready);
        assert!(journal.validate().is_ok());
    }

    #[test]
    fn operation_workspace_reference_must_name_a_declared_workspace() {
        let journal = OptiScalerJournal::new(
            vec!["game".to_owned()],
            control(),
            vec![workspace()],
            vec![write(0, Some(0), "game/dxgi.dll")],
        )
        .expect("journal");
        let mut wire = serde_json::to_value(journal).expect("wire");
        wire["operations"][0]["workspace_id"] = serde_json::json!(1);
        assert!(serde_json::from_value::<OptiScalerJournal>(wire).is_err());
    }
}
