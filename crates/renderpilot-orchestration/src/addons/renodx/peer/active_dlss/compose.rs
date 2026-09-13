use std::path::Path;

use renderpilot_domain::{
    InstalledAddon, ManagedAddonFile, NormalizedPathRelation, PathRef, PeerEndpointRole,
    PeerTransitionError, PlannedGameProxyTopology, RenoDxDlssClaim, RenoDxDlssProjection,
    RenoDxReshadeIniAuthority, RenoDxReshadeIniFeature, TrackedSourceRole, normalized_path_key,
    normalized_path_relation,
};

use crate::addons::renodx::source;
use crate::peer_mutation_executor::ExactEndpointProgram;

use super::effects::{before_image, lower_endpoint};
use super::error::ActiveDlssError;
use super::model::{ActiveDlssComposition, ActiveDlssEffect, ActiveDlssInput};

/// Composes one active-topology DLSS-Fix transition from sealed facts only.
pub(crate) fn compose_active_dlss(
    input: &ActiveDlssInput<'_>,
) -> Result<ActiveDlssComposition, ActiveDlssError> {
    let companion_path = validate_input(input)?;
    let before_claim = claim_for(input.before_peer, &companion_path)?;
    let after_claim = claim_for(input.after_peer, &companion_path)?;
    let projection = RenoDxDlssProjection::new(
        companion_path.clone(),
        before_image(&companion_path, input.companion.snapshot)?,
        before_claim.clone(),
        after_claim.clone(),
    );
    projection.validate_against_peers(Some(input.before_peer), Some(input.after_peer))?;
    validate_record_delta(input.before_peer, input.after_peer, &companion_path)?;
    validate_companion_effect(
        &before_claim,
        &after_claim,
        &input.companion.effect,
        input.companion.snapshot,
        &companion_path,
    )?;

    let mut lowered = Vec::with_capacity(2);
    if let Some(endpoint) = lower_endpoint(
        &input.companion.path,
        input.companion.snapshot,
        &input.companion.effect,
        PeerEndpointRole::Disjoint,
    )? {
        lowered.push(endpoint);
    }

    let authority_feature = validate_ini_input(input)?;
    if let Some(ini) = input.ini.as_ref()
        && let Some(endpoint) = lower_endpoint(
            &ini.path,
            ini.snapshot,
            &ini.effect,
            PeerEndpointRole::RenoDxReshadeIni,
        )?
    {
        lowered.push(endpoint);
    }
    if lowered.len() > 1
        && matches!(
            normalized_path_relation(
                lowered[0].endpoint.path().as_str(),
                lowered[1].endpoint.path().as_str()
            ),
            NormalizedPathRelation::Equal
        )
    {
        return Err(ActiveDlssError::Path(lowered[1].endpoint.path().clone()));
    }

    let planned_topology = PlannedGameProxyTopology::Exact(input.topology.clone());
    if lowered.is_empty() {
        return if before_claim == after_claim {
            Ok(ActiveDlssComposition::Noop)
        } else {
            Ok(ActiveDlssComposition::ClaimOnly {
                after_peer: input.after_peer.clone(),
                projection,
                planned_topology,
            })
        };
    }

    let mut endpoints = Vec::with_capacity(lowered.len());
    let mut payloads = Vec::with_capacity(lowered.len());
    let mut game_intents = Vec::with_capacity(lowered.len());
    for item in lowered {
        endpoints.push(item.endpoint);
        payloads.push(item.payload);
        game_intents.push(item.intent);
    }
    let program = ExactEndpointProgram::new(endpoints)?;
    if payloads.len() != program.endpoints().len() || game_intents.len() != payloads.len() {
        return Err(ActiveDlssError::Invalid(
            "active DLSS-Fix endpoint outputs are not aligned",
        ));
    }
    let reshade_ini_authority = authority_feature
        .filter(|_| {
            program
                .endpoints()
                .iter()
                .any(|endpoint| endpoint.role() == PeerEndpointRole::RenoDxReshadeIni)
        })
        .map(|feature| {
            RenoDxReshadeIniAuthority::new(feature, input.root.canonical_game_root_ref().clone())
        })
        .transpose()?;
    Ok(ActiveDlssComposition::Physical {
        after_peer: input.after_peer.clone(),
        projection,
        program,
        payloads,
        game_intents,
        planned_topology,
        reshade_ini_authority,
    })
}

fn validate_input(input: &ActiveDlssInput<'_>) -> Result<PathRef, ActiveDlssError> {
    for peer in [input.before_peer, input.after_peer] {
        if peer.kind() != renderpilot_domain::AddonKind::RenoDx
            || peer.game_id() != &input.topology.game_id
        {
            return Err(ActiveDlssError::Invalid(
                "DLSS-Fix requires RenoDX records for the active topology game",
            ));
        }
        if !peer.created_files().iter().any(|path| {
            matches!(
                normalized_path_relation(path.as_str(), peer.addon_file().as_str()),
                NormalizedPathRelation::Equal
            )
        }) {
            return Err(ActiveDlssError::Invalid(
                "RenoDX peer record does not claim its add-on payload",
            ));
        }
    }
    if input.before_peer.addon_file() != input.after_peer.addon_file() {
        return Err(ActiveDlssError::Invalid(
            "DLSS-Fix cannot change the main add-on path",
        ));
    }
    input
        .topology
        .validate()
        .map_err(|_| ActiveDlssError::Invalid("active topology is invalid"))?;
    input
        .root
        .roots()
        .require_game_path(&input.topology.root_slot)
        .map_err(|_| ActiveDlssError::Path(input.topology.root_slot.clone()))?;

    let addon = Path::new(input.before_peer.addon_file().as_str());
    let parent = addon
        .parent()
        .ok_or_else(|| ActiveDlssError::Path(input.before_peer.addon_file().clone()))?;
    let arch = crate::addons::renodx::arch_from_addon_file(input.before_peer.addon_file().as_str())
        .ok_or(ActiveDlssError::Invalid(
            "RenoDX add-on path has no supported architecture",
        ))?;
    let expected = path_ref(&parent.join(source::dlss_fix_file_name(arch)))?;
    if matches!(
        normalized_path_relation(input.before_peer.addon_file().as_str(), expected.as_str()),
        NormalizedPathRelation::Equal
    ) {
        return Err(ActiveDlssError::Invalid(
            "RenoDX main add-on collides with its DLSS-Fix companion",
        ));
    }
    input
        .root
        .roots()
        .require_sealed_path(input.before_peer.addon_file())
        .map_err(|_| ActiveDlssError::Path(input.before_peer.addon_file().clone()))?;
    input
        .root
        .roots()
        .require_sealed_path(&expected)
        .map_err(|_| ActiveDlssError::Path(expected.clone()))?;
    if !matches!(
        normalized_path_relation(input.companion.path.as_str(), expected.as_str()),
        NormalizedPathRelation::Equal
    ) {
        return Err(ActiveDlssError::Path(input.companion.path.clone()));
    }
    reject_noncanonical_candidates(input.before_peer, &expected)?;
    reject_noncanonical_candidates(input.after_peer, &expected)?;
    Ok(expected)
}

fn validate_ini_input(
    input: &ActiveDlssInput<'_>,
) -> Result<Option<RenoDxReshadeIniFeature>, ActiveDlssError> {
    match (&input.ini, input.ini_feature) {
        (None, None) => Ok(None),
        (None, Some(_)) => Err(ActiveDlssError::Invalid(
            "DLSS-Fix INI feature has no endpoint input",
        )),
        (Some(_), None) => Err(ActiveDlssError::Invalid(
            "DLSS-Fix INI endpoint has no typed feature",
        )),
        (Some(ini), Some(feature)) => {
            if !feature.is_dlss_fix() {
                return Err(ActiveDlssError::Domain(
                    PeerTransitionError::UnsupportedRenoDxReshadeIniFeature,
                ));
            }
            let authority = RenoDxReshadeIniAuthority::new(
                feature,
                input.root.canonical_game_root_ref().clone(),
            )?;
            if !authority.matches_ini_path(&ini.path)
                || input.root.roots().require_game_path(&ini.path).is_err()
            {
                return Err(ActiveDlssError::Path(ini.path.clone()));
            }
            Ok(Some(feature))
        }
    }
}

fn claim_for(peer: &InstalledAddon, path: &PathRef) -> Result<RenoDxDlssClaim, ActiveDlssError> {
    let created = peer
        .created_files()
        .iter()
        .filter(|candidate| {
            matches!(
                normalized_path_relation(candidate.as_str(), path.as_str()),
                NormalizedPathRelation::Equal
            )
        })
        .count();
    if created > 1 {
        return Err(ActiveDlssError::Domain(
            PeerTransitionError::RenoDxDlssPeerMismatch(path.clone()),
        ));
    }
    let mut sources = peer
        .tracked_sources()
        .iter()
        .filter(|source| source.role() == TrackedSourceRole::DlssFix);
    let first = sources.next().cloned();
    if sources.next().is_some() {
        return Err(ActiveDlssError::Domain(
            PeerTransitionError::InvalidRenoDxDlssClaim("peer has duplicate DlssFix source slots"),
        ));
    }
    Ok(RenoDxDlssClaim::new(created == 1, first)?)
}

fn validate_companion_effect(
    before: &RenoDxDlssClaim,
    after: &RenoDxDlssClaim,
    effect: &ActiveDlssEffect,
    snapshot: &crate::peer_mutation_executor::PeerPathSnapshot,
    path: &PathRef,
) -> Result<(), ActiveDlssError> {
    match effect {
        ActiveDlssEffect::Unchanged => {
            if before.created() != after.created() {
                return Err(ActiveDlssError::Invalid(
                    "DLSS-Fix ownership cannot change without a prepared endpoint effect",
                ));
            }
            if before.created() && snapshot.file().is_none() {
                return Err(ActiveDlssError::Invalid(
                    "owned DLSS-Fix companion is absent for an unchanged effect",
                ));
            }
            Ok(())
        }
        ActiveDlssEffect::Write(bytes) => {
            if !after.created() {
                return Err(ActiveDlssError::Domain(
                    PeerTransitionError::RenoDxDlssPeerMismatch(path.clone()),
                ));
            }
            if snapshot.file().is_some()
                && snapshot.bytes() != Some(bytes.as_slice())
                && !before.created()
                && before.source().is_some()
            {
                return Err(ActiveDlssError::Invalid(
                    "DLSS-Fix repair cannot replace a present unowned companion",
                ));
            }
            Ok(())
        }
        ActiveDlssEffect::Remove => {
            if after.source().is_some() {
                return Err(ActiveDlssError::Invalid(
                    "DLSS-Fix removal must clear its source claim",
                ));
            }
            if snapshot.file().is_none() && after.created() {
                return Err(ActiveDlssError::Domain(
                    PeerTransitionError::RenoDxDlssPeerMismatch(path.clone()),
                ));
            }
            if snapshot.file().is_some() && !before.created() {
                return Err(ActiveDlssError::Invalid(
                    "DLSS-Fix cannot remove a present unowned companion",
                ));
            }
            if snapshot.file().is_some() && after.created() {
                return Err(ActiveDlssError::Domain(
                    PeerTransitionError::RenoDxDlssPeerMismatch(path.clone()),
                ));
            }
            Ok(())
        }
    }
}

fn validate_record_delta(
    before: &InstalledAddon,
    after: &InstalledAddon,
    path: &PathRef,
) -> Result<(), ActiveDlssError> {
    if before.game_id() != after.game_id()
        || before.kind() != after.kind()
        || before.addon_file() != after.addon_file()
        || before.addon_version() != after.addon_version()
        || before.backed_up_files() != after.backed_up_files()
        || before.managed_files() != after.managed_files()
        || before.installed_at() != after.installed_at()
        || before.updated_at() != after.updated_at()
        || before.host_kind() != after.host_kind()
        || before.reshade_channel() != after.reshade_channel()
        || before.registered_exe_path() != after.registered_exe_path()
    {
        return Err(ActiveDlssError::Invalid(
            "active DLSS-Fix transition changed unrelated record fields",
        ));
    }
    if filtered_created(before, path) != filtered_created(after, path)
        || filtered_sources(before) != filtered_sources(after)
    {
        return Err(ActiveDlssError::Invalid(
            "active DLSS-Fix transition changed unrelated record ordering",
        ));
    }
    if let (Some(before), Some(after)) = (source_index(before), source_index(after))
        && before != after
    {
        return Err(ActiveDlssError::Invalid(
            "active DLSS-Fix transition moved its source slot",
        ));
    }
    Ok(())
}

fn filtered_created(peer: &InstalledAddon, path: &PathRef) -> Vec<String> {
    peer.created_files()
        .iter()
        .filter(|candidate| {
            !matches!(
                normalized_path_relation(candidate.as_str(), path.as_str()),
                NormalizedPathRelation::Equal
            )
        })
        .map(|candidate| normalized_path_key(candidate.as_str()))
        .collect()
}

fn filtered_sources(peer: &InstalledAddon) -> Vec<renderpilot_domain::TrackedSource> {
    peer.tracked_sources()
        .iter()
        .filter(|source| source.role() != TrackedSourceRole::DlssFix)
        .cloned()
        .collect()
}

fn source_index(peer: &InstalledAddon) -> Option<usize> {
    peer.tracked_sources()
        .iter()
        .position(|source| source.role() == TrackedSourceRole::DlssFix)
}

fn reject_noncanonical_candidates(
    peer: &InstalledAddon,
    expected: &PathRef,
) -> Result<(), ActiveDlssError> {
    for path in peer
        .created_files()
        .iter()
        .chain(peer.backed_up_files())
        .chain(peer.managed_files().iter().map(ManagedAddonFile::path))
    {
        if path
            .file_name()
            .is_some_and(source::is_dlss_fix_candidate_file_name)
            && !matches!(
                normalized_path_relation(path.as_str(), expected.as_str()),
                NormalizedPathRelation::Equal
            )
        {
            return Err(ActiveDlssError::Path(path.clone()));
        }
        if matches!(
            normalized_path_relation(path.as_str(), expected.as_str()),
            NormalizedPathRelation::Equal
        ) {
            if !peer.created_files().iter().any(|created| {
                matches!(
                    normalized_path_relation(created.as_str(), path.as_str()),
                    NormalizedPathRelation::Equal
                )
            }) {
                return Err(ActiveDlssError::Invalid(
                    "DLSS-Fix companion claim must be a created claim",
                ));
            }
            if peer.backed_up_files().iter().any(|backed| {
                matches!(
                    normalized_path_relation(backed.as_str(), path.as_str()),
                    NormalizedPathRelation::Equal
                )
            }) || peer.managed_files().iter().any(|managed| {
                matches!(
                    normalized_path_relation(managed.path().as_str(), path.as_str()),
                    NormalizedPathRelation::Equal
                )
            }) {
                return Err(ActiveDlssError::Invalid(
                    "DLSS-Fix companion cannot be a backed or managed claim",
                ));
            }
        }
    }
    Ok(())
}

fn path_ref(path: &Path) -> Result<PathRef, ActiveDlssError> {
    let path = path
        .to_str()
        .ok_or(ActiveDlssError::Invalid("DLSS-Fix path is not valid UTF-8"))?;
    PathRef::new(path.to_owned()).map_err(|_| ActiveDlssError::Invalid("DLSS-Fix path is invalid"))
}
