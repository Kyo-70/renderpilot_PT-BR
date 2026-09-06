use super::*;
use crate::{
    AddonKind, FileReceipt, GameId, GameProxyTopology, InstalledAddon, ManagedAddonFile,
    ManagedFileBaseline, ManagedFileMode, PathRef, ProxyImplementation, ProxyLink,
    ProxyRootPrestate, Sha256Hash,
};

mod catalog;
mod host_release;
mod metadata;
mod renodx_reshade_ini;
mod reused;
mod reused_membership;

fn hash(byte: char) -> Sha256Hash {
    Sha256Hash::new(format!("{:x}", (byte as u8) % 16).repeat(64)).expect("hash")
}

fn path(value: &str) -> PathRef {
    PathRef::new(format!("C:/Games/Test/{value}")).expect("path")
}

fn game() -> GameId {
    GameId::new("manual:peer-transition").expect("game")
}

fn peer(created: &[&str], backed: &[&str], managed: Vec<ManagedAddonFile>) -> InstalledAddon {
    let mut record = InstalledAddon::new(game(), AddonKind::RenoDx, path("peer.addon64"));
    for value in created {
        record = record.with_created_file(path(value));
    }
    for value in backed {
        record = record.with_backed_up_file(path(value));
    }
    record
        .try_with_managed_files(managed)
        .expect("managed record")
}

fn managed(
    name: &str,
    mode: ManagedFileMode,
    baseline: ManagedFileBaseline,
    digest: char,
) -> ManagedAddonFile {
    let target = path(name);
    match mode {
        ManagedFileMode::Owned => ManagedAddonFile::owned(target, baseline, hash(digest)),
        ManagedFileMode::Reused => ManagedAddonFile::reused(target, hash(digest)),
    }
}

fn image(identity: &str, digest: char, length: u64) -> PeerFileImage {
    PeerFileImage::new(identity, hash(digest), length).expect("image")
}

fn outer_topology() -> GameProxyTopology {
    let root = path("dxgi.dll");
    GameProxyTopology {
        id: "topology:peer-transition".to_owned(),
        game_id: game(),
        root_slot: root.clone(),
        outer: ProxyLink {
            implementation: ProxyImplementation::OptiScaler,
            path: root,
            receipt: FileReceipt::owned("outer", hash('o')).expect("outer receipt"),
        },
        downstream: None,
        downstream_origin: None,
        root_prestate: ProxyRootPrestate::Absent,
    }
}

fn planned_observed(digest: char, length: u64) -> PlannedGameProxyTopology {
    let outer = outer_topology();
    PlannedGameProxyTopology::ObservedOwnedDownstream {
        id: outer.id,
        game_id: outer.game_id,
        root_slot: outer.root_slot.clone(),
        outer: outer.outer,
        implementation: ProxyImplementation::ReShade,
        downstream_path: path("ReShade64.dll"),
        downstream_origin: outer.root_slot,
        root_prestate: outer.root_prestate,
        planned_sha256: hash(digest),
        planned_length: length,
    }
}

fn relocated_topology() -> GameProxyTopology {
    let mut topology = outer_topology();
    topology.downstream = Some(ProxyLink {
        implementation: ProxyImplementation::ReShade,
        path: path("ReShade64.dll"),
        receipt: FileReceipt::owned("native-downstream", hash('a')).expect("downstream receipt"),
    });
    topology.downstream_origin = Some(topology.root_slot.clone());
    topology.root_prestate = ProxyRootPrestate::RelocatedDownstream;
    topology.validate().expect("relocated topology");
    topology
}

fn create(name: &str, digest: char, length: u64) -> PeerEndpointIntent {
    PeerEndpointIntent::create(
        path(name),
        PeerEndpointRole::Disjoint,
        Some(hash(digest)),
        Some(length),
    )
    .expect("create intent")
}

fn replace(name: &str, digest: char, length: u64) -> PeerEndpointIntent {
    PeerEndpointIntent::replace(
        path(name),
        PeerEndpointRole::Disjoint,
        Some(hash(digest)),
        Some(length),
    )
    .expect("replace intent")
}

fn remove(name: &str) -> PeerEndpointIntent {
    PeerEndpointIntent::remove(path(name), PeerEndpointRole::Disjoint).expect("remove intent")
}

fn topology_intent(
    operation: PeerEndpointOperation,
    digest: char,
    length: u64,
) -> PeerEndpointIntent {
    PeerEndpointIntent::new(
        path("ReShade64.dll"),
        PeerEndpointRole::TopologyDownstream,
        operation,
        (operation != PeerEndpointOperation::Remove).then(|| hash(digest)),
        (operation != PeerEndpointOperation::Remove).then_some(length),
    )
    .expect("topology intent")
}

fn intent_for<'a>(contract: &'a PeerTransitionContract, name: &str) -> &'a PeerEndpointIntent {
    contract
        .intents()
        .iter()
        .find(|intent| intent.path().as_str().ends_with(name))
        .expect("intent")
}

#[test]
fn topology_free_add_and_remove_keep_the_addon_file_claim() {
    let installed = peer(&["host.dll"], &[], Vec::new());
    let create = PeerTransitionContract::derive_physical(
        None,
        Some(&installed),
        None,
        None,
        ProxyPeerRoute::DurableDisjoint,
        vec![create("peer.addon64", 'a', 3), create("host.dll", 'b', 4)],
    )
    .expect("create");
    assert_eq!(create.intents().len(), 2);
    assert_eq!(
        intent_for(&create, "peer.addon64").operation(),
        PeerEndpointOperation::Create
    );

    let remove_contract = PeerTransitionContract::derive_physical(
        Some(&installed),
        None,
        None,
        None,
        ProxyPeerRoute::DurableDisjoint,
        vec![remove("peer.addon64"), remove("host.dll")],
    )
    .expect("remove");
    assert_eq!(remove_contract.intents().len(), 2);
    assert_eq!(
        intent_for(&remove_contract, "host.dll").operation(),
        PeerEndpointOperation::Remove
    );
}

#[test]
fn canonical_addon_duplicate_is_one_endpoint() {
    let addon = peer(&["peer.addon64"], &[], Vec::new());
    let contract = PeerTransitionContract::derive_physical(
        None,
        Some(&addon),
        None,
        None,
        ProxyPeerRoute::DurableDisjoint,
        vec![create("peer.addon64", 'a', 3)],
    )
    .expect("duplicate addon claim is normalized");
    assert_eq!(contract.intents().len(), 1);
}

#[test]
fn unchanged_exact_topology_is_preserved_on_a_disjoint_route() {
    let topology = outer_topology();
    let installed = peer(&["payload.dll"], &[], Vec::new());
    let contract = PeerTransitionContract::derive_physical(
        None,
        Some(&installed),
        Some(&topology),
        Some(&PlannedGameProxyTopology::Exact(topology.clone())),
        ProxyPeerRoute::DurableDisjoint,
        vec![
            create("peer.addon64", 'a', 3),
            create("payload.dll", 'b', 4),
        ],
    )
    .expect("unchanged exact topology");
    assert!(
        contract
            .intents()
            .iter()
            .all(|intent| intent.role() == PeerEndpointRole::Disjoint)
    );
}

#[test]
fn backed_capture_and_restore_are_paired() {
    let captured = peer(&["host.dll"], &["host.dll"], Vec::new());
    let capture = PeerTransitionContract::derive_physical(
        None,
        Some(&captured),
        None,
        None,
        ProxyPeerRoute::DurableDisjoint,
        vec![
            create("peer.addon64", 'a', 3),
            replace("host.dll", 'n', 4),
            create("host.dll.bak", 'a', 3),
        ],
    )
    .expect("capture");
    let evidence = capture
        .intents()
        .iter()
        .map(|intent| {
            if intent.operation() == PeerEndpointOperation::Create {
                let digest = if intent.path().as_str().ends_with(".bak")
                    || intent.path().as_str().ends_with(".addon64")
                {
                    'a'
                } else {
                    'n'
                };
                PeerEndpointEvidence::new(intent.clone(), None, Some(image("created", digest, 3)))
            } else {
                PeerEndpointEvidence::new(
                    intent.clone(),
                    Some(image("old", 'a', 3)),
                    Some(image("new", 'n', 4)),
                )
            }
        })
        .collect::<Vec<_>>();
    capture
        .validate_evidence(&evidence)
        .expect("capture evidence");

    let restored = peer(&["host.dll"], &[], Vec::new());
    let restore = PeerTransitionContract::derive_physical(
        Some(&captured),
        Some(&restored),
        None,
        None,
        ProxyPeerRoute::DurableDisjoint,
        vec![replace("host.dll", 'a', 3), remove("host.dll.bak")],
    )
    .expect("restore");
    assert_eq!(
        intent_for(&restore, "host.dll.bak").operation(),
        PeerEndpointOperation::Remove
    );
}

#[test]
fn managed_owned_and_present_baselines_emit_typed_edges() {
    let present = peer(
        &[],
        &[],
        vec![managed(
            "managed.dll",
            ManagedFileMode::Owned,
            ManagedFileBaseline::Present { sha256: hash('b') },
            'a',
        )],
    );
    let contract = PeerTransitionContract::derive_physical(
        None,
        Some(&present),
        None,
        None,
        ProxyPeerRoute::DurableDisjoint,
        vec![
            create("peer.addon64", 'a', 3),
            replace("managed.dll", 'a', 4),
            create("managed.dll.bak", 'b', 3),
        ],
    )
    .expect("managed capture");
    assert_eq!(contract.intents().len(), 3);
    assert_eq!(
        intent_for(&contract, "managed.dll").operation(),
        PeerEndpointOperation::Replace
    );

    let restored = PeerTransitionContract::derive_physical(
        Some(&present),
        None,
        None,
        None,
        ProxyPeerRoute::DurableDisjoint,
        vec![
            remove("peer.addon64"),
            replace("managed.dll", 'b', 3),
            remove("managed.dll.bak"),
        ],
    )
    .expect("managed restore");
    assert_eq!(
        intent_for(&restored, "managed.dll.bak").operation(),
        PeerEndpointOperation::Remove
    );
}

#[test]
fn stable_generic_replace_must_be_explicit_and_non_noop() {
    let before = peer(&["stable.dll"], &[], Vec::new());
    let stable = PeerTransitionContract::derive_physical(
        Some(&before),
        Some(&before),
        None,
        None,
        ProxyPeerRoute::DurableDisjoint,
        vec![replace("stable.dll", 'd', 9)],
    )
    .expect("explicit stable replace");
    stable
        .validate_preimages(&[Some(image("before", 'c', 8))])
        .expect("changed stable preimage");
    assert!(matches!(
        stable.validate_preimages(&[Some(image("same", 'd', 9))]),
        Err(PeerTransitionError::NoopEndpoint(_))
    ));

    assert!(matches!(
        PeerTransitionContract::derive_physical(
            Some(&before),
            Some(&before),
            None,
            None,
            ProxyPeerRoute::DurableDisjoint,
            vec![],
        ),
        Err(PeerTransitionError::EmptyIntentSet)
    ));
}

#[test]
fn stable_backed_live_replace_is_explicit_without_sidecar_change() {
    let before = peer(&["stable-backed.dll"], &["stable-backed.dll"], Vec::new());
    let contract = PeerTransitionContract::derive_physical(
        Some(&before),
        Some(&before),
        None,
        None,
        ProxyPeerRoute::DurableDisjoint,
        vec![replace("stable-backed.dll", 'd', 9)],
    )
    .expect("stable backed live replace");

    assert_eq!(contract.intents().len(), 1);
    assert_eq!(
        contract.intents()[0].operation(),
        PeerEndpointOperation::Replace
    );
    contract
        .validate_preimages(&[Some(image("stable-backed", 'c', 8))])
        .expect("changed stable backed preimage");
}

#[test]
fn stable_generic_missing_endpoint_repair_is_explicit_and_typed() {
    let before = peer(&["missing.dll"], &[], Vec::new());
    let contract = PeerTransitionContract::derive_physical(
        Some(&before),
        Some(&before),
        None,
        None,
        ProxyPeerRoute::DurableDisjoint,
        vec![create("missing.dll", 'd', 9)],
    )
    .expect("stable generic repair");

    assert_eq!(contract.intents().len(), 1);
    assert_eq!(contract.intents()[0].planned_sha256(), Some(&hash('d')));
    assert_eq!(contract.intents()[0].planned_length(), Some(9));
    assert!(matches!(
        contract.validate_preimages(&[Some(image("unexpected", 'd', 9))]),
        Err(PeerTransitionError::InvalidPreimage(_))
    ));
}

#[test]
fn stable_generic_repair_requires_digest_and_length() {
    let before = peer(&["missing.dll"], &[], Vec::new());
    let intent =
        PeerEndpointIntent::create(path("missing.dll"), PeerEndpointRole::Disjoint, None, None)
            .expect("untyped create");
    assert!(matches!(
        PeerTransitionContract::derive_physical(
            Some(&before),
            Some(&before),
            None,
            None,
            ProxyPeerRoute::DurableDisjoint,
            vec![intent],
        ),
        Err(PeerTransitionError::UnclaimedPhysicalEndpoint(_))
    ));
}

#[test]
fn stable_generic_remove_is_not_a_retained_mutation() {
    let before = peer(&["stable.dll"], &[], Vec::new());
    assert!(matches!(
        PeerTransitionContract::derive_physical(
            Some(&before),
            Some(&before),
            None,
            None,
            ProxyPeerRoute::DurableDisjoint,
            vec![remove("stable.dll")],
        ),
        Err(PeerTransitionError::UnclaimedPhysicalEndpoint(_))
    ));
}

#[test]
fn stable_generic_replacements_follow_caller_order() {
    let before = peer(&["first.dll", "second.dll"], &[], Vec::new());
    let contract = PeerTransitionContract::derive_physical(
        Some(&before),
        Some(&before),
        None,
        None,
        ProxyPeerRoute::DurableDisjoint,
        vec![replace("second.dll", 'e', 8), replace("first.dll", 'd', 7)],
    )
    .expect("stable replacements");

    assert_eq!(
        contract
            .intents()
            .iter()
            .map(|intent| intent.path().file_name().expect("file name"))
            .collect::<Vec<_>>(),
        vec!["second.dll", "first.dll"]
    );
}

#[test]
fn two_present_peer_records_must_keep_game_and_kind_identity() {
    let before = peer(&["stable.dll"], &[], Vec::new());
    let other_game = InstalledAddon::new(
        GameId::new("manual:other-peer-transition").expect("game"),
        AddonKind::RenoDx,
        path("peer.addon64"),
    )
    .with_created_file(path("stable.dll"));
    assert!(matches!(
        PeerTransitionContract::derive_physical(
            Some(&before),
            Some(&other_game),
            None,
            None,
            ProxyPeerRoute::DurableDisjoint,
            vec![replace("stable.dll", 'd', 9)],
        ),
        Err(PeerTransitionError::InvalidPeerSnapshot(
            "peer records belong to different games"
        ))
    ));

    let other_kind = InstalledAddon::new(game(), AddonKind::Luma, path("peer.addon64"))
        .with_created_file(path("stable.dll"));
    assert!(matches!(
        PeerTransitionContract::derive_physical(
            Some(&before),
            Some(&other_kind),
            None,
            None,
            ProxyPeerRoute::DurableDisjoint,
            vec![replace("stable.dll", 'd', 9)],
        ),
        Err(PeerTransitionError::InvalidPeerSnapshot(
            "peer kind cannot change in one transition"
        ))
    ));
}

#[test]
fn reused_membership_requires_another_real_disjoint_edge() {
    let reused = peer(
        &[],
        &[],
        vec![managed(
            "reused.dll",
            ManagedFileMode::Reused,
            ManagedFileBaseline::Present { sha256: hash('r') },
            'r',
        )],
    );
    let released = peer(&["other.dll"], &[], Vec::new());
    let route = PeerTransitionContract::derive_physical(
        Some(&reused),
        Some(&released),
        None,
        None,
        ProxyPeerRoute::DurableDisjoint,
        vec![create("other.dll", 'a', 3)],
    )
    .expect("paired release");
    assert_eq!(route.intents().len(), 1);
}

#[test]
fn coordinated_observed_create_replace_and_exact_remove_materialize() {
    let before_topology = outer_topology();
    let after_peer = peer(
        &[],
        &[],
        vec![managed(
            "ReShade64.dll",
            ManagedFileMode::Owned,
            ManagedFileBaseline::Absent,
            'a',
        )],
    );
    let create_intent = topology_intent(PeerEndpointOperation::Create, 'a', 3);
    let planned = planned_observed('a', 3);
    let create_contract = PeerTransitionContract::derive_physical(
        None,
        Some(&after_peer),
        Some(&before_topology),
        Some(&planned),
        ProxyPeerRoute::Coordinated(CoordinatedPeerOperation::Create),
        vec![create("peer.addon64", 'a', 3), create_intent.clone()],
    )
    .expect("coordinated create");
    let evidence = vec![
        PeerEndpointEvidence::new(
            create("peer.addon64", 'a', 3),
            None,
            Some(image("addon", 'a', 3)),
        ),
        PeerEndpointEvidence::new(
            create_intent,
            None,
            Some(image("native-downstream", 'a', 3)),
        ),
    ];
    let materialized = planned
        .materialize_from_evidence(
            create_contract.route(),
            create_contract.intents(),
            &evidence,
        )
        .expect("materialize observed topology");
    assert_eq!(
        materialized
            .downstream
            .as_ref()
            .expect("downstream")
            .receipt
            .identity(),
        "native-downstream"
    );

    let before_peer = after_peer;
    let replaced_peer = peer(
        &[],
        &[],
        vec![managed(
            "ReShade64.dll",
            ManagedFileMode::Owned,
            ManagedFileBaseline::Absent,
            'b',
        )],
    );
    let replace_intent = topology_intent(PeerEndpointOperation::Replace, 'b', 4);
    let replace_plan = planned_observed('b', 4);
    let replace = PeerTransitionContract::derive_physical(
        Some(&before_peer),
        Some(&replaced_peer),
        Some(&materialized),
        Some(&replace_plan),
        ProxyPeerRoute::Coordinated(CoordinatedPeerOperation::ReplaceSamePath),
        vec![replace_intent.clone()],
    )
    .expect("coordinated replace");
    replace
        .validate_evidence(&[PeerEndpointEvidence::new(
            replace_intent,
            Some(image("native-downstream", 'a', 3)),
            Some(image("new-native", 'b', 4)),
        )])
        .expect("replace evidence");

    let remove_topology = outer_topology();
    let remove_intent = topology_intent(PeerEndpointOperation::Remove, 'a', 0);
    let remove_plan = PlannedGameProxyTopology::Exact(remove_topology.clone());
    let remove_contract = PeerTransitionContract::derive_physical(
        Some(&before_peer),
        None,
        Some(&materialized),
        Some(&remove_plan),
        ProxyPeerRoute::Coordinated(CoordinatedPeerOperation::Remove),
        vec![remove("peer.addon64"), remove_intent.clone()],
    )
    .expect("coordinated remove");
    let restored = remove_plan
        .materialize_from_evidence(
            remove_contract.route(),
            remove_contract.intents(),
            &[
                PeerEndpointEvidence::new(
                    remove("peer.addon64"),
                    Some(image("addon", 'a', 3)),
                    None,
                ),
                PeerEndpointEvidence::new(remove_intent, Some(image("new-native", 'a', 3)), None),
            ],
        )
        .expect("exact remove topology");
    assert_eq!(restored, remove_topology);
}

#[test]
fn coordinated_remove_allows_owned_relocated_downstream_to_return_to_outer_only() {
    let before_topology = relocated_topology();
    let before_peer = peer(
        &[],
        &[],
        vec![managed(
            "ReShade64.dll",
            ManagedFileMode::Owned,
            ManagedFileBaseline::Absent,
            'a',
        )],
    );
    let after_topology = outer_topology();
    let contract = PeerTransitionContract::derive_physical(
        Some(&before_peer),
        None,
        Some(&before_topology),
        Some(&PlannedGameProxyTopology::Exact(after_topology)),
        ProxyPeerRoute::Coordinated(CoordinatedPeerOperation::Remove),
        vec![
            remove("peer.addon64"),
            topology_intent(PeerEndpointOperation::Remove, 'a', 0),
        ],
    )
    .expect("owned relocated downstream remove");

    assert_eq!(contract.intents().len(), 2);
    assert_eq!(
        intent_for(&contract, "ReShade64.dll").operation(),
        PeerEndpointOperation::Remove
    );
}

#[test]
fn coordinated_remove_rejects_non_owned_or_unmanaged_downstream() {
    let before_topology = relocated_topology();
    let after_topology = outer_topology();
    let remove_plan = PlannedGameProxyTopology::Exact(after_topology);
    let physical_program = vec![
        remove("peer.addon64"),
        topology_intent(PeerEndpointOperation::Remove, 'a', 0),
    ];

    let non_owned_topology = {
        let mut topology = before_topology.clone();
        topology.downstream.as_mut().expect("downstream").receipt =
            FileReceipt::reused("external-downstream", hash('a')).expect("reused receipt");
        topology.validate().expect("non-owned topology");
        topology
    };
    let owned_peer = peer(
        &[],
        &[],
        vec![managed(
            "ReShade64.dll",
            ManagedFileMode::Owned,
            ManagedFileBaseline::Absent,
            'a',
        )],
    );
    assert!(matches!(
        PeerTransitionContract::derive_physical(
            Some(&owned_peer),
            None,
            Some(&non_owned_topology),
            Some(&remove_plan),
            ProxyPeerRoute::Coordinated(CoordinatedPeerOperation::Remove),
            physical_program.clone(),
        ),
        Err(PeerTransitionError::InvalidManagedDownstream(_))
    ));

    let unmanaged_peer = peer(&[], &[], Vec::new());
    assert!(matches!(
        PeerTransitionContract::derive_physical(
            Some(&unmanaged_peer),
            None,
            Some(&before_topology),
            Some(&remove_plan),
            ProxyPeerRoute::Coordinated(CoordinatedPeerOperation::Remove),
            physical_program,
        ),
        Err(PeerTransitionError::InvalidManagedDownstream(_))
    ));
}

#[test]
fn coordinated_remove_rejects_a_downstream_in_the_planned_after_topology() {
    let before_topology = relocated_topology();
    let mut wrong_after = outer_topology();
    wrong_after.downstream = before_topology.downstream.clone();
    wrong_after.downstream_origin = Some(wrong_after.root_slot.clone());
    wrong_after.validate().expect("wrong after topology");
    let before_peer = peer(
        &[],
        &[],
        vec![managed(
            "ReShade64.dll",
            ManagedFileMode::Owned,
            ManagedFileBaseline::Absent,
            'a',
        )],
    );

    assert!(matches!(
        PeerTransitionContract::derive_physical(
            Some(&before_peer),
            None,
            Some(&before_topology),
            Some(&PlannedGameProxyTopology::Exact(wrong_after)),
            ProxyPeerRoute::Coordinated(CoordinatedPeerOperation::Remove),
            vec![
                remove("peer.addon64"),
                topology_intent(PeerEndpointOperation::Remove, 'a', 0)
            ],
        ),
        Err(PeerTransitionError::InvalidPlannedTopology(_))
    ));
}

#[test]
fn coordinated_replace_still_rejects_a_root_prestate_change() {
    let before_topology = relocated_topology();
    let before_peer = peer(
        &[],
        &[],
        vec![managed(
            "ReShade64.dll",
            ManagedFileMode::Owned,
            ManagedFileBaseline::Absent,
            'a',
        )],
    );
    let after_peer = peer(
        &[],
        &[],
        vec![managed(
            "ReShade64.dll",
            ManagedFileMode::Owned,
            ManagedFileBaseline::Absent,
            'b',
        )],
    );

    assert!(matches!(
        PeerTransitionContract::derive_physical(
            Some(&before_peer),
            Some(&after_peer),
            Some(&before_topology),
            Some(&planned_observed('b', 4)),
            ProxyPeerRoute::Coordinated(CoordinatedPeerOperation::ReplaceSamePath),
            vec![topology_intent(PeerEndpointOperation::Replace, 'b', 4)],
        ),
        Err(PeerTransitionError::ImmutableTopology)
    ));
}

#[test]
fn route_shape_and_observed_materialization_fail_closed() {
    let before = outer_topology();
    let exact = PlannedGameProxyTopology::Exact(before.clone());
    let bad_disjoint = PeerTransitionContract::derive_physical(
        None,
        None,
        None,
        Some(&exact),
        ProxyPeerRoute::DurableDisjoint,
        vec![create("x.dll", 'a', 1)],
    );
    assert!(matches!(
        bad_disjoint,
        Err(PeerTransitionError::InvalidPlannedTopology(_))
    ));

    let intent = topology_intent(PeerEndpointOperation::Create, 'a', 3);
    let plan = planned_observed('a', 3);
    assert!(matches!(
        plan.materialize_from_evidence(
            ProxyPeerRoute::Coordinated(CoordinatedPeerOperation::Create),
            std::slice::from_ref(&intent),
            &[PeerEndpointEvidence::new(
                intent.clone(),
                None,
                Some(image("x", 'b', 3))
            )],
        ),
        Err(PeerTransitionError::DigestMismatch(_))
    ));
    assert!(matches!(
        plan.materialize_from_evidence(
            ProxyPeerRoute::Coordinated(CoordinatedPeerOperation::Create),
            std::slice::from_ref(&intent),
            &[PeerEndpointEvidence::new(
                intent.clone(),
                None,
                Some(image("x", 'a', 2))
            )],
        ),
        Err(PeerTransitionError::LengthMismatch(_))
    ));
    assert!(matches!(
        plan.materialize_from_evidence(
            ProxyPeerRoute::Coordinated(CoordinatedPeerOperation::Create),
            std::slice::from_ref(&intent),
            &[],
        ),
        Err(PeerTransitionError::EvidenceCardinality { .. })
    ));
    let wrong_path = PeerEndpointIntent::create(
        path("other.dll"),
        PeerEndpointRole::TopologyDownstream,
        Some(hash('a')),
        Some(3),
    )
    .expect("intent");
    assert!(matches!(
        plan.materialize_from_evidence(
            ProxyPeerRoute::Coordinated(CoordinatedPeerOperation::Create),
            std::slice::from_ref(&wrong_path),
            &[PeerEndpointEvidence::new(
                wrong_path.clone(),
                None,
                Some(image("x", 'a', 3))
            )],
        ),
        Err(PeerTransitionError::PhysicalProgramMismatch(_))
    ));
    assert!(matches!(
        exact.materialize_from_evidence(
            ProxyPeerRoute::Coordinated(CoordinatedPeerOperation::Create),
            std::slice::from_ref(&intent),
            &[PeerEndpointEvidence::new(
                intent.clone(),
                None,
                Some(image("x", 'a', 3))
            )],
        ),
        Err(PeerTransitionError::TopologyMaterializationMismatch(_))
    ));
    let unknown = PlannedGameProxyTopology::ObservedOwnedDownstream {
        id: before.id,
        game_id: before.game_id,
        root_slot: before.root_slot.clone(),
        outer: before.outer,
        implementation: ProxyImplementation::SpecialK,
        downstream_path: path("ReShade64.dll"),
        downstream_origin: before.root_slot,
        root_prestate: before.root_prestate,
        planned_sha256: hash('a'),
        planned_length: 3,
    };
    assert!(matches!(
        unknown.materialize_from_evidence(
            ProxyPeerRoute::Coordinated(CoordinatedPeerOperation::Create),
            std::slice::from_ref(&intent),
            &[PeerEndpointEvidence::new(
                intent.clone(),
                None,
                Some(image("x", 'a', 3))
            )],
        ),
        Err(PeerTransitionError::InvalidPlannedTopology(_))
    ));
}

#[test]
fn generic_managed_and_sidecar_overlap_is_rejected() {
    let managed_file = managed(
        "managed.dll",
        ManagedFileMode::Owned,
        ManagedFileBaseline::Absent,
        'a',
    );
    let conflict = InstalledAddon::new(game(), AddonKind::RenoDx, path("peer.addon64"))
        .with_created_file(path("managed.dll"))
        .try_with_managed_files(vec![managed_file])
        .expect_err("base addon rejects generic/managed overlap");
    assert!(matches!(
        conflict,
        crate::InstalledAddonInvariantError::ManagedPathOwnedByEngine(_)
    ));
}

#[test]
fn evidence_requires_order_and_typed_postimage() {
    let first = create("a.dll", 'a', 1);
    let second = create("b.dll", 'b', 1);
    let swapped = vec![
        PeerEndpointEvidence::new(second.clone(), None, Some(image("b", 'b', 1))),
        PeerEndpointEvidence::new(first.clone(), None, Some(image("a", 'a', 1))),
    ];
    assert!(matches!(
        validate_evidence(&[first.clone(), second], &swapped),
        Err(PeerTransitionError::EvidenceOrderMismatch(_))
    ));
    assert!(matches!(
        validate_evidence(
            std::slice::from_ref(&first),
            &[PeerEndpointEvidence::new(
                first.clone(),
                None,
                Some(image("a", 'b', 1))
            )]
        ),
        Err(PeerTransitionError::DigestMismatch(_))
    ));
}
