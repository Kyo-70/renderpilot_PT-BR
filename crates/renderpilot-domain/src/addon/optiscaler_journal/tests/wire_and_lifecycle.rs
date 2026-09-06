#[test]
fn journal_is_unversioned_and_strict() {
    let journal = journal_with_operations(vec!["game".to_owned()], namespace(), vec![operation()])
        .expect("journal");
    let value = serde_json::to_value(&journal).expect("serialize");
    assert_eq!(value.get("kind"), Some(&json!("optiscaler")));
    assert!(value.get("version").is_none());
    assert!(value.get("protocol").is_none());

    let mut with_unknown = value;
    with_unknown["unexpected"] = json!(true);
    assert!(serde_json::from_value::<OptiScalerJournal>(with_unknown).is_err());
}

#[test]
fn namespace_capability_and_binding_wire_shape_are_canonical() {
    assert!(NamespaceCapability::new("A".repeat(64)).is_err());
    assert!(NamespaceCapability::new("a".repeat(63)).is_err());
    let control = namespace();
    let encoded_text = serde_json::to_string(&control).expect("serialize control binding");
    assert_eq!(
        encoded_text,
        format!(
            r#"{{"path":"transactions/control-abc-{}","identity":null,"capability":"{}"}}"#,
            capability_text(),
            capability_text()
        )
    );
    let decoded: ControlNamespaceBinding =
        serde_json::from_str(&encoded_text).expect("decode canonical control binding");
    assert_eq!(
        serde_json::to_string(&decoded).expect("re-encode control binding"),
        encoded_text
    );

    let encoded = serde_json::to_value(control).expect("serialize control binding");
    assert_eq!(
        encoded["path"],
        json!(format!("transactions/control-abc-{}", capability_text()))
    );
    assert_eq!(encoded["identity"], json!(null));
    assert_eq!(encoded["capability"], json!("a".repeat(64)));

    let mut obsolete_shape = encoded;
    obsolete_shape["transaction_dir"] = json!("transactions/old");
    assert!(serde_json::from_value::<ControlNamespaceBinding>(obsolete_shape).is_err());

    let private = private_workspace(0);
    let encoded_text = serde_json::to_string(&private).expect("serialize private binding");
    assert_eq!(
        encoded_text,
        format!(
            r#"{{"workspace_id":0,"root_index":0,"path":"game/.renderpilot-optiscaler-workspace-abc-0-{}","identity":null,"capability":"{}"}}"#,
            capability_text(),
            capability_text()
        )
    );
    let decoded: PrivateWorkspaceBinding =
        serde_json::from_str(&encoded_text).expect("decode canonical private binding");
    assert_eq!(
        serde_json::to_string(&decoded).expect("re-encode private binding"),
        encoded_text
    );
}

#[test]
fn binding_constructors_normalize_but_wire_decode_requires_exact_spelling() {
    fn equivalent_spellings(path: &str) -> [String; 5] {
        [
            format!("  {path}  "),
            path.replace('/', "\\"),
            path.to_ascii_uppercase(),
            format!("{path}/"),
            format!(r"\\?\{}", path.replace('/', "\\")),
        ]
    }

    let control_path = format!("transactions/control-abc-{}", capability_text());
    for raw_path in equivalent_spellings(&control_path) {
        let binding =
            ControlNamespaceBinding::new(raw_path.clone(), None, capability()).expect("control");
        assert_eq!(binding.path(), control_path);
        let error = serde_json::from_value::<ControlNamespaceBinding>(json!({
            "path": raw_path,
            "identity": null,
            "capability": capability_text(),
        }))
        .expect_err("noncanonical control wire path");
        assert!(error.to_string().contains("canonical durable spelling"));
    }

    let private_path = private_path(0);
    for raw_path in equivalent_spellings(&private_path) {
        let binding = PrivateWorkspaceBinding::new(0, 0, raw_path.clone(), None, capability())
            .expect("private");
        assert_eq!(binding.path(), private_path);
        let error = serde_json::from_value::<PrivateWorkspaceBinding>(json!({
            "workspace_id": 0,
            "root_index": 0,
            "path": raw_path,
            "identity": null,
            "capability": capability_text(),
        }))
        .expect_err("noncanonical private wire path");
        assert!(error.to_string().contains("canonical durable spelling"));
    }
}

#[test]
fn canonical_binding_paths_preserve_non_ascii_case_and_reject_raw_unicode_whitespace() {
    let unicode_path = format!("transactions/Ä-{}", capability_text());
    let padded = format!("\u{a0}{unicode_path}\u{a0}");

    let control = ControlNamespaceBinding::new(padded.clone(), None, capability())
        .expect("trimmed control path");
    assert_eq!(control.path(), unicode_path);
    let control_json = serde_json::to_string(&control).expect("control JSON");
    let decoded: ControlNamespaceBinding =
        serde_json::from_str(&control_json).expect("canonical Unicode control path");
    assert_eq!(decoded.path(), unicode_path);
    assert_eq!(
        serde_json::to_string(&decoded).expect("control re-encode"),
        control_json
    );
    assert!(
        serde_json::from_value::<ControlNamespaceBinding>(json!({
            "path": padded,
            "identity": null,
            "capability": capability_text(),
        }))
        .is_err()
    );

    let private = PrivateWorkspaceBinding::new(0, 0, unicode_path.clone(), None, capability())
        .expect("Unicode private path");
    assert_eq!(private.path(), unicode_path);
    let private_json = serde_json::to_string(&private).expect("private JSON");
    let decoded: PrivateWorkspaceBinding =
        serde_json::from_str(&private_json).expect("canonical Unicode private path");
    assert_eq!(decoded.path(), unicode_path);
    assert_eq!(
        serde_json::to_string(&decoded).expect("private re-encode"),
        private_json
    );

    let padded = format!("\u{a0}{unicode_path}\u{a0}");
    assert!(
        serde_json::from_value::<PrivateWorkspaceBinding>(json!({
            "workspace_id": 0,
            "root_index": 0,
            "path": padded,
            "identity": null,
            "capability": capability_text(),
        }))
        .is_err()
    );
}

#[test]
fn namespace_paths_keep_structural_rejections() {
    for invalid in [
        "",
        " \t ",
        "\0",
        ".",
        "..",
        "transactions//leaf",
        "transactions/./leaf",
        "transactions/../leaf",
    ] {
        assert!(
            ControlNamespaceBinding::new(invalid, None, capability()).is_err(),
            "accepted invalid namespace path {invalid:?}"
        );
    }
}

#[test]
fn repeated_validation_preserves_all_leaf_invariants() {
    let control = ControlNamespaceBinding::new(
        format!("transactions/control-abc-{}", capability_text()),
        Some("control-identity".to_owned()),
        capability(),
    )
    .expect("control binding");
    let private = PrivateWorkspaceBinding::new(
        0,
        0,
        private_path(0),
        Some("private-identity".to_owned()),
        capability(),
    )
    .expect("private binding");
    let file = DurableObservation::file("file-identity", hash()).expect("file observation");
    let directory =
        DurableObservation::directory("directory-identity").expect("directory observation");
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
    let workspace_cleanup = CleanupState::WorkspaceRemoveIntent {
        workspace_id: 0,
        expected_identity: "participant-identity".to_owned(),
    };
    let control_cleanup = CleanupState::ControlRemoveIntent {
        expected_identity: "control-identity".to_owned(),
    };
    let journal = journal_with_operations(vec!["game".to_owned()], namespace(), vec![operation()])
        .expect("journal");

    for _ in 0..2 {
        control.validate().expect("control remains valid");
        private.validate().expect("private remains valid");
        file.validate().expect("file remains valid");
        directory.validate().expect("directory remains valid");
        endpoint.validate().expect("endpoint remains valid");
        workspace_cleanup
            .validate()
            .expect("workspace cleanup remains valid");
        control_cleanup
            .validate()
            .expect("control cleanup remains valid");
        journal.validate().expect("journal remains valid");
    }
}

#[test]
fn journal_rejects_namespace_leaf_without_ordinal_or_capability_binding() {
    let bad_control = ControlNamespaceBinding::new("transactions/control-abc", None, capability())
        .expect("binding shape is locally valid");
    assert!(
        journal_with_operations(vec!["game".to_owned()], bad_control, vec![operation()]).is_err()
    );

    let bad_workspace =
        PrivateWorkspaceBinding::new(0, 0, "transactions/private-abc-0", None, capability())
            .expect("binding shape is locally valid");
    assert!(
        OptiScalerJournal::new(
            vec!["game".to_owned()],
            namespace(),
            vec![bad_workspace],
            vec![operation()],
        )
        .is_err()
    );
}

fn journal_at_terminal_state(preserved: bool) -> OptiScalerJournal {
    let mut operation = operation();
    let live = DurableObservation::File {
        identity: "file-0".to_owned(),
        digest: hash(),
    };
    if let OperationEffect::Write(effect) = operation.effect_mut() {
        effect
            .endpoint_mut()
            .set_expected_after(ExpectedAfter::Known(live.clone()));
        *effect.state_mut() = if preserved {
            WriteState::Preserved
        } else {
            WriteState::Applied {
                live,
                custody: DurableObservation::Absent,
            }
        };
    }
    let mut journal =
        journal_with_operations(vec!["game".to_owned()], namespace(), vec![operation])
            .expect("journal");
    journal.private_workspaces_mut()[0]
        .set_identity("participant-0")
        .expect("workspace identity");
    journal
        .control_namespace_mut()
        .set_identity("control-0")
        .expect("control identity");
    journal.set_materialization(MaterializationState::Ready);
    journal
}

#[test]
fn preserved_action_accepts_a_pending_or_exact_known_endpoint() {
    let mut pending = operation();
    if let OperationEffect::Write(effect) = pending.effect_mut() {
        *effect.state_mut() = WriteState::Preserved;
    }
    assert!(pending.effect().validate().is_ok());
    let journal = journal_with_operations(vec!["game".to_owned()], namespace(), vec![pending])
        .expect("pending preserved journal");
    assert!(journal.validate().is_ok());

    let known = journal_at_terminal_state(true);
    assert!(known.validate().is_ok());
}

#[test]
fn preserved_relocation_rejects_mixed_pending_and_known_endpoints() {
    let source = OperationEndpoint::new(
        Endpoint::Source,
        "game/source.dll",
        Preimage::Initial {
            observation: DurableObservation::File {
                identity: "source-before".to_owned(),
                digest: hash(),
            },
            receipt: None,
            owned_basis: None,
        },
        ExpectedAfter::Pending,
    )
    .expect("source endpoint");
    let destination = OperationEndpoint::new(
        Endpoint::Destination,
        "game/destination.dll",
        Preimage::Initial {
            observation: DurableObservation::Absent,
            receipt: None,
            owned_basis: None,
        },
        ExpectedAfter::Known(DurableObservation::File {
            identity: "destination-after".to_owned(),
            digest: hash(),
        }),
    )
    .expect("destination endpoint");
    assert!(RelocateEffect::new(source, destination, RelocateState::Preserved).is_err());
}

#[test]
fn applied_verify_can_be_reversed_without_changing_its_exact_observation() {
    let observed = DurableObservation::File {
        identity: "verified".to_owned(),
        digest: hash(),
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
    let mut effect =
        VerifyEffect::new(endpoint, VerifyState::Applied { observed }).expect("applied verify");
    *effect.state_mut() = VerifyState::Preserved;
    assert!(OperationEffect::Verify(effect).validate().is_ok());
}

#[test]
fn lifecycle_predicates_are_boundary_exact() {
    let mut committed = journal_at_terminal_state(false);
    assert!(committed.validate().is_ok());
    assert!(committed.is_finish_boundary_ready());
    assert_eq!(committed.rollback_cursor(), Some(0));
    committed.set_cleanup(CleanupState::Complete);
    assert!(committed.can_delete_after_commit());
    assert!(!committed.can_delete_after_rollback());

    let mut rolled_back = journal_at_terminal_state(true);
    assert!(rolled_back.validate().is_ok());
    assert!(rolled_back.is_rollback_latched());
    assert_eq!(rolled_back.rollback_cursor(), None);
    rolled_back.set_cleanup(CleanupState::Complete);
    assert!(rolled_back.can_delete_after_rollback());
    assert!(!rolled_back.can_delete_after_commit());
}

#[test]
fn journal_deserialization_rechecks_order_and_endpoint_invariants() {
    let journal = journal_with_operations(vec!["game".to_owned()], namespace(), vec![operation()])
        .expect("journal");
    let value = serde_json::to_value(journal).expect("serialize");

    let mut invalid_dependencies = value.clone();
    invalid_dependencies["operations"][0]["parent_dependencies"] = json!([1]);
    assert!(serde_json::from_value::<OptiScalerJournal>(invalid_dependencies).is_err());

    let mut invalid_endpoint = value;
    invalid_endpoint["operations"][0]["effect"]["payload"]["endpoint"]["path"] = json!("");
    assert!(serde_json::from_value::<OptiScalerJournal>(invalid_endpoint).is_err());
}

#[test]
fn journal_deserialization_rejects_noncanonical_nested_binding_paths() {
    let journal = journal_with_operations(vec!["game".to_owned()], namespace(), vec![operation()])
        .expect("journal");
    let value = serde_json::to_value(journal).expect("serialize");

    let mut invalid_control = value.clone();
    let control_path = invalid_control["control_namespace"]["path"]
        .as_str()
        .expect("control path")
        .to_owned();
    invalid_control["control_namespace"]["path"] = json!(format!(
        "{}\\",
        control_path.to_ascii_uppercase().replace('/', "\\")
    ));
    let error = serde_json::from_value::<OptiScalerJournal>(invalid_control)
        .expect_err("noncanonical nested control path");
    assert!(error.to_string().contains("canonical durable spelling"));

    let mut invalid_private = value;
    let private_path = invalid_private["private_workspaces"][0]["path"]
        .as_str()
        .expect("private path")
        .to_owned();
    invalid_private["private_workspaces"][0]["path"] = json!(format!(
        "{}\\",
        private_path.to_ascii_uppercase().replace('/', "\\")
    ));
    let error = serde_json::from_value::<OptiScalerJournal>(invalid_private)
        .expect_err("noncanonical nested private path");
    assert!(error.to_string().contains("canonical durable spelling"));
}

#[test]
fn journal_wire_rejects_equal_or_nested_private_workspaces() {
    let capability = capability();
    let leaf = format!(
        ".renderpilot-optiscaler-workspace-abc-0-1-{}",
        capability.as_str()
    );
    let first =
        PrivateWorkspaceBinding::new(0, 0, format!("game/first/{leaf}"), None, capability.clone())
            .expect("first workspace");
    let second =
        PrivateWorkspaceBinding::new(1, 0, format!("game/second/{leaf}"), None, capability)
            .expect("second workspace");
    let journal = OptiScalerJournal::new(
        vec!["game".to_owned()],
        namespace(),
        vec![first, second],
        vec![
            planned_operation_record(
                0,
                "game/dxgi.dll",
                Preimage::Initial {
                    observation: DurableObservation::Absent,
                    receipt: None,
                    owned_basis: None,
                },
                |endpoint| {
                    OperationEffect::Write(
                        WriteEffect::new(endpoint, WriteState::Planned).expect("write"),
                    )
                },
            ),
            OperationRecord::new(
                1,
                Vec::new(),
                Some(1),
                slots(),
                OperationEffect::Write(
                    WriteEffect::new(
                        planned_endpoint(
                            Endpoint::Single,
                            "game/d3d12.dll",
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
            )
            .expect("second operation"),
        ],
    )
    .expect("disjoint journal");
    let value = serde_json::to_value(journal).expect("serialize journal");
    let first_path = value["private_workspaces"][0]["path"]
        .as_str()
        .expect("first path")
        .to_owned();
    let second_path = value["private_workspaces"][1]["path"]
        .as_str()
        .expect("second path")
        .to_owned();

    for (first_path, second_path) in [
        (first_path.clone(), first_path.clone()),
        (first_path.clone(), format!("{first_path}/nested/{leaf}")),
        (format!("{second_path}/nested/{leaf}"), second_path),
    ] {
        let mut invalid = value.clone();
        invalid["private_workspaces"][0]["path"] = json!(first_path);
        invalid["private_workspaces"][1]["path"] = json!(second_path);
        assert!(serde_json::from_value::<OptiScalerJournal>(invalid).is_err());
    }
}

#[test]
fn journal_wire_rejects_equal_or_nested_control_and_endpoint_paths() {
    let journal = journal_with_operations(vec!["game".to_owned()], namespace(), vec![operation()])
        .expect("journal");
    let value = serde_json::to_value(journal).expect("serialize journal");
    let control_path = value["control_namespace"]["path"]
        .as_str()
        .expect("control path")
        .to_owned();

    for endpoint_path in [
        control_path.clone(),
        format!("{control_path}/target.bin"),
        "transactions".to_owned(),
    ] {
        let mut invalid = value.clone();
        invalid["operations"][0]["effect"]["payload"]["endpoint"]["path"] = json!(endpoint_path);
        assert!(serde_json::from_value::<OptiScalerJournal>(invalid).is_err());
    }
}

#[test]
fn identities_are_nonempty_and_artifacts_reject_uncertain_observation() {
    assert!(
        serde_json::from_value::<DurableObservation>(json!({
            "kind": "file",
            "identity": "",
            "digest": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        }))
        .is_err()
    );
    assert!(
        PrivateArtifactSlots::new(
            DurableObservation::Unreadable,
            DurableObservation::Absent,
            DurableObservation::Absent,
        )
        .is_err()
    );
    assert!(
        PrivateArtifactSlots::new(
            DurableObservation::File {
                identity: "same".to_owned(),
                digest: hash(),
            },
            DurableObservation::File {
                identity: "same".to_owned(),
                digest: hash(),
            },
            DurableObservation::Absent,
        )
        .is_err()
    );
}

#[test]
fn prior_postimage_is_explicit_and_does_not_use_path_lookup() {
    let preimage = Preimage::PriorPostimage {
        operation_id: 0,
        endpoint: Endpoint::Destination,
    };
    let endpoint = OperationEndpoint::new(
        Endpoint::Single,
        "dxgi.dll",
        preimage,
        ExpectedAfter::Known(DurableObservation::File {
            identity: "unix:1".to_owned(),
            digest: hash(),
        }),
    )
    .expect("endpoint");
    let encoded = serde_json::to_value(endpoint).expect("serialize");
    assert_eq!(encoded["preimage"]["kind"], json!("prior_postimage"));
    assert_eq!(encoded["preimage"]["operation_id"], json!(0));
    assert_eq!(encoded["preimage"]["endpoint"], json!("destination"));
}

#[test]
fn initial_receipt_must_match_file_observation() {
    let receipt = FileReceipt::owned("unix:wrong", hash()).expect("receipt");
    let preimage = Preimage::Initial {
        observation: DurableObservation::File {
            identity: "unix:right".to_owned(),
            digest: hash(),
        },
        receipt: Some(receipt),
        owned_basis: None,
    };
    assert!(preimage.validate().is_err());
}

#[test]
fn edited_owned_basis_preserves_identity_without_requiring_current_digest() {
    let current = hash();
    let original = Sha256Hash::new("b".repeat(64)).expect("valid hash");
    let receipt = FileReceipt::reused("unix:1", current).expect("receipt");
    let basis = FileReceipt::owned("unix:1", original).expect("basis");
    let preimage = Preimage::Initial {
        observation: DurableObservation::File {
            identity: "unix:1".to_owned(),
            digest: hash(),
        },
        receipt: Some(receipt),
        owned_basis: Some(basis),
    };
    assert!(preimage.validate().is_ok());
}

#[test]
fn threat_model_is_persisted_without_becoming_a_version() {
    let journal = OptiScalerJournal::new_with_threat_model(
        ThreatModel::HostileSameUid,
        vec!["game".to_owned()],
        namespace(),
        vec![private_workspace(0)],
        vec![operation()],
    )
    .expect("journal");
    let value = serde_json::to_value(journal).expect("serialize");
    assert_eq!(value["threat_model"], json!("hostile_same_uid"));
    assert!(value.get("format_version").is_none());
}

fn planned_endpoint(role: Endpoint, path: &str, preimage: Preimage) -> OperationEndpoint {
    OperationEndpoint::new(role, path, preimage, ExpectedAfter::Pending).expect("endpoint")
}

fn planned_operation_record(
    operation_id: u32,
    path: &str,
    preimage: Preimage,
    effect: fn(OperationEndpoint) -> OperationEffect,
) -> OperationRecord {
    let effect = effect(planned_endpoint(Endpoint::Single, path, preimage));
    let workspace_id = effect.requires_private_workspace().then_some(0);
    OperationRecord::new(operation_id, Vec::new(), workspace_id, slots(), effect)
        .expect("operation")
}
