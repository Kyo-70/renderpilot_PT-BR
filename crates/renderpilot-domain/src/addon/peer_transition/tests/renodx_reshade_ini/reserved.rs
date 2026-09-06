use super::*;

#[test]
fn historical_uninstall_does_not_restore_or_delete_implicit_sidecar() {
    let uninstall_authority = authority(RenoDxReshadeIniFeature::Uninstall);
    let before = peer(&["game/ReShade.ini"], &["game/ReShade.ini"], Vec::new());
    let addon = PeerEndpointIntent::remove(before.addon_file().clone(), PeerEndpointRole::Disjoint)
        .expect("addon remove");
    let contract = PeerTransitionContract::derive_physical_with_renodx_reshade_ini(
        Some(&before),
        None,
        None,
        None,
        ProxyPeerRoute::DurableDisjoint,
        uninstall_authority.clone(),
        vec![
            addon,
            typed(PeerEndpointOperation::Replace, &uninstall_authority),
        ],
    )
    .expect("historical uninstall");
    assert_eq!(contract.intents().len(), 2);
    assert!(
        contract
            .intents()
            .iter()
            .all(|intent| !intent.path().as_str().ends_with("ReShade.ini.bak"))
    );

    let bad_sidecar = PeerEndpointIntent::remove(
        PathRef::new("C:/Games/Test/game/ReShade.ini.bak").expect("sidecar"),
        PeerEndpointRole::Disjoint,
    )
    .expect("sidecar endpoint");
    let bad_addon =
        PeerEndpointIntent::remove(before.addon_file().clone(), PeerEndpointRole::Disjoint)
            .expect("addon remove");
    assert!(matches!(
        PeerTransitionContract::derive_physical_with_renodx_reshade_ini(
            Some(&before),
            None,
            None,
            None,
            ProxyPeerRoute::DurableDisjoint,
            uninstall_authority.clone(),
            vec![
                bad_addon,
                bad_sidecar,
                typed(PeerEndpointOperation::Replace, &uninstall_authority),
            ],
        ),
        Err(PeerTransitionError::InvalidRenoDxReshadeIniTransition(_))
    ));
}

#[test]
fn typed_authority_rejects_reserved_managed_and_generic_sidecar_claims() {
    let authority = authority(RenoDxReshadeIniFeature::DlssFixUpdate);
    let managed_ini = ManagedAddonFile::owned(
        authority.ini_path().clone(),
        ManagedFileBaseline::Absent,
        hash('m'),
    );
    let managed_record = peer(&[], &[], vec![managed_ini]);
    assert!(matches!(
        PeerTransitionContract::derive_physical_with_renodx_reshade_ini(
            Some(&managed_record),
            Some(&managed_record),
            None,
            None,
            ProxyPeerRoute::DurableDisjoint,
            authority.clone(),
            vec![typed(PeerEndpointOperation::Replace, &authority)],
        ),
        Err(PeerTransitionError::InvalidRenoDxReshadeIniTransition(_))
    ));

    let generic_sidecar = peer(&[], &["game/ReShade.ini.bak"], Vec::new());
    assert!(
        PeerTransitionContract::derive_physical_with_renodx_reshade_ini(
            Some(&generic_sidecar),
            Some(&generic_sidecar),
            None,
            None,
            ProxyPeerRoute::DurableDisjoint,
            authority.clone(),
            vec![typed(PeerEndpointOperation::Replace, &authority)],
        )
        .is_err()
    );
}
