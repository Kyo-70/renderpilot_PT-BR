use super::*;

#[test]
fn reused_membership_add_same_and_release_are_record_only_edges() {
    let before = peer(&[], &[], Vec::new());
    let reused = managed(
        "reused.dll",
        ManagedFileMode::Reused,
        ManagedFileBaseline::Present { sha256: hash('r') },
        'r',
    );

    let added = peer(&["other.dll"], &[], vec![reused.clone()]);
    let addition = PeerTransitionContract::derive_physical(
        Some(&before),
        Some(&added),
        None,
        None,
        ProxyPeerRoute::DurableDisjoint,
        vec![create("other.dll", 'a', 3)],
    )
    .expect("reused membership addition");
    assert_eq!(addition.intents().len(), 1);

    let unchanged = peer(&["other.dll"], &[], vec![reused]);
    let same = PeerTransitionContract::derive_physical(
        Some(&unchanged),
        Some(&unchanged),
        None,
        None,
        ProxyPeerRoute::DurableDisjoint,
        vec![replace("other.dll", 'b', 3)],
    )
    .expect("unchanged reused membership");
    assert_eq!(same.intents().len(), 1);

    let released = peer(&["other.dll"], &[], Vec::new());
    let release = PeerTransitionContract::derive_physical(
        Some(&unchanged),
        Some(&released),
        None,
        None,
        ProxyPeerRoute::DurableDisjoint,
        vec![replace("other.dll", 'b', 3)],
    )
    .expect("reused membership release");
    assert_eq!(release.intents().len(), 1);

    assert!(matches!(
        PeerTransitionContract::derive_physical(
            Some(&unchanged),
            Some(&released),
            None,
            None,
            ProxyPeerRoute::DurableDisjoint,
            vec![],
        ),
        Err(PeerTransitionError::EmptyIntentSet)
    ));
}

#[test]
fn reused_to_owned_acquisition_requires_sidecar_before_live_replace() {
    let before = peer(
        &[],
        &[],
        vec![managed(
            "reused.dll",
            ManagedFileMode::Reused,
            ManagedFileBaseline::Present { sha256: hash('r') },
            'r',
        )],
    );
    let after = peer(
        &[],
        &[],
        vec![managed(
            "reused.dll",
            ManagedFileMode::Owned,
            ManagedFileBaseline::Present { sha256: hash('r') },
            'i',
        )],
    );
    let acquisition = PeerTransitionContract::derive_physical(
        Some(&before),
        Some(&after),
        None,
        None,
        ProxyPeerRoute::DurableDisjoint,
        vec![
            create("reused.dll.bak", 'r', 3),
            replace("reused.dll", 'i', 4),
        ],
    )
    .expect("reused acquisition");
    assert_eq!(acquisition.intents().len(), 2);
    assert_eq!(acquisition.intents()[0].path(), &path("reused.dll.bak"));
    assert_eq!(
        acquisition.intents()[0].operation(),
        PeerEndpointOperation::Create
    );
    assert_eq!(acquisition.intents()[1].path(), &path("reused.dll"));
    assert_eq!(
        acquisition.intents()[1].operation(),
        PeerEndpointOperation::Replace
    );

    assert!(matches!(
        PeerTransitionContract::derive_physical(
            Some(&before),
            Some(&after),
            None,
            None,
            ProxyPeerRoute::DurableDisjoint,
            vec![
                replace("reused.dll", 'i', 4),
                create("reused.dll.bak", 'r', 3)
            ],
        ),
        Err(PeerTransitionError::PhysicalProgramMismatch(_))
    ));

    let after_with_other = peer(
        &["other.dll"],
        &[],
        vec![managed(
            "reused.dll",
            ManagedFileMode::Owned,
            ManagedFileBaseline::Present { sha256: hash('r') },
            'i',
        )],
    );
    assert!(matches!(
        PeerTransitionContract::derive_physical(
            Some(&before),
            Some(&after_with_other),
            None,
            None,
            ProxyPeerRoute::DurableDisjoint,
            vec![
                create("reused.dll.bak", 'r', 3),
                create("other.dll", 'u', 3),
                replace("reused.dll", 'i', 4),
            ],
        ),
        Err(PeerTransitionError::PhysicalProgramMismatch(_))
    ));

    let wrong_baseline = peer(
        &[],
        &[],
        vec![managed(
            "reused.dll",
            ManagedFileMode::Owned,
            ManagedFileBaseline::Present { sha256: hash('x') },
            'i',
        )],
    );
    assert!(matches!(
        PeerTransitionContract::derive_physical(
            Some(&before),
            Some(&wrong_baseline),
            None,
            None,
            ProxyPeerRoute::DurableDisjoint,
            vec![
                create("reused.dll.bak", 'r', 3),
                replace("reused.dll", 'i', 4)
            ],
        ),
        Err(PeerTransitionError::InvalidManagedModeTransition(_))
    ));

    assert!(matches!(
        PeerTransitionContract::derive_physical(
            Some(&before),
            Some(&after),
            None,
            None,
            ProxyPeerRoute::DurableDisjoint,
            vec![replace("reused.dll", 'i', 4)],
        ),
        Err(PeerTransitionError::PhysicalProgramMismatch(_))
    ));

    let wrong_mode = peer(
        &[],
        &[],
        vec![managed(
            "reused.dll",
            ManagedFileMode::Reused,
            ManagedFileBaseline::Present { sha256: hash('r') },
            'i',
        )],
    );
    assert!(matches!(
        PeerTransitionContract::derive_physical(
            Some(&before),
            Some(&wrong_mode),
            None,
            None,
            ProxyPeerRoute::DurableDisjoint,
            vec![replace("reused.dll", 'i', 4)],
        ),
        Err(PeerTransitionError::ReusedClaimChanged(_))
    ));

    let owned_before = peer(
        &[],
        &[],
        vec![managed(
            "reused.dll",
            ManagedFileMode::Owned,
            ManagedFileBaseline::Absent,
            'r',
        )],
    );
    assert!(matches!(
        PeerTransitionContract::derive_physical(
            Some(&owned_before),
            Some(&wrong_mode),
            None,
            None,
            ProxyPeerRoute::DurableDisjoint,
            vec![replace("reused.dll", 'i', 4)],
        ),
        Err(PeerTransitionError::InvalidManagedModeTransition(_))
    ));
}

#[test]
fn coordinated_routes_allow_reused_membership_changes_with_a_real_endpoint() {
    let before_topology = outer_topology();
    let added_peer = peer(
        &[],
        &[],
        vec![
            managed(
                "ReShade64.dll",
                ManagedFileMode::Owned,
                ManagedFileBaseline::Absent,
                'a',
            ),
            managed(
                "shared.dll",
                ManagedFileMode::Reused,
                ManagedFileBaseline::Present { sha256: hash('r') },
                'r',
            ),
        ],
    );
    let create = PeerTransitionContract::derive_physical(
        None,
        Some(&added_peer),
        Some(&before_topology),
        Some(&planned_observed('a', 3)),
        ProxyPeerRoute::Coordinated(CoordinatedPeerOperation::Create),
        vec![
            create("peer.addon64", 'a', 3),
            topology_intent(PeerEndpointOperation::Create, 'a', 3),
        ],
    )
    .expect("coordinated reused membership addition");
    assert_eq!(create.intents().len(), 2);

    let before_peer = peer(
        &[],
        &[],
        vec![
            managed(
                "ReShade64.dll",
                ManagedFileMode::Owned,
                ManagedFileBaseline::Absent,
                'a',
            ),
            managed(
                "shared.dll",
                ManagedFileMode::Reused,
                ManagedFileBaseline::Present { sha256: hash('r') },
                'r',
            ),
        ],
    );
    let remove_contract = PeerTransitionContract::derive_physical(
        Some(&before_peer),
        None,
        Some(&relocated_topology()),
        Some(&PlannedGameProxyTopology::Exact(outer_topology())),
        ProxyPeerRoute::Coordinated(CoordinatedPeerOperation::Remove),
        vec![
            remove("peer.addon64"),
            topology_intent(PeerEndpointOperation::Remove, 'a', 0),
        ],
    )
    .expect("coordinated reused membership release");
    assert_eq!(remove_contract.intents().len(), 2);
}

#[test]
fn coordinated_reused_host_acquisition_preserves_sidecar_before_downstream_replace() {
    let mut before_topology = relocated_topology();
    before_topology
        .downstream
        .as_mut()
        .expect("downstream")
        .receipt = FileReceipt::reused("reused-downstream", hash('r')).expect("receipt");
    before_topology.validate().expect("reused topology");
    let mut planned = planned_observed('i', 4);
    if let PlannedGameProxyTopology::ObservedOwnedDownstream { root_prestate, .. } = &mut planned {
        *root_prestate = before_topology.root_prestate;
    }

    let before_peer = peer(
        &[],
        &[],
        vec![managed(
            "ReShade64.dll",
            ManagedFileMode::Reused,
            ManagedFileBaseline::Present { sha256: hash('r') },
            'r',
        )],
    );
    let after_peer = peer(
        &[],
        &[],
        vec![managed(
            "ReShade64.dll",
            ManagedFileMode::Owned,
            ManagedFileBaseline::Present { sha256: hash('r') },
            'i',
        )],
    );
    let contract = PeerTransitionContract::derive_physical(
        Some(&before_peer),
        Some(&after_peer),
        Some(&before_topology),
        Some(&planned),
        ProxyPeerRoute::Coordinated(CoordinatedPeerOperation::ReplaceSamePath),
        vec![
            create("ReShade64.dll.bak", 'r', 3),
            topology_intent(PeerEndpointOperation::Replace, 'i', 4),
        ],
    )
    .expect("coordinated host acquisition");
    assert_eq!(contract.intents()[0].path(), &path("ReShade64.dll.bak"));
    assert_eq!(
        contract.intents()[1].role(),
        PeerEndpointRole::TopologyDownstream
    );
    let guards = required_read_guards(
        Some(&before_peer),
        Some(&after_peer),
        Some(&before_topology),
        Some(&planned),
        contract.route(),
        contract.intents(),
    )
    .expect("coordinated acquisition guards");
    assert_eq!(guards.len(), 1);
    assert_eq!(guards[0].sources(), &[PeerReadGuardSource::TopologyOuter]);
}

#[test]
fn coordinated_initial_reused_topology_adoption_binds_receipt_and_sidecar() {
    let mut before_topology = relocated_topology();
    let before_digest = hash('r');
    before_topology
        .downstream
        .as_mut()
        .expect("downstream")
        .receipt =
        FileReceipt::reused("foreign-downstream", before_digest.clone()).expect("receipt");
    before_topology.validate().expect("reused topology");

    let after_peer = peer(
        &[],
        &[],
        vec![managed(
            "ReShade64.dll",
            ManagedFileMode::Owned,
            ManagedFileBaseline::Present {
                sha256: before_digest.clone(),
            },
            'i',
        )],
    );
    let mut planned = planned_observed('i', 4);
    if let PlannedGameProxyTopology::ObservedOwnedDownstream { root_prestate, .. } = &mut planned {
        *root_prestate = before_topology.root_prestate;
    }
    let physical = vec![
        create("peer.addon64", 'p', 3),
        create("ReShade64.dll.bak", 'r', 3),
        topology_intent(PeerEndpointOperation::Replace, 'i', 4),
    ];
    let contract = PeerTransitionContract::derive_physical(
        None,
        Some(&after_peer),
        Some(&before_topology),
        Some(&planned),
        ProxyPeerRoute::Coordinated(CoordinatedPeerOperation::ReplaceSamePath),
        physical.clone(),
    )
    .expect("initial reused topology adoption");
    assert_eq!(contract.intents(), physical.as_slice());
    assert_eq!(contract.intents()[1].role(), PeerEndpointRole::Disjoint);
    assert_eq!(
        contract.intents()[2].role(),
        PeerEndpointRole::TopologyDownstream
    );
    assert_eq!(contract.intents()[1].planned_sha256(), Some(&before_digest));

    let guards = required_read_guards(
        None,
        Some(&after_peer),
        Some(&before_topology),
        Some(&planned),
        contract.route(),
        contract.intents(),
    )
    .expect("initial adoption guards");
    assert_eq!(guards.len(), 1);
    assert_eq!(guards[0].path(), &before_topology.root_slot);
    assert_eq!(guards[0].sources(), &[PeerReadGuardSource::TopologyOuter]);

    let without_sidecar = vec![
        create("peer.addon64", 'p', 3),
        topology_intent(PeerEndpointOperation::Replace, 'i', 4),
    ];
    assert!(
        PeerTransitionContract::derive_physical(
            None,
            Some(&after_peer),
            Some(&before_topology),
            Some(&planned),
            ProxyPeerRoute::Coordinated(CoordinatedPeerOperation::ReplaceSamePath),
            without_sidecar,
        )
        .is_err()
    );

    let wrong_sidecar_path = vec![
        create("peer.addon64", 'p', 3),
        create("ReShade64.dll.backup", 'r', 3),
        topology_intent(PeerEndpointOperation::Replace, 'i', 4),
    ];
    assert!(
        PeerTransitionContract::derive_physical(
            None,
            Some(&after_peer),
            Some(&before_topology),
            Some(&planned),
            ProxyPeerRoute::Coordinated(CoordinatedPeerOperation::ReplaceSamePath),
            wrong_sidecar_path,
        )
        .is_err()
    );

    let reordered = vec![
        create("peer.addon64", 'p', 3),
        topology_intent(PeerEndpointOperation::Replace, 'i', 4),
        create("ReShade64.dll.bak", 'r', 3),
    ];
    assert!(
        PeerTransitionContract::derive_physical(
            None,
            Some(&after_peer),
            Some(&before_topology),
            Some(&planned),
            ProxyPeerRoute::Coordinated(CoordinatedPeerOperation::ReplaceSamePath),
            reordered,
        )
        .is_err()
    );

    let nonadjacent = vec![
        create("peer.addon64", 'p', 3),
        create("other.dll", 'x', 3),
        create("ReShade64.dll.bak", 'r', 3),
        topology_intent(PeerEndpointOperation::Replace, 'i', 4),
    ];
    assert!(
        PeerTransitionContract::derive_physical(
            None,
            Some(&after_peer),
            Some(&before_topology),
            Some(&planned),
            ProxyPeerRoute::Coordinated(CoordinatedPeerOperation::ReplaceSamePath),
            nonadjacent,
        )
        .is_err()
    );

    let wrong_sidecar_digest = vec![
        create("peer.addon64", 'p', 3),
        create("ReShade64.dll.bak", 'x', 3),
        topology_intent(PeerEndpointOperation::Replace, 'i', 4),
    ];
    assert!(
        PeerTransitionContract::derive_physical(
            None,
            Some(&after_peer),
            Some(&before_topology),
            Some(&planned),
            ProxyPeerRoute::Coordinated(CoordinatedPeerOperation::ReplaceSamePath),
            wrong_sidecar_digest,
        )
        .is_err()
    );

    let wrong_baseline = peer(
        &[],
        &[],
        vec![managed(
            "ReShade64.dll",
            ManagedFileMode::Owned,
            ManagedFileBaseline::Present { sha256: hash('x') },
            'i',
        )],
    );
    assert!(
        PeerTransitionContract::derive_physical(
            None,
            Some(&wrong_baseline),
            Some(&before_topology),
            Some(&planned),
            ProxyPeerRoute::Coordinated(CoordinatedPeerOperation::ReplaceSamePath),
            physical.clone(),
        )
        .is_err()
    );

    let wrong_mode = peer(
        &[],
        &[],
        vec![managed(
            "ReShade64.dll",
            ManagedFileMode::Reused,
            ManagedFileBaseline::Present {
                sha256: before_digest.clone(),
            },
            'i',
        )],
    );
    assert!(
        PeerTransitionContract::derive_physical(
            None,
            Some(&wrong_mode),
            Some(&before_topology),
            Some(&planned),
            ProxyPeerRoute::Coordinated(CoordinatedPeerOperation::ReplaceSamePath),
            physical.clone(),
        )
        .is_err()
    );

    let absent_baseline = peer(
        &[],
        &[],
        vec![managed(
            "ReShade64.dll",
            ManagedFileMode::Owned,
            ManagedFileBaseline::Absent,
            'i',
        )],
    );
    assert!(
        PeerTransitionContract::derive_physical(
            None,
            Some(&absent_baseline),
            Some(&before_topology),
            Some(&planned),
            ProxyPeerRoute::Coordinated(CoordinatedPeerOperation::ReplaceSamePath),
            physical.clone(),
        )
        .is_err()
    );

    let mut owned_topology = before_topology.clone();
    owned_topology
        .downstream
        .as_mut()
        .expect("downstream")
        .receipt = FileReceipt::owned("owned-downstream", before_digest).expect("receipt");
    assert!(
        PeerTransitionContract::derive_physical(
            None,
            Some(&after_peer),
            Some(&owned_topology),
            Some(&planned),
            ProxyPeerRoute::Coordinated(CoordinatedPeerOperation::ReplaceSamePath),
            physical.clone(),
        )
        .is_err()
    );

    let mut wrong_path = planned.clone();
    if let PlannedGameProxyTopology::ObservedOwnedDownstream {
        downstream_path, ..
    } = &mut wrong_path
    {
        *downstream_path = path("other.dll");
    }
    assert!(
        PeerTransitionContract::derive_physical(
            None,
            Some(&after_peer),
            Some(&before_topology),
            Some(&wrong_path),
            ProxyPeerRoute::Coordinated(CoordinatedPeerOperation::ReplaceSamePath),
            physical.clone(),
        )
        .is_err()
    );

    let mut wrong_digest = planned.clone();
    if let PlannedGameProxyTopology::ObservedOwnedDownstream { planned_sha256, .. } =
        &mut wrong_digest
    {
        *planned_sha256 = hash('x');
    }
    assert!(
        PeerTransitionContract::derive_physical(
            None,
            Some(&after_peer),
            Some(&before_topology),
            Some(&wrong_digest),
            ProxyPeerRoute::Coordinated(CoordinatedPeerOperation::ReplaceSamePath),
            physical,
        )
        .is_err()
    );

    let wrong_postimage = vec![
        create("peer.addon64", 'p', 3),
        create("ReShade64.dll.bak", 'r', 3),
        topology_intent(PeerEndpointOperation::Replace, 'x', 4),
    ];
    assert!(
        PeerTransitionContract::derive_physical(
            None,
            Some(&after_peer),
            Some(&before_topology),
            Some(&planned),
            ProxyPeerRoute::Coordinated(CoordinatedPeerOperation::ReplaceSamePath),
            wrong_postimage,
        )
        .is_err()
    );
}
