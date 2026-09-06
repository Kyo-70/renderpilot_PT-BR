//! Route and evidence validation for peer transitions.

use std::collections::BTreeSet;

use crate::{normalized_path_key, normalized_path_relation};

use super::model::{
    CoordinatedPeerOperation, PeerEndpointEvidence, PeerEndpointIntent, PeerEndpointOperation,
    PeerEndpointRole, PeerTransitionAuthorities, PeerTransitionError, ProxyPeerRoute,
};
use super::optiscaler_config::{ExactOptiConfigProjection, OptiConfigOperation};
use super::renodx_reshade_ini::RenoDxReshadeIniAuthority;
use super::{EndpointGuard, managed_sidecar_path};

/// Validates a complete flat endpoint program for its closed route class.
pub fn validate_intents(
    route: ProxyPeerRoute,
    intents: &[PeerEndpointIntent],
) -> Result<(), PeerTransitionError> {
    validate_intents_with_authority(route, intents, None)
}

/// Validates a route while admitting the one authority-bound RenoDX.ini endpoint.
pub fn validate_intents_with_renodx_reshade_ini(
    route: ProxyPeerRoute,
    intents: &[PeerEndpointIntent],
    authority: &RenoDxReshadeIniAuthority,
) -> Result<(), PeerTransitionError> {
    validate_intents_with_authority(route, intents, Some(authority))
}

/// Validates a route against all singleton authorities participating in the
/// same aggregate. This is the domain boundary for coexistence and the
/// OptiScaler LoadReshade ordering contract.
pub fn validate_intents_with_authorities(
    route: ProxyPeerRoute,
    intents: &[PeerEndpointIntent],
    authorities: &PeerTransitionAuthorities,
) -> Result<(), PeerTransitionError> {
    if let (Some(renodx), Some(opti)) = (
        authorities.renodx_reshade_ini.as_ref(),
        authorities.optiscaler_config.as_ref(),
    ) && normalized_path_key(renodx.canonical_game_root().as_str())
        != normalized_path_key(opti.authority().canonical_game_root().as_str())
    {
        return Err(PeerTransitionError::InvalidOptiScalerConfigTransition(
            "coexisting authorities have different canonical game roots",
        ));
    }
    validate_intents_with_scoped_authority(
        route,
        intents,
        authorities.renodx_reshade_ini.as_ref(),
        authorities.optiscaler_config.as_ref(),
        authorities.dlss_fix.as_ref(),
    )?;

    validate_topology_authority(intents, authorities.topology_downstream.as_ref())?;
    validate_dlss_authority(intents, authorities.dlss_fix.as_ref())
}

/// Shared route validation.  The optional authority is the only capability
/// that can admit a typed RenoDX endpoint; ordinary callers remain closed.
pub(super) fn validate_intents_with_authority(
    route: ProxyPeerRoute,
    intents: &[PeerEndpointIntent],
    authority: Option<&RenoDxReshadeIniAuthority>,
) -> Result<(), PeerTransitionError> {
    validate_intents_with_scoped_authority(route, intents, authority, None, None)
}

pub(super) fn validate_intents_with_scoped_authority(
    route: ProxyPeerRoute,
    intents: &[PeerEndpointIntent],
    renodx_authority: Option<&RenoDxReshadeIniAuthority>,
    optiscaler_authority: Option<&ExactOptiConfigProjection>,
    dlss_authority: Option<&super::dlss::RenoDxDlssProjection>,
) -> Result<(), PeerTransitionError> {
    if intents.is_empty() {
        if let Some(projection) = optiscaler_authority
            && projection.operation().requires_physical_endpoint()
        {
            return Err(PeerTransitionError::MissingOptiScalerConfigTransition);
        }
        return Err(PeerTransitionError::EmptyIntentSet);
    }

    let mut seen = BTreeSet::new();
    for (index, intent) in intents.iter().enumerate() {
        let key = normalized_path_key(intent.path().as_str());
        if !seen.insert(key) {
            return Err(PeerTransitionError::DuplicateEndpoint(
                intent.path().clone(),
            ));
        }
        for previous in intents.iter().take(index) {
            if normalized_path_relation(previous.path().as_str(), intent.path().as_str()).overlaps()
            {
                return Err(PeerTransitionError::OverlappingEndpoints(
                    previous.path().clone(),
                    intent.path().clone(),
                ));
            }
        }
    }

    let mut typed = intents
        .iter()
        .filter(|intent| intent.role() == PeerEndpointRole::RenoDxReshadeIni);
    let first_typed = typed.next();
    let has_multiple_typed = typed.next().is_some();
    match renodx_authority {
        Some(authority) => {
            let Some(intent) = first_typed else {
                return Err(PeerTransitionError::InvalidRenoDxReshadeIniCardinality(0));
            };
            if has_multiple_typed {
                return Err(PeerTransitionError::InvalidRenoDxReshadeIniCardinality(
                    2 + typed.count(),
                ));
            }
            if !authority.matches_ini_path(intent.path()) {
                return Err(PeerTransitionError::InvalidRenoDxReshadeIniPath(
                    intent.path().clone(),
                ));
            }
        }
        None if let Some(intent) = first_typed => {
            return Err(PeerTransitionError::InvalidEndpointRole(
                intent.path().clone(),
            ));
        }
        None => {}
    }

    let mut config = intents
        .iter()
        .filter(|intent| intent.role() == PeerEndpointRole::OptiScalerConfig);
    let first_config = config.next();
    let has_multiple_config = config.next().is_some();
    match optiscaler_authority {
        Some(projection) => {
            projection.validate()?;
            if has_multiple_config {
                return Err(PeerTransitionError::InvalidOptiScalerConfigCardinality(
                    2 + config.count(),
                ));
            }
            match projection.operation() {
                OptiConfigOperation::NoChange if let Some(intent) = first_config => {
                    return Err(PeerTransitionError::OptiScalerConfigNoChange(
                        intent.path().clone(),
                    ));
                }
                OptiConfigOperation::NoChange => {}
                OptiConfigOperation::EnableLoadReshade
                | OptiConfigOperation::DisableLoadReshade => {
                    let Some(intent) = first_config else {
                        return Err(PeerTransitionError::MissingOptiScalerConfigTransition);
                    };
                    if !projection.authority().matches_config_path(intent.path()) {
                        return Err(PeerTransitionError::InvalidOptiScalerConfigPath(
                            intent.path().clone(),
                        ));
                    }
                    if intent.operation() != PeerEndpointOperation::Replace {
                        return Err(PeerTransitionError::InvalidOptiScalerConfigTransition(
                            "LoadReshade changes require one Replace endpoint",
                        ));
                    }
                    if intent.planned_sha256() != Some(projection.receipt().installed.digest()) {
                        return Err(PeerTransitionError::InvalidOptiScalerConfigTransition(
                            "config endpoint digest differs from its Configuration receipt",
                        ));
                    }
                }
            }
        }
        None if let Some(intent) = first_config => {
            return Err(PeerTransitionError::InvalidEndpointRole(
                intent.path().clone(),
            ));
        }
        None => {}
    }

    let downstream = intents
        .iter()
        .filter(|intent| intent.role() == PeerEndpointRole::TopologyDownstream)
        .collect::<Vec<_>>();
    match route {
        ProxyPeerRoute::DurableDisjoint => {
            if intents.iter().any(|intent| {
                intent.role() != PeerEndpointRole::Disjoint
                    && !(renodx_authority.is_some()
                        && intent.role() == PeerEndpointRole::RenoDxReshadeIni)
                    && !(optiscaler_authority.is_some()
                        && intent.role() == PeerEndpointRole::OptiScalerConfig)
                    && !(dlss_authority.is_some() && intent.role() == PeerEndpointRole::DlssFix)
            }) {
                let path = intents
                    .iter()
                    .find(|intent| {
                        intent.role() != PeerEndpointRole::Disjoint
                            && !(renodx_authority.is_some()
                                && intent.role() == PeerEndpointRole::RenoDxReshadeIni)
                            && !(optiscaler_authority.is_some()
                                && intent.role() == PeerEndpointRole::OptiScalerConfig)
                            && !(dlss_authority.is_some()
                                && intent.role() == PeerEndpointRole::DlssFix)
                    })
                    .expect("role check found an endpoint")
                    .path()
                    .clone();
                return Err(PeerTransitionError::InvalidEndpointRole(path));
            }
        }
        ProxyPeerRoute::Coordinated(operation) => {
            if downstream.len() != 1 {
                return Err(PeerTransitionError::InvalidDownstreamCardinality(
                    downstream.len(),
                ));
            }
            let expected = match operation {
                CoordinatedPeerOperation::Create => PeerEndpointOperation::Create,
                CoordinatedPeerOperation::ReplaceSamePath => PeerEndpointOperation::Replace,
                CoordinatedPeerOperation::Remove => {
                    // A present managed baseline restores the live file in place
                    // while removing its sidecar, so the downstream endpoint is
                    // a replace even though the logical operation is remove.
                    let actual = downstream[0].operation();
                    if !matches!(
                        actual,
                        PeerEndpointOperation::Remove | PeerEndpointOperation::Replace
                    ) {
                        return Err(PeerTransitionError::CoordinatedOperationMismatch);
                    }
                    // The config-order validation below still has to run for
                    // a downstream release. Returning here would silently
                    // admit a disable operation after its host removal.
                    PeerEndpointOperation::Remove
                }
            };
            if operation != CoordinatedPeerOperation::Remove
                && downstream[0].operation() != expected
            {
                return Err(PeerTransitionError::CoordinatedOperationMismatch);
            }
        }
    }
    validate_optiscaler_config_order(route, intents, optiscaler_authority)?;
    validate_dlss_authority(intents, dlss_authority)?;
    Ok(())
}

fn validate_topology_authority(
    intents: &[PeerEndpointIntent],
    authority_path: Option<&crate::PathRef>,
) -> Result<(), PeerTransitionError> {
    let downstream = intents
        .iter()
        .filter(|intent| intent.role() == PeerEndpointRole::TopologyDownstream)
        .collect::<Vec<_>>();
    match authority_path {
        Some(path) => {
            if downstream.len() != 1 {
                return Err(PeerTransitionError::InvalidDownstreamCardinality(
                    downstream.len(),
                ));
            }
            if normalized_path_key(downstream[0].path().as_str())
                != normalized_path_key(path.as_str())
            {
                return Err(PeerTransitionError::PhysicalProgramMismatch(
                    downstream[0].path().clone(),
                ));
            }
        }
        None if !downstream.is_empty() => {
            return Err(PeerTransitionError::InvalidEndpointRole(
                downstream[0].path().clone(),
            ));
        }
        None => {}
    }
    Ok(())
}

fn validate_dlss_authority(
    intents: &[PeerEndpointIntent],
    projection: Option<&super::dlss::RenoDxDlssProjection>,
) -> Result<(), PeerTransitionError> {
    let Some(projection) = projection else {
        return Ok(());
    };
    let companion_key = normalized_path_key(projection.companion_path().as_str());
    let matching = intents
        .iter()
        .enumerate()
        .filter(|(_, intent)| normalized_path_key(intent.path().as_str()) == companion_key)
        .collect::<Vec<_>>();
    if matching.len() > 1 {
        return Err(PeerTransitionError::InvalidDlssFixCardinality(
            matching.len(),
        ));
    }
    let Some((_, intent)) = matching.first() else {
        // A claim-only transition is valid when no physical companion change
        // is implied by the projection's before/after claim slots.
        return Ok(());
    };
    if intent.role() != PeerEndpointRole::DlssFix {
        return Err(PeerTransitionError::InvalidDlssFixPath(
            intent.path().clone(),
        ));
    }
    let expected = match (
        projection.before_image(),
        projection.before_claim().created(),
        projection.after_claim().created(),
    ) {
        (_, false, false) => None,
        (super::dlss::RenoDxDlssBeforeImage::Absent, _, true) => {
            Some(PeerEndpointOperation::Create)
        }
        (super::dlss::RenoDxDlssBeforeImage::Present { .. }, _, true) => {
            Some(PeerEndpointOperation::Replace)
        }
        (_, true, false) => Some(PeerEndpointOperation::Remove),
    };
    if expected != Some(intent.operation()) {
        return Err(PeerTransitionError::InvalidDlssFixOperation(
            intent.path().clone(),
        ));
    }
    Ok(())
}

/// Validates the physical companion binding for the specialized RenoDX DLSS
/// projection.  This is intentionally separate from `validate_dlss_authority`:
/// the latter validates the public typed `DlssFix` authority contract, while a
/// RenoDX physical program represents the same disjoint companion file as a
/// `Disjoint` endpoint.
pub(super) fn validate_dlss_physical_program(
    intents: &[PeerEndpointIntent],
    projection: &super::dlss::RenoDxDlssProjection,
) -> Result<(), PeerTransitionError> {
    let companion_key = normalized_path_key(projection.companion_path().as_str());
    let matching = intents
        .iter()
        .filter(|intent| normalized_path_key(intent.path().as_str()) == companion_key)
        .collect::<Vec<_>>();
    if matching.len() > 1 {
        return Err(PeerTransitionError::InvalidDlssFixCardinality(
            matching.len(),
        ));
    }

    let expected = match (
        projection.before_image(),
        projection.before_claim().created(),
        projection.after_claim().created(),
    ) {
        (_, false, false) => None,
        (super::dlss::RenoDxDlssBeforeImage::Absent, _, true) => {
            Some(PeerEndpointOperation::Create)
        }
        (super::dlss::RenoDxDlssBeforeImage::Present { .. }, _, true) => {
            Some(PeerEndpointOperation::Replace)
        }
        (_, true, false) => Some(PeerEndpointOperation::Remove),
    };

    let Some(intent) = matching.first() else {
        // Physical omission is claim-only and therefore safe only when the
        // companion's created ownership is unchanged (source-slot refresh or
        // removal). A created ownership transition must have an exact file
        // endpoint in the same physical program.
        if projection.before_claim().created() != projection.after_claim().created() {
            return Err(PeerTransitionError::InvalidDlssFixPath(
                projection.companion_path().clone(),
            ));
        }
        return Ok(());
    };

    if intent.role() != PeerEndpointRole::Disjoint {
        return Err(PeerTransitionError::InvalidDlssFixPath(
            intent.path().clone(),
        ));
    }
    if expected != Some(intent.operation()) {
        return Err(PeerTransitionError::InvalidDlssFixOperation(
            intent.path().clone(),
        ));
    }
    Ok(())
}

fn validate_optiscaler_config_order(
    route: ProxyPeerRoute,
    intents: &[PeerEndpointIntent],
    projection: Option<&ExactOptiConfigProjection>,
) -> Result<(), PeerTransitionError> {
    let Some(projection) = projection else {
        return Ok(());
    };
    let config_index = intents
        .iter()
        .position(|intent| intent.role() == PeerEndpointRole::OptiScalerConfig);
    let downstream = intents
        .iter()
        .enumerate()
        .filter(|(_, intent)| intent.role() == PeerEndpointRole::TopologyDownstream)
        .collect::<Vec<_>>();
    match projection.operation() {
        OptiConfigOperation::NoChange => {
            if matches!(route, ProxyPeerRoute::Coordinated(_)) {
                return Err(PeerTransitionError::InvalidOptiScalerConfigTransition(
                    "NoChange cannot accompany a coordinated downstream transition",
                ));
            }
        }
        OptiConfigOperation::EnableLoadReshade => {
            let Some(config_index) = config_index else {
                return Err(PeerTransitionError::MissingOptiScalerConfigTransition);
            };
            let Some((downstream_index, _)) = downstream.iter().find(|(_, intent)| {
                matches!(
                    intent.operation(),
                    PeerEndpointOperation::Create | PeerEndpointOperation::Replace
                )
            }) else {
                return Err(PeerTransitionError::InvalidOptiScalerConfigTransition(
                    "enable requires downstream acquisition or replacement",
                ));
            };
            if config_index <= *downstream_index {
                return Err(PeerTransitionError::OptiScalerConfigOrder {
                    config: config_index,
                    downstream: *downstream_index,
                });
            }
        }
        OptiConfigOperation::DisableLoadReshade => {
            let Some(config_index) = config_index else {
                return Err(PeerTransitionError::MissingOptiScalerConfigTransition);
            };
            let Some((downstream_index, _)) = downstream.iter().find(|(_, intent)| {
                matches!(
                    intent.operation(),
                    PeerEndpointOperation::Remove | PeerEndpointOperation::Replace
                )
            }) else {
                return Err(PeerTransitionError::InvalidOptiScalerConfigTransition(
                    "disable requires downstream release or removal",
                ));
            };
            if config_index >= *downstream_index {
                return Err(PeerTransitionError::OptiScalerConfigOrder {
                    config: config_index,
                    downstream: *downstream_index,
                });
            }
        }
    }
    Ok(())
}

/// Validates evidence against an endpoint program without snapshot-specific
/// guards. This is useful at an executor boundary; a derived contract adds
/// stronger preimage checks through `validate_evidence_with_guards`.
pub fn validate_evidence(
    intents: &[PeerEndpointIntent],
    evidence: &[PeerEndpointEvidence],
) -> Result<(), PeerTransitionError> {
    validate_evidence_with_guards(intents, evidence, &[])
}

pub(crate) fn validate_evidence_with_guards(
    intents: &[PeerEndpointIntent],
    evidence: &[PeerEndpointEvidence],
    guards: &[EndpointGuard],
) -> Result<(), PeerTransitionError> {
    if intents.len() != evidence.len() {
        return Err(PeerTransitionError::EvidenceCardinality {
            expected: intents.len(),
            actual: evidence.len(),
        });
    }
    if !guards.is_empty() && guards.len() != intents.len() {
        return Err(PeerTransitionError::EvidenceCardinality {
            expected: intents.len(),
            actual: guards.len(),
        });
    }

    for (index, (intent, observed)) in intents.iter().zip(evidence).enumerate() {
        if observed.intent() != intent {
            return Err(PeerTransitionError::EvidenceOrderMismatch(
                intent.path().clone(),
            ));
        }
        let before = observed.before();
        let after = observed.after();
        match intent.operation() {
            PeerEndpointOperation::Create if before.is_some() || after.is_none() => {
                return Err(PeerTransitionError::InvalidPreimage(intent.path().clone()));
            }
            PeerEndpointOperation::Replace if before.is_none() || after.is_none() => {
                return Err(PeerTransitionError::InvalidPreimage(intent.path().clone()));
            }
            PeerEndpointOperation::Remove if before.is_none() || after.is_some() => {
                return Err(PeerTransitionError::InvalidPostimage(intent.path().clone()));
            }
            _ => {}
        }
        if let Some(planned) = intent.planned_sha256()
            && after.is_none_or(|image| image.sha256() != planned)
        {
            return Err(PeerTransitionError::DigestMismatch(intent.path().clone()));
        }
        if let Some(planned) = intent.planned_length()
            && after.is_none_or(|image| image.length() != planned)
        {
            return Err(PeerTransitionError::LengthMismatch(intent.path().clone()));
        }
        if let Some(guard) = guards.get(index) {
            if let Some(expected) = guard.before_sha256.as_ref()
                && before.is_none_or(|image| image.sha256() != expected)
            {
                return Err(PeerTransitionError::DigestMismatch(intent.path().clone()));
            }
            if let Some(expected) = guard.before_length
                && before.is_none_or(|image| image.length() != expected)
            {
                return Err(PeerTransitionError::LengthMismatch(intent.path().clone()));
            }
            if let Some(expected) = guard.after_sha256.as_ref()
                && after.is_none_or(|image| image.sha256() != expected)
            {
                return Err(PeerTransitionError::DigestMismatch(intent.path().clone()));
            }
            if let Some(expected) = guard.after_length
                && after.is_none_or(|image| image.length() != expected)
            {
                return Err(PeerTransitionError::LengthMismatch(intent.path().clone()));
            }
        }
    }

    validate_paired_images(intents, evidence)
}

fn validate_paired_images(
    intents: &[PeerEndpointIntent],
    evidence: &[PeerEndpointEvidence],
) -> Result<(), PeerTransitionError> {
    for (index, intent) in intents.iter().enumerate() {
        if intent.operation() != PeerEndpointOperation::Replace {
            continue;
        }
        let sidecar = managed_sidecar_path(intent.path())?;
        let Some(sidecar_index) = intents.iter().position(|candidate| {
            normalized_path_key(candidate.path().as_str()) == normalized_path_key(sidecar.as_str())
        }) else {
            continue;
        };
        let sidecar = &intents[sidecar_index];
        if !matches!(
            sidecar.operation(),
            PeerEndpointOperation::Create | PeerEndpointOperation::Remove
        ) {
            continue;
        }
        let live = &evidence[index];
        let sidecar_evidence = &evidence[sidecar_index];
        let matches = match sidecar.operation() {
            PeerEndpointOperation::Create => live
                .before()
                .zip(sidecar_evidence.after())
                .is_some_and(|(before_live, after_sidecar)| {
                    before_live.sha256() == after_sidecar.sha256()
                        && before_live.length() == after_sidecar.length()
                }),
            PeerEndpointOperation::Remove => live
                .after()
                .zip(sidecar_evidence.before())
                .is_some_and(|(after_live, before_sidecar)| {
                    after_live.sha256() == before_sidecar.sha256()
                        && after_live.length() == before_sidecar.length()
                }),
            PeerEndpointOperation::Replace => true,
        };
        if !matches {
            return Err(PeerTransitionError::PairedImageMismatch(
                intent.path().clone(),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        FileReceipt, OptiScalerFileCleanup, OptiScalerFileReceipt, OptiScalerFileRole, PathRef,
        RenoDxDlssBeforeImage, RenoDxDlssClaim, RenoDxDlssProjection, RenoDxReshadeIniAuthority,
        RenoDxReshadeIniFeature, Sha256Hash,
    };

    fn root() -> PathRef {
        PathRef::new("C:/Games/Test").expect("root")
    }

    fn config_projection(operation: OptiConfigOperation) -> ExactOptiConfigProjection {
        let authority = super::super::optiscaler_config::OptiScalerConfigAuthority::new(root())
            .expect("authority");
        let receipt = OptiScalerFileReceipt {
            path: authority.config_path().clone(),
            installed: FileReceipt::owned(
                "config-id",
                Sha256Hash::new("a".repeat(64)).expect("digest"),
            )
            .expect("receipt"),
            role: OptiScalerFileRole::Configuration,
            cleanup: OptiScalerFileCleanup::PreserveCurrentThenRestoreBaseline,
            baseline: crate::OptiScalerReleaseFileBaseline::Absent,
        };
        ExactOptiConfigProjection::new(authority, receipt, operation).expect("projection")
    }

    fn config_intent(operation: OptiConfigOperation) -> PeerEndpointIntent {
        config_projection(operation)
            .physical_intent()
            .expect("intent result")
            .expect("physical intent")
    }

    fn downstream(path: &str, operation: PeerEndpointOperation) -> PeerEndpointIntent {
        PeerEndpointIntent::new(
            PathRef::new(format!("C:/Games/Test/{path}")).expect("downstream path"),
            PeerEndpointRole::TopologyDownstream,
            operation,
            None,
            None,
        )
        .expect("downstream intent")
    }

    #[test]
    fn role_parser_round_trips_all_canonical_wire_values() {
        for role in [
            PeerEndpointRole::Disjoint,
            PeerEndpointRole::TopologyDownstream,
            PeerEndpointRole::RenoDxReshadeIni,
            PeerEndpointRole::OptiScalerConfig,
            PeerEndpointRole::DlssFix,
        ] {
            assert_eq!(PeerEndpointRole::parse(role.as_str()).expect("role"), role);
            assert_eq!(
                role.as_str().parse::<PeerEndpointRole>().expect("role"),
                role
            );
        }
        assert!(matches!(
            PeerEndpointRole::parse("unknown"),
            Err(PeerTransitionError::UnknownEndpointRole(value)) if value == "unknown"
        ));
    }

    #[test]
    fn config_enable_must_follow_downstream_acquisition() {
        let projection = config_projection(OptiConfigOperation::EnableLoadReshade);
        let config = projection
            .physical_intent()
            .expect("intent")
            .expect("config");
        let authorities = PeerTransitionAuthorities {
            topology_downstream: Some(PathRef::new("C:/Games/Test/ReShade64.dll").unwrap()),
            optiscaler_config: Some(projection),
            ..PeerTransitionAuthorities::empty()
        };
        validate_intents_with_authorities(
            ProxyPeerRoute::Coordinated(CoordinatedPeerOperation::Create),
            &[
                downstream("ReShade64.dll", PeerEndpointOperation::Create),
                config.clone(),
            ],
            &authorities,
        )
        .expect("enable order");
        assert!(matches!(
            validate_intents_with_authorities(
                ProxyPeerRoute::Coordinated(CoordinatedPeerOperation::Create),
                &[
                    config,
                    downstream("ReShade64.dll", PeerEndpointOperation::Create)
                ],
                &authorities,
            ),
            Err(PeerTransitionError::OptiScalerConfigOrder { .. })
        ));
    }

    #[test]
    fn config_disable_must_precede_downstream_release() {
        let projection = config_projection(OptiConfigOperation::DisableLoadReshade);
        let config = projection
            .physical_intent()
            .expect("intent")
            .expect("config");
        let authorities = PeerTransitionAuthorities {
            topology_downstream: Some(PathRef::new("C:/Games/Test/ReShade64.dll").unwrap()),
            optiscaler_config: Some(projection),
            ..PeerTransitionAuthorities::empty()
        };
        validate_intents_with_authorities(
            ProxyPeerRoute::Coordinated(CoordinatedPeerOperation::Remove),
            &[
                config.clone(),
                downstream("ReShade64.dll", PeerEndpointOperation::Remove),
            ],
            &authorities,
        )
        .expect("disable order");
        assert!(matches!(
            validate_intents_with_authorities(
                ProxyPeerRoute::Coordinated(CoordinatedPeerOperation::Remove),
                &[
                    downstream("ReShade64.dll", PeerEndpointOperation::Remove),
                    config
                ],
                &authorities,
            ),
            Err(PeerTransitionError::OptiScalerConfigOrder { .. })
        ));
    }

    #[test]
    fn no_change_cannot_be_encoded_as_a_physical_config_endpoint() {
        let projection = config_projection(OptiConfigOperation::NoChange);
        let authority = PeerTransitionAuthorities {
            optiscaler_config: Some(projection),
            ..PeerTransitionAuthorities::empty()
        };
        let intent = config_intent(OptiConfigOperation::EnableLoadReshade);
        assert!(matches!(
            validate_intents_with_authorities(
                ProxyPeerRoute::DurableDisjoint,
                &[intent],
                &authority
            ),
            Err(PeerTransitionError::OptiScalerConfigNoChange(_))
        ));
    }

    #[test]
    fn typed_authorities_allow_disjoint_reshade_ini_and_optiscaler_config() {
        let config = config_projection(OptiConfigOperation::NoChange);
        let root = root();
        let reshade = RenoDxReshadeIniAuthority::new(RenoDxReshadeIniFeature::Install, root)
            .expect("reshade authority");
        let reshade_intent = PeerEndpointIntent::replace(
            reshade.ini_path().clone(),
            PeerEndpointRole::RenoDxReshadeIni,
            None,
            None,
        )
        .expect("reshade intent");
        let authorities = PeerTransitionAuthorities {
            renodx_reshade_ini: Some(reshade),
            optiscaler_config: Some(config),
            ..PeerTransitionAuthorities::empty()
        };
        validate_intents_with_authorities(
            ProxyPeerRoute::DurableDisjoint,
            &[reshade_intent],
            &authorities,
        )
        .expect("coexisting authority");
    }

    #[test]
    fn coexisting_typed_authorities_must_share_the_canonical_root() {
        let config = config_projection(OptiConfigOperation::NoChange);
        let reshade_root = PathRef::new("C:/Games/Other").expect("root");
        let reshade =
            RenoDxReshadeIniAuthority::new(RenoDxReshadeIniFeature::Install, reshade_root)
                .expect("reshade authority");
        let authorities = PeerTransitionAuthorities {
            renodx_reshade_ini: Some(reshade),
            optiscaler_config: Some(config),
            ..PeerTransitionAuthorities::empty()
        };
        assert!(matches!(
            validate_intents_with_authorities(ProxyPeerRoute::DurableDisjoint, &[], &authorities),
            Err(PeerTransitionError::InvalidOptiScalerConfigTransition(_))
        ));
    }

    #[test]
    fn duplicate_config_paths_and_roles_fail_closed() {
        let projection = config_projection(OptiConfigOperation::NoChange);
        let path = projection.authority().config_path().clone();
        let first = PeerEndpointIntent::replace(
            path.clone(),
            PeerEndpointRole::OptiScalerConfig,
            None,
            None,
        )
        .expect("intent");
        let second =
            PeerEndpointIntent::replace(path, PeerEndpointRole::OptiScalerConfig, None, None)
                .expect("intent");
        let authorities = PeerTransitionAuthorities {
            optiscaler_config: Some(projection),
            ..PeerTransitionAuthorities::empty()
        };
        assert!(matches!(
            validate_intents_with_authorities(
                ProxyPeerRoute::DurableDisjoint,
                &[first, second],
                &authorities,
            ),
            Err(PeerTransitionError::DuplicateEndpoint(_))
        ));
    }

    #[test]
    fn dlss_fix_has_a_single_typed_role_and_claim_derived_operation() {
        let companion = PathRef::new("C:/Games/Test/nvngx_dlss.dll").expect("path");
        let projection = RenoDxDlssProjection::new(
            companion.clone(),
            RenoDxDlssBeforeImage::Absent,
            RenoDxDlssClaim::absent(),
            RenoDxDlssClaim::new(true, None).expect("claim"),
        );
        let authorities = PeerTransitionAuthorities {
            dlss_fix: Some(projection),
            ..PeerTransitionAuthorities::empty()
        };
        let intent =
            PeerEndpointIntent::create(companion.clone(), PeerEndpointRole::DlssFix, None, None)
                .expect("intent");
        validate_intents_with_authorities(
            ProxyPeerRoute::DurableDisjoint,
            std::slice::from_ref(&intent),
            &authorities,
        )
        .expect("dlss fix endpoint");

        let wrong_role =
            PeerEndpointIntent::create(companion, PeerEndpointRole::Disjoint, None, None)
                .expect("intent");
        assert!(matches!(
            validate_intents_with_authorities(
                ProxyPeerRoute::DurableDisjoint,
                &[wrong_role],
                &authorities,
            ),
            Err(PeerTransitionError::InvalidDlssFixPath(_))
        ));
    }

    fn dlss_physical_projection(
        before_image: RenoDxDlssBeforeImage,
        before_created: bool,
        after_created: bool,
    ) -> RenoDxDlssProjection {
        RenoDxDlssProjection::new(
            PathRef::new("C:/Games/Test/nvngx_dlss.dll").expect("path"),
            before_image,
            RenoDxDlssClaim::new(before_created, None).expect("before claim"),
            RenoDxDlssClaim::new(after_created, None).expect("after claim"),
        )
    }

    fn dlss_physical_intent(
        path: &str,
        role: PeerEndpointRole,
        operation: PeerEndpointOperation,
    ) -> PeerEndpointIntent {
        PeerEndpointIntent::new(
            PathRef::new(format!("C:/Games/Test/{path}")).expect("path"),
            role,
            operation,
            None,
            None,
        )
        .expect("intent")
    }

    #[test]
    fn physical_dlss_binding_accepts_exact_disjoint_create_replace_and_remove() {
        let create = dlss_physical_projection(RenoDxDlssBeforeImage::Absent, false, true);
        validate_dlss_physical_program(
            &[dlss_physical_intent(
                "NVNGX_DLSS.DLL",
                PeerEndpointRole::Disjoint,
                PeerEndpointOperation::Create,
            )],
            &create,
        )
        .expect("create companion");

        let replace = dlss_physical_projection(
            RenoDxDlssBeforeImage::present(
                "native-dlss",
                Sha256Hash::new("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855")
                    .expect("digest"),
                0,
                Vec::new(),
            )
            .expect("before image"),
            true,
            true,
        );
        validate_dlss_physical_program(
            &[dlss_physical_intent(
                "nvngx_dlss.dll",
                PeerEndpointRole::Disjoint,
                PeerEndpointOperation::Replace,
            )],
            &replace,
        )
        .expect("replace companion");

        let remove = dlss_physical_projection(RenoDxDlssBeforeImage::Absent, true, false);
        validate_dlss_physical_program(
            &[dlss_physical_intent(
                "nvngx_dlss.dll",
                PeerEndpointRole::Disjoint,
                PeerEndpointOperation::Remove,
            )],
            &remove,
        )
        .expect("remove companion");
    }

    #[test]
    fn physical_dlss_binding_allows_only_claim_only_source_slot_changes() {
        let source_refresh = dlss_physical_projection(
            RenoDxDlssBeforeImage::Present {
                identity: "native-dlss".to_owned(),
                sha256: Sha256Hash::new(
                    "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
                )
                .expect("digest"),
                length: 0,
                bytes: Vec::new(),
            },
            true,
            true,
        );
        validate_dlss_physical_program(&[], &source_refresh).expect("source refresh");

        let source_removal = dlss_physical_projection(RenoDxDlssBeforeImage::Absent, false, false);
        validate_dlss_physical_program(&[], &source_removal).expect("source removal");
    }

    #[test]
    fn physical_dlss_binding_rejects_created_delta_without_exact_companion() {
        let projection = dlss_physical_projection(RenoDxDlssBeforeImage::Absent, false, true);
        assert!(matches!(
            validate_dlss_physical_program(&[], &projection),
            Err(PeerTransitionError::InvalidDlssFixPath(_))
        ));

        let unrelated = dlss_physical_intent(
            "other.dll",
            PeerEndpointRole::Disjoint,
            PeerEndpointOperation::Create,
        );
        assert!(matches!(
            validate_dlss_physical_program(&[unrelated], &projection),
            Err(PeerTransitionError::InvalidDlssFixPath(_))
        ));
    }

    #[test]
    fn physical_dlss_binding_rejects_wrong_role_and_operation() {
        let create = dlss_physical_projection(RenoDxDlssBeforeImage::Absent, false, true);
        let wrong_role = dlss_physical_intent(
            "nvngx_dlss.dll",
            PeerEndpointRole::DlssFix,
            PeerEndpointOperation::Create,
        );
        assert!(matches!(
            validate_dlss_physical_program(&[wrong_role], &create),
            Err(PeerTransitionError::InvalidDlssFixPath(_))
        ));

        let wrong_operation = dlss_physical_intent(
            "nvngx_dlss.dll",
            PeerEndpointRole::Disjoint,
            PeerEndpointOperation::Replace,
        );
        assert!(matches!(
            validate_dlss_physical_program(&[wrong_operation], &create),
            Err(PeerTransitionError::InvalidDlssFixOperation(_))
        ));
    }
}
