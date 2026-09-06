use super::*;

#[test]
fn present_baseline_release_requires_adjacent_restore_pair() {
    let before_topology = relocated_topology();
    let before_peer = peer(
        &[],
        &[],
        vec![managed(
            "ReShade64.dll",
            ManagedFileMode::Owned,
            ManagedFileBaseline::Present { sha256: hash('b') },
            'a',
        )],
    );
    let after_topology = PlannedGameProxyTopology::Exact(outer_topology());
    let sidecar = remove("ReShade64.dll.bak");
    let live = topology_intent(PeerEndpointOperation::Replace, 'b', 3);
    let contract = PeerTransitionContract::derive_physical(
        Some(&before_peer),
        None,
        Some(&before_topology),
        Some(&after_topology),
        ProxyPeerRoute::Coordinated(CoordinatedPeerOperation::Remove),
        vec![remove("peer.addon64"), live.clone(), sidecar.clone()],
    )
    .expect("present baseline release");
    assert_eq!(contract.intents()[1], live);
    assert_eq!(contract.intents()[2], sidecar);

    let evidence = vec![
        PeerEndpointEvidence::new(remove("peer.addon64"), Some(image("addon", 'a', 3)), None),
        PeerEndpointEvidence::new(
            contract.intents()[1].clone(),
            Some(image("native-downstream", 'a', 3)),
            Some(image("restored", 'b', 3)),
        ),
        PeerEndpointEvidence::new(
            contract.intents()[2].clone(),
            Some(image("sidecar", 'b', 2)),
            None,
        ),
    ];
    assert!(matches!(
        contract.validate_evidence(&evidence),
        Err(PeerTransitionError::PairedImageMismatch(_))
    ));

    assert!(matches!(
        PeerTransitionContract::derive_physical(
            Some(&before_peer),
            None,
            Some(&before_topology),
            Some(&after_topology),
            ProxyPeerRoute::Coordinated(CoordinatedPeerOperation::Remove),
            vec![remove("peer.addon64"), sidecar.clone(), live.clone()],
        ),
        Err(PeerTransitionError::PhysicalProgramMismatch(_))
    ));

    let before_with_gap = peer(
        &["unrelated.dll"],
        &[],
        vec![managed(
            "ReShade64.dll",
            ManagedFileMode::Owned,
            ManagedFileBaseline::Present { sha256: hash('b') },
            'a',
        )],
    );
    assert!(matches!(
        PeerTransitionContract::derive_physical(
            Some(&before_with_gap),
            None,
            Some(&before_topology),
            Some(&after_topology),
            ProxyPeerRoute::Coordinated(CoordinatedPeerOperation::Remove),
            vec![
                remove("peer.addon64"),
                live.clone(),
                remove("unrelated.dll"),
                sidecar,
            ],
        ),
        Err(PeerTransitionError::PhysicalProgramMismatch(_))
    ));

    assert!(matches!(
        PeerTransitionContract::derive_physical(
            Some(&before_peer),
            None,
            Some(&before_topology),
            Some(&after_topology),
            ProxyPeerRoute::Coordinated(CoordinatedPeerOperation::Remove),
            vec![remove("peer.addon64"), live],
        ),
        Err(PeerTransitionError::PhysicalProgramMismatch(_))
    ));
}

#[test]
fn absent_baseline_release_is_one_live_remove_without_sidecar() {
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
    let after_topology = PlannedGameProxyTopology::Exact(outer_topology());
    let live = topology_intent(PeerEndpointOperation::Remove, 'a', 0);
    let contract = PeerTransitionContract::derive_physical(
        Some(&before_peer),
        None,
        Some(&before_topology),
        Some(&after_topology),
        ProxyPeerRoute::Coordinated(CoordinatedPeerOperation::Remove),
        vec![remove("peer.addon64"), live.clone()],
    )
    .expect("absent baseline release");
    assert_eq!(contract.intents()[1], live);

    assert!(matches!(
        PeerTransitionContract::derive_physical(
            Some(&before_peer),
            None,
            Some(&before_topology),
            Some(&after_topology),
            ProxyPeerRoute::Coordinated(CoordinatedPeerOperation::Remove),
            vec![remove("peer.addon64"), live, remove("ReShade64.dll.bak")],
        ),
        Err(PeerTransitionError::PhysicalProgramMismatch(_))
    ));
}
