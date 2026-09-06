use super::*;

#[test]
fn typed_read_guards_rederive_the_typed_contract_without_an_ini_guard() {
    let authority = authority(RenoDxReshadeIniFeature::DlssFixUpdate);
    let record = peer(&["game/ReShade.ini"], &[], Vec::new());
    let intent = typed(PeerEndpointOperation::Replace, &authority);
    let guards = required_read_guards_with_renodx_reshade_ini(
        Some(&record),
        Some(&record),
        None,
        None,
        ProxyPeerRoute::DurableDisjoint,
        std::slice::from_ref(&intent),
        &authority,
    )
    .expect("typed guards");
    assert!(guards.is_empty());
    assert!(
        required_read_guards(
            Some(&record),
            Some(&record),
            None,
            None,
            ProxyPeerRoute::DurableDisjoint,
            std::slice::from_ref(&intent),
        )
        .is_err()
    );
}

#[test]
fn contract_materializer_accepts_authority_bound_typed_evidence() {
    let authority = authority(RenoDxReshadeIniFeature::DlssFixUpdate);
    let record = peer(&["game/ReShade.ini"], &[], Vec::new());
    let intent = typed(PeerEndpointOperation::Replace, &authority);
    let topology = outer_topology();
    let contract = PeerTransitionContract::derive_physical_with_renodx_reshade_ini(
        Some(&record),
        Some(&record),
        Some(&topology),
        Some(&PlannedGameProxyTopology::Exact(topology.clone())),
        ProxyPeerRoute::DurableDisjoint,
        authority,
        vec![intent.clone()],
    )
    .expect("typed contract");
    let evidence = PeerEndpointEvidence::new(
        intent,
        Some(image("before", 'a', 4)),
        Some(image("after", 'i', 4)),
    );
    assert!(matches!(
        PlannedGameProxyTopology::Exact(topology.clone()).materialize_from_evidence(
            contract.route(),
            contract.intents(),
            &[],
        ),
        Err(PeerTransitionError::InvalidEndpointRole(_))
    ));
    assert_eq!(
        PlannedGameProxyTopology::Exact(topology.clone())
            .materialize_from_contract_evidence(&contract, &[evidence])
            .expect("materialized topology"),
        topology
    );
}

#[test]
fn renodx_errors_have_stable_display_and_equality() {
    let unsupported = PeerTransitionError::UnsupportedRenoDxReshadeIniFeature;
    assert_eq!(
        unsupported.to_string(),
        "unsupported RenoDX ReShade.ini feature"
    );
    assert_eq!(
        unsupported,
        PeerTransitionError::UnsupportedRenoDxReshadeIniFeature
    );

    let cardinality = PeerTransitionError::InvalidRenoDxReshadeIniCardinality(2);
    assert_eq!(
        cardinality.to_string(),
        "invalid RenoDX ReShade.ini endpoint cardinality: expected one, got 2"
    );
    assert_eq!(
        cardinality,
        PeerTransitionError::InvalidRenoDxReshadeIniCardinality(2)
    );

    let invalid_path = PeerTransitionError::InvalidRenoDxReshadeIniPath(path("wrong"));
    assert_eq!(
        invalid_path.to_string(),
        "invalid RenoDX ReShade.ini path derived from: C:/Games/Test/wrong"
    );
    assert_eq!(
        invalid_path,
        PeerTransitionError::InvalidRenoDxReshadeIniPath(path("wrong"))
    );

    let invalid_transition = PeerTransitionError::InvalidRenoDxReshadeIniTransition("reason");
    assert_eq!(
        invalid_transition.to_string(),
        "invalid RenoDX ReShade.ini transition: reason"
    );
    assert_eq!(
        invalid_transition,
        PeerTransitionError::InvalidRenoDxReshadeIniTransition("reason")
    );
}
