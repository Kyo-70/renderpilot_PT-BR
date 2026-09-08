use super::*;

fn relocation_journal() -> (OptiScalerJournal, FileReceipt, FileReceipt) {
    let capability = capability();
    let source = FileReceipt::owned("relocation-id", hash()).expect("source receipt");
    let destination = FileReceipt::owned("relocation-id", hash()).expect("destination receipt");
    let source_endpoint = OperationEndpoint::new(
        Endpoint::Source,
        "game/source.dll",
        Preimage::Initial {
            observation: file_observation(&source),
            receipt: Some(source.clone()),
            owned_basis: Some(source.clone()),
        },
        ExpectedAfter::Known(DurableObservation::Absent),
    )
    .expect("source endpoint");
    let destination_endpoint = OperationEndpoint::new(
        Endpoint::Destination,
        "game/destination.dll",
        Preimage::Initial {
            observation: DurableObservation::Absent,
            receipt: None,
            owned_basis: None,
        },
        ExpectedAfter::Known(file_observation(&destination)),
    )
    .expect("destination endpoint");
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
        OperationEffect::Relocate(Box::new(
            RelocateEffect::new(
                source_endpoint,
                destination_endpoint,
                RelocateState::Applied {
                    source_after: DurableObservation::Absent,
                    destination_after: file_observation(&destination),
                },
            )
            .expect("relocation effect"),
        )),
    )
    .expect("operation");
    let mut journal = test_journal(
        vec!["game".to_owned()],
        ControlNamespaceBinding::new(
            format!("game/control-relocate-{}", capability.as_str()),
            None,
            capability,
        )
        .expect("control namespace"),
        vec![operation],
    );
    journal
        .control_namespace_mut()
        .set_identity("control-id")
        .expect("control identity");
    journal.set_materialization(MaterializationState::Ready);
    (journal, source, destination)
}

fn occupied_reused_relocation_journal(
    outer_is_absent: bool,
    outer_is_reused: bool,
) -> (OptiScalerJournal, FileReceipt, FileReceipt, FileReceipt) {
    let capability = capability();
    let outer_digest = Sha256Hash::new("c".repeat(64)).expect("outer digest");
    let outer = if outer_is_reused {
        FileReceipt::reused("outer-id", outer_digest).expect("reused outer receipt")
    } else {
        FileReceipt::owned("outer-id", outer_digest).expect("owned outer receipt")
    };
    let peer_before = FileReceipt::reused("peer-id", hash()).expect("peer receipt");
    let peer_after = FileReceipt::reused("peer-id", hash()).expect("peer receipt");
    let outer_release_endpoint = OperationEndpoint::new(
        Endpoint::Single,
        "game/destination.dll",
        if outer_is_absent {
            Preimage::Initial {
                observation: DurableObservation::Absent,
                receipt: None,
                owned_basis: None,
            }
        } else {
            Preimage::Initial {
                observation: file_observation(&outer),
                receipt: Some(outer.clone()),
                owned_basis: (!outer_is_reused).then(|| outer.clone()),
            }
        },
        ExpectedAfter::Known(DurableObservation::Absent),
    )
    .expect("outer-release endpoint");
    let (outer_workspace, outer_release) = if outer_is_absent {
        (
            None,
            OperationEffect::Verify(
                VerifyEffect::new(
                    outer_release_endpoint,
                    VerifyState::Applied {
                        observed: DurableObservation::Absent,
                    },
                )
                .expect("outer absent verify"),
            ),
        )
    } else {
        (
            Some(0),
            OperationEffect::Delete(
                DeleteEffect::new(
                    outer_release_endpoint,
                    DeleteState::Applied {
                        custody: file_observation(&outer),
                    },
                )
                .expect("outer delete"),
            ),
        )
    };
    let outer_release = OperationRecord::new(
        0,
        Vec::new(),
        outer_workspace,
        PrivateArtifactSlots::new(
            DurableObservation::Absent,
            DurableObservation::Absent,
            DurableObservation::Absent,
        )
        .expect("delete slots"),
        outer_release,
    )
    .expect("outer-release operation");
    let source_endpoint = OperationEndpoint::new(
        Endpoint::Source,
        "game/source.dll",
        Preimage::Initial {
            observation: file_observation(&peer_before),
            receipt: Some(peer_before.clone()),
            owned_basis: None,
        },
        ExpectedAfter::Known(DurableObservation::Absent),
    )
    .expect("source endpoint");
    let destination_endpoint = OperationEndpoint::new(
        Endpoint::Destination,
        "game/destination.dll",
        Preimage::PriorPostimage {
            operation_id: 0,
            endpoint: Endpoint::Single,
        },
        ExpectedAfter::Known(file_observation(&peer_after)),
    )
    .expect("destination endpoint");
    let relocate = OperationRecord::new(
        1,
        Vec::new(),
        None,
        PrivateArtifactSlots::new(
            DurableObservation::Absent,
            DurableObservation::Absent,
            DurableObservation::Absent,
        )
        .expect("relocation slots"),
        OperationEffect::Relocate(Box::new(
            RelocateEffect::new(
                source_endpoint,
                destination_endpoint,
                RelocateState::Applied {
                    source_after: DurableObservation::Absent,
                    destination_after: file_observation(&peer_after),
                },
            )
            .expect("relocation effect"),
        )),
    )
    .expect("relocation operation");
    let mut journal = test_journal(
        vec!["game".to_owned()],
        ControlNamespaceBinding::new(
            format!("game/control-occupied-{}", capability.as_str()),
            None,
            capability,
        )
        .expect("control namespace"),
        vec![outer_release, relocate],
    );
    journal
        .control_namespace_mut()
        .set_identity("control-occupied")
        .expect("control identity");
    if !outer_is_absent {
        journal.private_workspaces_mut()[0]
            .set_identity("workspace-occupied")
            .expect("workspace identity");
    }
    journal.set_materialization(MaterializationState::Ready);
    (journal, outer, peer_before, peer_after)
}

#[test]
fn remove_owned_file_accepts_an_exact_relocation_source_fold() {
    let (journal, source, destination) = relocation_journal();
    let source_key = normalized_path_key("game/source.dll");
    let destination_key = normalized_path_key("game/destination.dll");
    let relocation = OptiScalerBoundTransition::Relocate {
        source: source.clone(),
        destination: destination.clone(),
    };
    let binding = OptiScalerAggregateBinding {
        paths: BTreeMap::from([
            (
                source_key.clone(),
                OptiScalerBoundPath {
                    path: "game/source.dll".to_owned(),
                    transition: OptiScalerBoundTransition::RemoveOwnedFile {
                        installed: source,
                        restoration: None,
                        allow_absent: false,
                    },
                },
            ),
            (
                destination_key.clone(),
                OptiScalerBoundPath {
                    path: "game/destination.dll".to_owned(),
                    transition: relocation,
                },
            ),
        ]),
        after_claims: BTreeMap::new(),
        retained_fsr_custody: BTreeMap::new(),
        known_paths: BTreeSet::from([source_key, destination_key]),
        auxiliary: Vec::new(),
        owned_preservations: Vec::new(),
    };
    validate_binding_against_journal(&journal, &binding).expect("exact relocation fold");

    let mut unrelated = binding;
    unrelated
        .paths
        .get_mut(&normalized_path_key("game/destination.dll"))
        .expect("destination binding")
        .transition = OptiScalerBoundTransition::VerifyOwnedFile {
        prior: destination.clone(),
        current: destination,
    };
    assert!(validate_binding_against_journal(&journal, &unrelated).is_err());
}

#[test]
fn relocation_preserves_exact_reused_peer_receipts() {
    let (journal, source, destination) = relocation_journal();
    let source = FileReceipt::reused(source.identity().to_owned(), source.digest().clone())
        .expect("reused source receipt");
    let destination = FileReceipt::reused(
        destination.identity().to_owned(),
        destination.digest().clone(),
    )
    .expect("reused destination receipt");
    let source_key = normalized_path_key("game/source.dll");
    let destination_key = normalized_path_key("game/destination.dll");
    let binding = OptiScalerAggregateBinding {
        paths: BTreeMap::from([
            (
                source_key.clone(),
                OptiScalerBoundPath {
                    path: "game/source.dll".to_owned(),
                    transition: OptiScalerBoundTransition::Relocate {
                        source: source.clone(),
                        destination: destination.clone(),
                    },
                },
            ),
            (
                destination_key.clone(),
                OptiScalerBoundPath {
                    path: "game/destination.dll".to_owned(),
                    transition: OptiScalerBoundTransition::Relocate {
                        source,
                        destination,
                    },
                },
            ),
        ]),
        after_claims: BTreeMap::new(),
        retained_fsr_custody: BTreeMap::new(),
        known_paths: BTreeSet::from([source_key, destination_key]),
        auxiliary: Vec::new(),
        owned_preservations: Vec::new(),
    };
    validate_binding_against_journal(&journal, &binding)
        .expect("reused relocation must preserve identity and digest exactly");
}

#[test]
fn peer_baseline_relocation_requires_explicit_typed_path_and_digest() {
    let (journal, source, destination) = relocation_journal();
    let source_key = normalized_path_key("game/source.dll");
    let destination_key = normalized_path_key("game/destination.dll");
    let binding = OptiScalerAggregateBinding {
        paths: BTreeMap::from([
            (
                source_key.clone(),
                OptiScalerBoundPath {
                    path: "game/source.dll".to_owned(),
                    transition: OptiScalerBoundTransition::RelocatePeerBaseline {
                        digest: source.digest().clone(),
                    },
                },
            ),
            (
                destination_key.clone(),
                OptiScalerBoundPath {
                    path: "game/destination.dll".to_owned(),
                    transition: OptiScalerBoundTransition::RelocatePeerBaseline {
                        digest: destination.digest().clone(),
                    },
                },
            ),
        ]),
        after_claims: BTreeMap::new(),
        retained_fsr_custody: BTreeMap::new(),
        known_paths: BTreeSet::from([source_key.clone(), destination_key.clone()]),
        auxiliary: Vec::new(),
        owned_preservations: Vec::new(),
    };
    validate_binding_against_journal(&journal, &binding)
        .expect("explicit peer baseline relocation must bind exact digest");

    let mut wrong_digest = binding;
    wrong_digest
        .paths
        .get_mut(&source_key)
        .expect("source binding")
        .transition = OptiScalerBoundTransition::RelocatePeerBaseline {
        digest: Sha256Hash::new("d".repeat(64)).expect("wrong digest"),
    };
    assert!(validate_binding_against_journal(&journal, &wrong_digest).is_err());

    let neutral = OptiScalerAggregateBinding {
        paths: BTreeMap::new(),
        after_claims: BTreeMap::new(),
        retained_fsr_custody: BTreeMap::new(),
        known_paths: BTreeSet::from([source_key, destination_key]),
        auxiliary: Vec::new(),
        owned_preservations: Vec::new(),
    };
    assert!(validate_binding_against_journal(&journal, &neutral).is_err());
}

#[test]
fn occupied_relocation_restores_a_reused_peer_after_owned_outer_delete() {
    let (journal, outer, peer_before, peer_after) =
        occupied_reused_relocation_journal(false, false);
    let source_key = normalized_path_key("game/source.dll");
    let destination_key = normalized_path_key("game/destination.dll");
    let binding = OptiScalerAggregateBinding {
        paths: BTreeMap::from([
            (
                source_key.clone(),
                OptiScalerBoundPath {
                    path: "game/source.dll".to_owned(),
                    transition: OptiScalerBoundTransition::Relocate {
                        source: peer_before,
                        destination: peer_after.clone(),
                    },
                },
            ),
            (
                destination_key.clone(),
                OptiScalerBoundPath {
                    path: "game/destination.dll".to_owned(),
                    transition: OptiScalerBoundTransition::RemoveOwnedFile {
                        installed: outer,
                        restoration: Some(peer_after),
                        allow_absent: false,
                    },
                },
            ),
        ]),
        after_claims: BTreeMap::new(),
        retained_fsr_custody: BTreeMap::new(),
        known_paths: BTreeSet::from([source_key, destination_key]),
        auxiliary: Vec::new(),
        owned_preservations: Vec::new(),
    };
    validate_binding_against_journal(&journal, &binding)
        .expect("occupied relocation must preserve the reused peer receipt exactly");
}

#[test]
fn absent_outer_relocation_restores_a_reused_peer_without_resurrecting_the_outer() {
    let (journal, outer, peer_before, peer_after) = occupied_reused_relocation_journal(true, false);
    let source_key = normalized_path_key("game/source.dll");
    let destination_key = normalized_path_key("game/destination.dll");
    let binding = OptiScalerAggregateBinding {
        paths: BTreeMap::from([
            (
                source_key.clone(),
                OptiScalerBoundPath {
                    path: "game/source.dll".to_owned(),
                    transition: OptiScalerBoundTransition::Relocate {
                        source: peer_before,
                        destination: peer_after.clone(),
                    },
                },
            ),
            (
                destination_key.clone(),
                OptiScalerBoundPath {
                    path: "game/destination.dll".to_owned(),
                    transition: OptiScalerBoundTransition::RemoveOwnedFile {
                        installed: outer,
                        restoration: Some(peer_after),
                        allow_absent: true,
                    },
                },
            ),
        ]),
        after_claims: BTreeMap::new(),
        retained_fsr_custody: BTreeMap::new(),
        known_paths: BTreeSet::from([source_key, destination_key]),
        auxiliary: Vec::new(),
        owned_preservations: Vec::new(),
    };
    validate_binding_against_journal(&journal, &binding)
        .expect("absent outer must relocate the exact downstream peer");
}

#[test]
fn exact_reused_outer_relocation_restores_the_exact_reused_peer() {
    let (journal, outer, peer_before, peer_after) = occupied_reused_relocation_journal(false, true);
    let source_key = normalized_path_key("game/source.dll");
    let destination_key = normalized_path_key("game/destination.dll");
    let binding = OptiScalerAggregateBinding {
        paths: BTreeMap::from([
            (
                source_key.clone(),
                OptiScalerBoundPath {
                    path: "game/source.dll".to_owned(),
                    transition: OptiScalerBoundTransition::Relocate {
                        source: peer_before,
                        destination: peer_after.clone(),
                    },
                },
            ),
            (
                destination_key.clone(),
                OptiScalerBoundPath {
                    path: "game/destination.dll".to_owned(),
                    transition: OptiScalerBoundTransition::RemoveReusedFile {
                        installed: outer,
                        restoration: Some(peer_after),
                        allow_absent: true,
                    },
                },
            ),
        ]),
        after_claims: BTreeMap::new(),
        retained_fsr_custody: BTreeMap::new(),
        known_paths: BTreeSet::from([source_key, destination_key]),
        auxiliary: Vec::new(),
        owned_preservations: Vec::new(),
    };
    validate_binding_against_journal(&journal, &binding)
        .expect("exact-adopted Reused outer must restore the exact downstream peer");
}

fn relocation_source_claim_binding(
    source: FileReceipt,
    destination_transition: OptiScalerBoundTransition,
) -> OptiScalerAggregateBinding {
    let source_key = normalized_path_key("game/source.dll");
    let destination_key = normalized_path_key("game/destination.dll");
    OptiScalerAggregateBinding {
        paths: BTreeMap::from([
            (
                source_key.clone(),
                OptiScalerBoundPath {
                    path: "game/source.dll".to_owned(),
                    transition: OptiScalerBoundTransition::RemoveOwnedFile {
                        installed: source,
                        restoration: None,
                        allow_absent: false,
                    },
                },
            ),
            (
                destination_key.clone(),
                OptiScalerBoundPath {
                    path: "game/destination.dll".to_owned(),
                    transition: destination_transition,
                },
            ),
        ]),
        after_claims: BTreeMap::new(),
        retained_fsr_custody: BTreeMap::new(),
        known_paths: BTreeSet::from([source_key, destination_key]),
        auxiliary: Vec::new(),
        owned_preservations: Vec::new(),
    }
}

#[test]
fn remove_owned_file_rejects_a_non_relocate_destination_binding() {
    let (journal, source, destination) = relocation_journal();
    let binding = relocation_source_claim_binding(
        source.clone(),
        OptiScalerBoundTransition::ClaimedFile {
            installed: destination.clone(),
        },
    );
    assert!(validate_binding_against_journal(&journal, &binding).is_err());

    let wrong_identity = FileReceipt::owned("wrong-relocation-id", hash()).expect("receipt");
    let wrong_identity_binding = relocation_source_claim_binding(
        source.clone(),
        OptiScalerBoundTransition::ClaimedFile {
            installed: wrong_identity,
        },
    );
    assert!(validate_binding_against_journal(&journal, &wrong_identity_binding).is_err());

    let wrong_digest = FileReceipt::owned(
        "relocation-id",
        Sha256Hash::new("c".repeat(64)).expect("digest"),
    )
    .expect("receipt");
    let wrong_digest_binding = relocation_source_claim_binding(
        source.clone(),
        OptiScalerBoundTransition::ClaimedFile {
            installed: wrong_digest,
        },
    );
    assert!(validate_binding_against_journal(&journal, &wrong_digest_binding).is_err());

    let unrelated_binding = relocation_source_claim_binding(
        source,
        OptiScalerBoundTransition::VerifyOwnedFile {
            prior: destination.clone(),
            current: destination,
        },
    );
    assert!(validate_binding_against_journal(&journal, &unrelated_binding).is_err());

    let mut missing_destination = binding;
    missing_destination
        .paths
        .remove(&normalized_path_key("game/destination.dll"));
    assert!(validate_binding_against_journal(&journal, &missing_destination).is_err());
}
