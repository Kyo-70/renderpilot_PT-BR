use std::path::{Component, Path};

use renderpilot_domain::{PathRef, ProxyImplementation, normalized_path_key};

use crate::peer_mutation_executor::PeerPathSnapshot;

use super::error::RenoDxHostError;

pub(super) fn require_exact_host_path(
    game_root: &PathRef,
    expected: &PathRef,
) -> Result<(), RenoDxHostError> {
    let path = Path::new(expected.as_str());
    if !path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::CurDir | Component::ParentDir))
    {
        return Err(RenoDxHostError::Assessment(
            "active host path is not an absolute canonical path",
        ));
    }
    let direct = Path::new(game_root.as_str()).join("ReShade64.dll");
    let direct = PathRef::new(
        direct
            .to_str()
            .ok_or(RenoDxHostError::Assessment(
                "game root cannot form a UTF-8 ReShade64.dll path",
            ))?
            .to_owned(),
    )
    .map_err(|_| RenoDxHostError::Assessment("game root cannot form ReShade64.dll"))?;
    if normalized_path_key(direct.as_str()) != normalized_path_key(expected.as_str()) {
        return Err(RenoDxHostError::Path(direct, expected.clone()));
    }
    Ok(())
}

pub(super) fn require_snapshot_file<'a>(
    path: &PathRef,
    snapshot: &'a PeerPathSnapshot,
) -> Result<&'a crate::peer_mutation_executor::VerifiedPeerFile, RenoDxHostError> {
    snapshot.file().ok_or_else(|| {
        RenoDxHostError::Evidence(path.clone(), "expected a present regular-file snapshot")
    })
}

pub(super) fn require_absent(
    path: &PathRef,
    snapshot: &PeerPathSnapshot,
) -> Result<(), RenoDxHostError> {
    if snapshot.file().is_some() {
        return Err(RenoDxHostError::Evidence(
            path.clone(),
            "expected an absent endpoint snapshot",
        ));
    }
    Ok(())
}

pub(super) fn require_optiscaler_outer(
    implementation: ProxyImplementation,
) -> Result<(), RenoDxHostError> {
    if implementation != ProxyImplementation::OptiScaler {
        return Err(RenoDxHostError::Assessment(
            "active topology outer implementation is not OptiScaler",
        ));
    }
    Ok(())
}
