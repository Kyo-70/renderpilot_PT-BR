use super::*;

mod dlss;
mod feature;
mod materialization;
mod reserved;
mod transition;

fn authority(feature: RenoDxReshadeIniFeature) -> RenoDxReshadeIniAuthority {
    RenoDxReshadeIniAuthority::new(feature, path("game")).expect("authority")
}

fn typed(
    operation: PeerEndpointOperation,
    authority: &RenoDxReshadeIniAuthority,
) -> PeerEndpointIntent {
    PeerEndpointIntent::new(
        authority.ini_path().clone(),
        PeerEndpointRole::RenoDxReshadeIni,
        operation,
        (operation != PeerEndpointOperation::Remove).then(|| hash('i')),
        (operation != PeerEndpointOperation::Remove).then_some(4),
    )
    .expect("typed intent")
}
