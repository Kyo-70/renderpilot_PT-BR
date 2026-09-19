//! RenoDX ReShade.ini endpoint binding and feature transition rules.

use crate::{
    AddonKind, InstalledAddon, NormalizedPathRelation, PathRef, normalized_path_key,
    normalized_path_relation,
};

use super::super::claims::PeerSnapshot;
use super::super::model::{
    PeerEndpointIntent, PeerEndpointOperation, PeerEndpointRole, PeerTransitionError,
};
use super::super::reconcile::DerivedEndpoint;
use super::super::renodx_reshade_ini::{RenoDxReshadeIniAuthority, RenoDxReshadeIniFeature};

/// Binds the caller's typed RenoDX endpoint to the aggregate transition.
///
/// Generic claim derivation intentionally still sees the complete persisted
/// record. This binding removes only the generic projection for the authority
/// path, and then reintroduces the caller's typed intent with an empty preimage
/// guard. The physical executor supplies the authoritative evidence later.
pub(super) fn bind(
    authority: &RenoDxReshadeIniAuthority,
    before_peer: Option<&InstalledAddon>,
    after_peer: Option<&InstalledAddon>,
    before: &PeerSnapshot,
    after: &PeerSnapshot,
    physical_program: &[PeerEndpointIntent],
    endpoints: &mut Vec<DerivedEndpoint>,
) -> Result<(), PeerTransitionError> {
    validate_peers(before_peer, after_peer)?;
    let typed = typed_endpoint(authority, physical_program)?;
    let sidecar = super::super::claims::managed_sidecar_path(authority.ini_path())?;
    reject_reserved_claims(authority, &sidecar, before, after, physical_program)?;
    validate_peer_presence(authority.feature(), before_peer, after_peer)?;
    let operation = expected_operation(
        authority.feature(),
        authority.ini_path(),
        before,
        after,
        typed.operation(),
    )?;

    endpoints.retain(|endpoint| {
        !matches!(
            normalized_path_relation(
                endpoint.intent.path().as_str(),
                authority.ini_path().as_str()
            ),
            NormalizedPathRelation::Equal
        )
    });
    if matches!(authority.feature(), RenoDxReshadeIniFeature::Uninstall)
        && is_historical_created_and_backed(before, authority.ini_path())
    {
        endpoints.retain(|endpoint| {
            !matches!(
                normalized_path_relation(endpoint.intent.path().as_str(), sidecar.as_str()),
                NormalizedPathRelation::Equal
            )
        });
    }
    if typed.operation() != operation {
        return Err(PeerTransitionError::InvalidRenoDxReshadeIniTransition(
            "physical intent operation does not match feature and persisted claims",
        ));
    }
    endpoints.push(DerivedEndpoint {
        intent: typed.clone(),
        guard: super::super::EndpointGuard::default(),
    });
    Ok(())
}

fn validate_peers(
    before_peer: Option<&InstalledAddon>,
    after_peer: Option<&InstalledAddon>,
) -> Result<(), PeerTransitionError> {
    if before_peer.is_none() && after_peer.is_none() {
        return Err(PeerTransitionError::InvalidRenoDxReshadeIniTransition(
            "a RenoDX peer record is required",
        ));
    }
    for peer in [before_peer, after_peer].into_iter().flatten() {
        if peer.kind() != AddonKind::RenoDx {
            return Err(PeerTransitionError::InvalidRenoDxReshadeIniTransition(
                "typed ReShade.ini authority requires RenoDX peer records",
            ));
        }
    }
    Ok(())
}

fn validate_peer_presence(
    feature: RenoDxReshadeIniFeature,
    before: Option<&InstalledAddon>,
    after: Option<&InstalledAddon>,
) -> Result<(), PeerTransitionError> {
    let valid = if feature.is_main_install() {
        before.is_none() && after.is_some()
    } else if feature.is_main_uninstall() {
        before.is_some() && after.is_none()
    } else {
        before.is_some() && after.is_some()
    };
    valid
        .then_some(())
        .ok_or(PeerTransitionError::InvalidRenoDxReshadeIniTransition(
            "peer record presence does not match RenoDX feature",
        ))
}

fn typed_endpoint<'a>(
    authority: &RenoDxReshadeIniAuthority,
    physical_program: &'a [PeerEndpointIntent],
) -> Result<&'a PeerEndpointIntent, PeerTransitionError> {
    let mut typed = physical_program
        .iter()
        .filter(|intent| intent.role() == PeerEndpointRole::RenoDxReshadeIni);
    let Some(intent) = typed.next() else {
        return Err(PeerTransitionError::InvalidRenoDxReshadeIniCardinality(0));
    };
    if typed.next().is_some() {
        return Err(PeerTransitionError::InvalidRenoDxReshadeIniCardinality(
            2 + typed.count(),
        ));
    }
    if !authority.matches_ini_path(intent.path()) {
        return Err(PeerTransitionError::InvalidRenoDxReshadeIniPath(
            intent.path().clone(),
        ));
    }
    Ok(intent)
}

fn reject_reserved_claims(
    authority: &RenoDxReshadeIniAuthority,
    sidecar: &PathRef,
    before: &PeerSnapshot,
    after: &PeerSnapshot,
    physical_program: &[PeerEndpointIntent],
) -> Result<(), PeerTransitionError> {
    let ini_key = normalized_path_key(authority.ini_path().as_str());
    let sidecar_key = normalized_path_key(sidecar.as_str());
    for snapshot in [before, after] {
        if snapshot.managed.contains_key(&ini_key) || snapshot.managed.contains_key(&sidecar_key) {
            return Err(PeerTransitionError::InvalidRenoDxReshadeIniTransition(
                "ReShade.ini and its sidecar cannot be managed claims",
            ));
        }
        if snapshot.created.contains_key(&sidecar_key) || snapshot.backed.contains_key(&sidecar_key)
        {
            return Err(PeerTransitionError::InvalidRenoDxReshadeIniTransition(
                "ReShade.ini sidecar cannot be a generic claim",
            ));
        }
    }
    if physical_program.iter().any(|intent| {
        matches!(
            normalized_path_relation(intent.path().as_str(), sidecar.as_str()),
            NormalizedPathRelation::Equal
        )
    }) {
        return Err(PeerTransitionError::InvalidRenoDxReshadeIniTransition(
            "ReShade.ini sidecar cannot be a physical endpoint",
        ));
    }
    Ok(())
}

fn expected_operation(
    feature: RenoDxReshadeIniFeature,
    ini_path: &PathRef,
    before: &PeerSnapshot,
    after: &PeerSnapshot,
    supplied: PeerEndpointOperation,
) -> Result<PeerEndpointOperation, PeerTransitionError> {
    let before_claim = claim_state(before, ini_path);
    let after_claim = claim_state(after, ini_path);
    let operation = match feature {
        RenoDxReshadeIniFeature::Install | RenoDxReshadeIniFeature::InstallFromFile => {
            if before_claim != (false, false) || after_claim.1 {
                return invalid_transition(
                    "main install requires an unclaimed before and after path",
                );
            }
            if after_claim.0 {
                PeerEndpointOperation::Create
            } else {
                PeerEndpointOperation::Replace
            }
        }
        RenoDxReshadeIniFeature::Uninstall => {
            if after_claim != (false, false) {
                return invalid_transition("uninstall requires an unclaimed after path");
            }
            match before_claim {
                (true, false) => PeerEndpointOperation::Remove,
                (false, false) | (true, true) => PeerEndpointOperation::Replace,
                (false, true) => return invalid_transition("backup claim has no live claim"),
            }
        }
        RenoDxReshadeIniFeature::Update => {
            if before_claim != after_claim {
                return invalid_transition("main update requires stable config claims");
            }
            if supplied != PeerEndpointOperation::Replace {
                return invalid_transition("main update requires a replacement config endpoint");
            }
            PeerEndpointOperation::Replace
        }
        RenoDxReshadeIniFeature::DlssFixInstall => {
            if before_claim.1 != after_claim.1 {
                return invalid_transition("DLSS-fix install cannot change backup membership");
            }
            if supplied == PeerEndpointOperation::Create {
                if !after_claim.0 {
                    return invalid_transition("DLSS-fix create requires a created after claim");
                }
                PeerEndpointOperation::Create
            } else if supplied == PeerEndpointOperation::Replace && before_claim == after_claim {
                PeerEndpointOperation::Replace
            } else {
                return invalid_transition("DLSS-fix install operation is not claim-compatible");
            }
        }
        RenoDxReshadeIniFeature::DlssFixUpdate | RenoDxReshadeIniFeature::DlssFixUninstall => {
            if before_claim != after_claim {
                return invalid_transition("DLSS-fix update/uninstall requires stable claims");
            }
            PeerEndpointOperation::Replace
        }
    };
    Ok(operation)
}

fn claim_state(snapshot: &PeerSnapshot, ini_path: &PathRef) -> (bool, bool) {
    let key = normalized_path_key(ini_path.as_str());
    (
        snapshot.created.contains_key(&key),
        snapshot.backed.contains_key(&key),
    )
}

fn is_historical_created_and_backed(snapshot: &PeerSnapshot, path: &PathRef) -> bool {
    let key = normalized_path_key(path.as_str());
    snapshot.created.contains_key(&key) && snapshot.backed.contains_key(&key)
}

fn invalid_transition(reason: &'static str) -> Result<PeerEndpointOperation, PeerTransitionError> {
    Err(PeerTransitionError::InvalidRenoDxReshadeIniTransition(
        reason,
    ))
}
