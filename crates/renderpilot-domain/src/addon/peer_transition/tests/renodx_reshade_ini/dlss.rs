use super::*;

#[test]
fn dlss_install_allows_repair_create_and_stable_replace() {
    let install_authority = authority(RenoDxReshadeIniFeature::DlssFixInstall);
    let before = peer(&[], &[], Vec::new());
    let after = peer(&["game/ReShade.ini"], &[], Vec::new());
    let create = PeerTransitionContract::derive_physical_with_renodx_reshade_ini(
        Some(&before),
        Some(&after),
        None,
        None,
        ProxyPeerRoute::DurableDisjoint,
        install_authority.clone(),
        vec![typed(PeerEndpointOperation::Create, &install_authority)],
    )
    .expect("repair create");
    assert_eq!(
        create.intents()[0].operation(),
        PeerEndpointOperation::Create
    );

    let stable = peer(&["game/ReShade.ini"], &[], Vec::new());
    let replace_authority = authority(RenoDxReshadeIniFeature::DlssFixInstall);
    let replace = PeerTransitionContract::derive_physical_with_renodx_reshade_ini(
        Some(&stable),
        Some(&stable),
        None,
        None,
        ProxyPeerRoute::DurableDisjoint,
        replace_authority.clone(),
        vec![typed(PeerEndpointOperation::Replace, &replace_authority)],
    )
    .expect("stable replace");
    assert_eq!(
        replace.intents()[0].operation(),
        PeerEndpointOperation::Replace
    );

    let missing_after = peer(&[], &[], Vec::new());
    assert!(matches!(
        PeerTransitionContract::derive_physical_with_renodx_reshade_ini(
            Some(&before),
            Some(&missing_after),
            None,
            None,
            ProxyPeerRoute::DurableDisjoint,
            install_authority.clone(),
            vec![typed(PeerEndpointOperation::Create, &install_authority)],
        ),
        Err(PeerTransitionError::InvalidRenoDxReshadeIniTransition(_))
    ));
    assert!(matches!(
        PeerTransitionContract::derive_physical_with_renodx_reshade_ini(
            Some(&before),
            Some(&after),
            None,
            None,
            ProxyPeerRoute::DurableDisjoint,
            install_authority.clone(),
            vec![typed(PeerEndpointOperation::Replace, &install_authority)],
        ),
        Err(PeerTransitionError::InvalidRenoDxReshadeIniTransition(_))
    ));

    let update_authority = authority(RenoDxReshadeIniFeature::DlssFixUpdate);
    assert!(matches!(
        PeerTransitionContract::derive_physical_with_renodx_reshade_ini(
            Some(&stable),
            Some(&stable),
            None,
            None,
            ProxyPeerRoute::DurableDisjoint,
            update_authority.clone(),
            vec![typed(PeerEndpointOperation::Create, &update_authority)],
        ),
        Err(PeerTransitionError::InvalidRenoDxReshadeIniTransition(_))
    ));
    assert!(matches!(
        PeerTransitionContract::derive_physical_with_renodx_reshade_ini(
            Some(&before),
            Some(&after),
            None,
            None,
            ProxyPeerRoute::DurableDisjoint,
            update_authority.clone(),
            vec![typed(PeerEndpointOperation::Replace, &update_authority)],
        ),
        Err(PeerTransitionError::InvalidRenoDxReshadeIniTransition(_))
    ));

    let uninstall_authority = authority(RenoDxReshadeIniFeature::DlssFixUninstall);
    assert!(matches!(
        PeerTransitionContract::derive_physical_with_renodx_reshade_ini(
            Some(&stable),
            Some(&stable),
            None,
            None,
            ProxyPeerRoute::DurableDisjoint,
            uninstall_authority.clone(),
            vec![typed(PeerEndpointOperation::Remove, &uninstall_authority)],
        ),
        Err(PeerTransitionError::InvalidRenoDxReshadeIniTransition(_))
    ));
    assert!(matches!(
        PeerTransitionContract::derive_physical_with_renodx_reshade_ini(
            Some(&before),
            Some(&after),
            None,
            None,
            ProxyPeerRoute::DurableDisjoint,
            uninstall_authority.clone(),
            vec![typed(PeerEndpointOperation::Replace, &uninstall_authority)],
        ),
        Err(PeerTransitionError::InvalidRenoDxReshadeIniTransition(_))
    ));
}
