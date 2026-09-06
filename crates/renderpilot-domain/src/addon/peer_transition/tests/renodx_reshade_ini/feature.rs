use super::*;
use crate::mutation_features::{
    RENODX_DLSS_FIX_INSTALL, RENODX_DLSS_FIX_UNINSTALL, RENODX_DLSS_FIX_UPDATE, RENODX_INSTALL,
    RENODX_INSTALL_FROM_FILE, RENODX_SWITCH_RESHADE_CHANNEL, RENODX_UNINSTALL, RENODX_UPDATE,
};

#[test]
fn feature_parser_is_closed_and_authority_handles_roots() {
    for (wire, feature) in [
        (RENODX_INSTALL, RenoDxReshadeIniFeature::Install),
        (
            RENODX_INSTALL_FROM_FILE,
            RenoDxReshadeIniFeature::InstallFromFile,
        ),
        (RENODX_UNINSTALL, RenoDxReshadeIniFeature::Uninstall),
        (
            RENODX_DLSS_FIX_INSTALL,
            RenoDxReshadeIniFeature::DlssFixInstall,
        ),
        (
            RENODX_DLSS_FIX_UPDATE,
            RenoDxReshadeIniFeature::DlssFixUpdate,
        ),
        (
            RENODX_DLSS_FIX_UNINSTALL,
            RenoDxReshadeIniFeature::DlssFixUninstall,
        ),
    ] {
        let parsed = RenoDxReshadeIniFeature::try_from_feature(wire).expect("feature");
        assert_eq!(parsed, feature);
        assert_eq!(parsed.as_feature(), wire);
    }
    assert!(matches!(
        RenoDxReshadeIniFeature::try_from_feature(RENODX_SWITCH_RESHADE_CHANNEL),
        Err(PeerTransitionError::UnsupportedRenoDxReshadeIniFeature)
    ));
    for unsupported in [
        RENODX_UPDATE,
        "luma_install",
        "optiscaler_install",
        "unknown",
    ] {
        assert!(matches!(
            RenoDxReshadeIniFeature::try_from_feature(unsupported),
            Err(PeerTransitionError::UnsupportedRenoDxReshadeIniFeature)
        ));
    }

    let game_root = path("game");
    let normal = RenoDxReshadeIniAuthority::try_from_feature(RENODX_INSTALL, game_root.clone())
        .expect("normal authority");
    assert_eq!(normal.canonical_game_root(), &game_root);
    assert_eq!(normal.ini_path(), &path("game/ReShade.ini"));

    let unix = RenoDxReshadeIniAuthority::new(
        RenoDxReshadeIniFeature::Install,
        PathRef::new("/").expect("root"),
    )
    .expect("unix authority");
    assert_eq!(unix.ini_path().as_str(), "/ReShade.ini");
    let drive = RenoDxReshadeIniAuthority::new(
        RenoDxReshadeIniFeature::Install,
        PathRef::new("C:/").expect("drive root"),
    )
    .expect("drive authority");
    assert_eq!(drive.ini_path().as_str(), "C:/ReShade.ini");
}

#[test]
fn ordinary_validation_rejects_typed_endpoint() {
    let authority = authority(RenoDxReshadeIniFeature::Install);
    let intent = typed(PeerEndpointOperation::Create, &authority);
    assert!(matches!(
        validate_intents(ProxyPeerRoute::DurableDisjoint, &[intent]),
        Err(PeerTransitionError::InvalidEndpointRole(_))
    ));
}

#[test]
fn renodx_binding_rejects_luma_peer_records() {
    let authority = authority(RenoDxReshadeIniFeature::Install);
    let luma = InstalledAddon::new(game(), AddonKind::Luma, path("luma.addon64"));
    let after = peer(&["game/ReShade.ini"], &[], Vec::new());
    let addon = PeerEndpointIntent::create(
        after.addon_file().clone(),
        PeerEndpointRole::Disjoint,
        Some(hash('a')),
        Some(8),
    )
    .expect("addon intent");
    assert!(
        PeerTransitionContract::derive_physical_with_renodx_reshade_ini(
            Some(&luma),
            Some(&after),
            None,
            None,
            ProxyPeerRoute::DurableDisjoint,
            authority.clone(),
            vec![addon, typed(PeerEndpointOperation::Create, &authority)],
        )
        .is_err()
    );

    let luma_after = InstalledAddon::new(game(), AddonKind::Luma, path("luma.addon64"))
        .with_created_file(path("game/ReShade.ini"));
    let addon = PeerEndpointIntent::create(
        luma_after.addon_file().clone(),
        PeerEndpointRole::Disjoint,
        Some(hash('a')),
        Some(8),
    )
    .expect("addon intent");
    assert!(
        PeerTransitionContract::derive_physical_with_renodx_reshade_ini(
            None,
            Some(&luma_after),
            None,
            None,
            ProxyPeerRoute::DurableDisjoint,
            authority.clone(),
            vec![addon, typed(PeerEndpointOperation::Create, &authority)],
        )
        .is_err()
    );
}

#[test]
fn typed_validation_requires_exact_authority_cardinality_and_path() {
    let authority = authority(RenoDxReshadeIniFeature::Install);
    let intent = typed(PeerEndpointOperation::Create, &authority);
    validate_intents_with_renodx_reshade_ini(
        ProxyPeerRoute::DurableDisjoint,
        std::slice::from_ref(&intent),
        &authority,
    )
    .expect("typed endpoint is admitted by authority");

    assert!(matches!(
        validate_intents_with_renodx_reshade_ini(ProxyPeerRoute::DurableDisjoint, &[], &authority,),
        Err(PeerTransitionError::EmptyIntentSet)
    ));
    let ordinary = PeerEndpointIntent::create(
        path("ordinary.dll"),
        PeerEndpointRole::Disjoint,
        Some(hash('o')),
        Some(1),
    )
    .expect("ordinary endpoint");
    assert!(matches!(
        validate_intents_with_renodx_reshade_ini(
            ProxyPeerRoute::DurableDisjoint,
            &[ordinary],
            &authority,
        ),
        Err(PeerTransitionError::InvalidRenoDxReshadeIniCardinality(0))
    ));
    let wrong = PeerEndpointIntent::create(
        path("other/ReShade.ini"),
        PeerEndpointRole::RenoDxReshadeIni,
        Some(hash('i')),
        Some(4),
    )
    .expect("wrong typed endpoint");
    assert!(matches!(
        validate_intents_with_renodx_reshade_ini(
            ProxyPeerRoute::DurableDisjoint,
            &[wrong],
            &authority,
        ),
        Err(PeerTransitionError::InvalidRenoDxReshadeIniPath(_))
    ));

    let duplicate = typed(PeerEndpointOperation::Create, &authority);
    assert!(matches!(
        validate_intents_with_renodx_reshade_ini(
            ProxyPeerRoute::DurableDisjoint,
            &[duplicate.clone(), duplicate],
            &authority,
        ),
        Err(PeerTransitionError::DuplicateEndpoint(_))
    ));
    for invalid_path in ["game/ReShade.ini.bak", "game/other.ini"] {
        let invalid = PeerEndpointIntent::create(
            path(invalid_path),
            PeerEndpointRole::RenoDxReshadeIni,
            Some(hash('i')),
            Some(4),
        )
        .expect("invalid typed endpoint");
        assert!(matches!(
            validate_intents_with_renodx_reshade_ini(
                ProxyPeerRoute::DurableDisjoint,
                &[invalid],
                &authority,
            ),
            Err(PeerTransitionError::InvalidRenoDxReshadeIniPath(_))
        ));
    }
}
