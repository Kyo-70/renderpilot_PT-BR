use super::*;

#[test]
fn main_install_binds_typed_create_and_removes_generic_projection() {
    let install_authority = authority(RenoDxReshadeIniFeature::Install);
    let ini = install_authority.ini_path().clone();
    let after = peer(&["game/ReShade.ini"], &[], Vec::new());
    let addon = PeerEndpointIntent::create(
        after.addon_file().clone(),
        PeerEndpointRole::Disjoint,
        Some(hash('a')),
        Some(8),
    )
    .expect("addon intent");
    let contract = PeerTransitionContract::derive_physical_with_renodx_reshade_ini(
        None,
        Some(&after),
        None,
        None,
        ProxyPeerRoute::DurableDisjoint,
        install_authority.clone(),
        vec![
            addon,
            typed(PeerEndpointOperation::Create, &install_authority),
        ],
    )
    .expect("typed install contract");
    assert_eq!(
        contract.renodx_reshade_ini_authority(),
        Some(&install_authority)
    );
    assert_eq!(contract.intents().len(), 2);
    assert_eq!(contract.intents()[1].path(), &ini);
    assert_eq!(
        contract.intents()[1].role(),
        PeerEndpointRole::RenoDxReshadeIni
    );
    contract
        .validate_intents()
        .expect("authority-bound contract validates");

    let replace_authority = authority(RenoDxReshadeIniFeature::Install);
    let no_ini = peer(&[], &[], Vec::new());
    let addon = PeerEndpointIntent::create(
        no_ini.addon_file().clone(),
        PeerEndpointRole::Disjoint,
        Some(hash('a')),
        Some(8),
    )
    .expect("addon intent");
    PeerTransitionContract::derive_physical_with_renodx_reshade_ini(
        None,
        Some(&no_ini),
        None,
        None,
        ProxyPeerRoute::DurableDisjoint,
        replace_authority.clone(),
        vec![
            addon,
            typed(PeerEndpointOperation::Replace, &replace_authority),
        ],
    )
    .expect("existing ReShade.ini replacement");

    let reversed = peer(&["game/ReShade.ini"], &[], Vec::new());
    let addon = PeerEndpointIntent::create(
        reversed.addon_file().clone(),
        PeerEndpointRole::Disjoint,
        Some(hash('a')),
        Some(8),
    )
    .expect("addon intent");
    assert!(matches!(
        PeerTransitionContract::derive_physical_with_renodx_reshade_ini(
            None,
            Some(&reversed),
            None,
            None,
            ProxyPeerRoute::DurableDisjoint,
            install_authority,
            vec![
                addon,
                typed(PeerEndpointOperation::Replace, &replace_authority)
            ],
        ),
        Err(PeerTransitionError::InvalidRenoDxReshadeIniTransition(_))
    ));
}

#[test]
fn main_uninstall_created_ini_binds_typed_remove() {
    let authority = authority(RenoDxReshadeIniFeature::Uninstall);
    let before = peer(&["game/ReShade.ini"], &[], Vec::new());
    let addon = PeerEndpointIntent::remove(before.addon_file().clone(), PeerEndpointRole::Disjoint)
        .expect("addon remove");
    let contract = PeerTransitionContract::derive_physical_with_renodx_reshade_ini(
        Some(&before),
        None,
        None,
        None,
        ProxyPeerRoute::DurableDisjoint,
        authority.clone(),
        vec![addon, typed(PeerEndpointOperation::Remove, &authority)],
    )
    .expect("typed uninstall contract");
    assert_eq!(
        contract.intents()[1].operation(),
        PeerEndpointOperation::Remove
    );

    let created_historical = peer(&["game/ReShade.ini"], &["game/ReShade.ini"], Vec::new());
    let addon = PeerEndpointIntent::remove(
        created_historical.addon_file().clone(),
        PeerEndpointRole::Disjoint,
    )
    .expect("addon remove");
    let historical_replace = PeerTransitionContract::derive_physical_with_renodx_reshade_ini(
        Some(&created_historical),
        None,
        None,
        None,
        ProxyPeerRoute::DurableDisjoint,
        authority.clone(),
        vec![addon, typed(PeerEndpointOperation::Replace, &authority)],
    )
    .expect("historical created path accepts replacement");
    assert_eq!(
        historical_replace.intents()[1].operation(),
        PeerEndpointOperation::Replace
    );

    let unclaimed = peer(&[], &[], Vec::new());
    let addon =
        PeerEndpointIntent::remove(unclaimed.addon_file().clone(), PeerEndpointRole::Disjoint)
            .expect("addon remove");
    PeerTransitionContract::derive_physical_with_renodx_reshade_ini(
        Some(&unclaimed),
        None,
        None,
        None,
        ProxyPeerRoute::DurableDisjoint,
        authority.clone(),
        vec![addon, typed(PeerEndpointOperation::Replace, &authority)],
    )
    .expect("unclaimed path accepts replacement");

    let created_only = peer(&["game/ReShade.ini"], &[], Vec::new());
    let addon = PeerEndpointIntent::remove(
        created_only.addon_file().clone(),
        PeerEndpointRole::Disjoint,
    )
    .expect("addon remove");
    assert!(matches!(
        PeerTransitionContract::derive_physical_with_renodx_reshade_ini(
            Some(&created_only),
            None,
            None,
            None,
            ProxyPeerRoute::DurableDisjoint,
            authority.clone(),
            vec![addon, typed(PeerEndpointOperation::Replace, &authority)],
        ),
        Err(PeerTransitionError::InvalidRenoDxReshadeIniTransition(_))
    ));

    let addon = PeerEndpointIntent::remove(
        created_only.addon_file().clone(),
        PeerEndpointRole::Disjoint,
    )
    .expect("addon remove");
    PeerTransitionContract::derive_physical_with_renodx_reshade_ini(
        Some(&created_only),
        None,
        None,
        None,
        ProxyPeerRoute::DurableDisjoint,
        authority.clone(),
        vec![addon, typed(PeerEndpointOperation::Remove, &authority)],
    )
    .expect("created path accepts removal");
}

#[test]
fn coordinated_route_accepts_typed_ini_and_one_downstream_only_with_authority() {
    let authority = authority(RenoDxReshadeIniFeature::DlssFixUpdate);
    let downstream = PeerEndpointIntent::replace(
        path("ReShade64.dll"),
        PeerEndpointRole::TopologyDownstream,
        Some(hash('r')),
        Some(6),
    )
    .expect("downstream intent");
    validate_intents_with_renodx_reshade_ini(
        ProxyPeerRoute::Coordinated(CoordinatedPeerOperation::ReplaceSamePath),
        &[
            downstream,
            typed(PeerEndpointOperation::Replace, &authority),
        ],
        &authority,
    )
    .expect("coordinated typed route");
}
