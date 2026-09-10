use std::collections::{BTreeMap, HashSet};

use renderpilot_domain::{AddonKind, InstalledAddon, PathRef, TrackedSource, TrackedSourceRole};

use crate::addons::luma::{
    dgvoodoo::PreparedDgVoodoo,
    peer::{
        active_dgvoodoo::{ActiveDgVoodooPlan, ActiveDgVoodooTargetViewPayload},
        active_update::error::LumaActiveUpdateError,
        root_authority::LumaPeerRootAuthority,
    },
};

use super::{
    guards,
    model::{Candidate, CandidateKind, CandidatePayload},
};

/// The complete ownership and dependency state validated before dgVoodoo
/// planning may observe or mutate any candidate endpoint.
type ValidatedDgVoodooState = (
    BTreeMap<String, PathRef>,
    BTreeMap<String, PathRef>,
    HashSet<String>,
);

pub(super) fn validate_before_and_dependencies(
    before: &InstalledAddon,
    authority: &LumaPeerRootAuthority,
    dependency_paths: &[PathRef],
) -> Result<ValidatedDgVoodooState, LumaActiveUpdateError> {
    if before.kind() != AddonKind::Luma {
        return Err(guards::invalid("active Luma update requires a Luma record"));
    }
    let dependencies = guards::validate_dependencies(authority, dependency_paths)?;
    let created = guards::collect_claim_map(before.created_files(), "created")?;
    let backed = guards::collect_claim_map(before.backed_up_files(), "backed-up")?;
    for key in backed.keys() {
        if !created.contains_key(key) {
            return Err(guards::invalid_detail(format!(
                "backed-up dependency claim has no corresponding created claim: {key}"
            )));
        }
    }
    guards::validate_active_aliases(before, &dependencies)?;
    Ok((created, backed, dependencies))
}

pub(super) fn validate_replace_sources(
    prepared: &PreparedDgVoodoo,
    refreshed_sources: &[TrackedSource],
) -> Result<(), LumaActiveUpdateError> {
    let wrappers = refreshed_sources
        .iter()
        .filter(|source| source.role() == TrackedSourceRole::DgVoodooWrapper)
        .collect::<Vec<_>>();
    if wrappers.len() != 1 || wrappers[0].is_advisory() {
        return Err(guards::invalid_detail(
            "replace requires exactly one non-advisory DgVoodooWrapper source",
        ));
    }
    let expected = prepared.tracked_source();
    if wrappers[0] != &expected {
        return Err(guards::invalid_detail(
            "refreshed DgVoodooWrapper source does not match the prepared archive",
        ));
    }
    Ok(())
}

pub(super) fn validate_remove_sources(
    refreshed_sources: &[TrackedSource],
) -> Result<(), LumaActiveUpdateError> {
    if refreshed_sources
        .iter()
        .any(|source| source.role() == TrackedSourceRole::DgVoodooWrapper)
    {
        return Err(guards::invalid(
            "remove requires no refreshed DgVoodooWrapper source",
        ));
    }
    Ok(())
}

pub(super) fn build_candidates(
    before: &InstalledAddon,
    authority: &LumaPeerRootAuthority,
    plan: Option<ActiveDgVoodooPlan>,
    created: &BTreeMap<String, PathRef>,
    backed: &BTreeMap<String, PathRef>,
    dependencies: &HashSet<String>,
) -> Result<Vec<Candidate>, LumaActiveUpdateError> {
    let mut candidates = Vec::new();
    let mut planned_endpoints = Vec::new();
    let mut desired = HashSet::new();
    if let Some(plan) = plan {
        let targets = plan.into_target_views();
        for target in targets {
            let live_key = renderpilot_domain::normalized_path_key(target.live().as_str());
            if !dependencies.contains(&live_key) {
                return Err(guards::invalid_detail(format!(
                    "desired dgVoodoo target is not listed as a dependency: {}",
                    target.live()
                )));
            }
            desired.insert(live_key.clone());
            guards::validate_target_aliases(before, authority, &target)?;
            planned_endpoints.push(target.live().clone());
            planned_endpoints.push(target.sidecar().clone());
            let owned = created.contains_key(&live_key);
            if matches!(
                target.payload(),
                ActiveDgVoodooTargetViewPayload::Config { .. }
            ) && !owned
            {
                continue;
            }
            let payload = target_payload(&target)?;
            candidates.push(Candidate {
                live: target.live().clone(),
                sidecar: target.sidecar().clone(),
                kind: CandidateKind::Desired(payload),
                created: owned,
                backed: backed.contains_key(&live_key),
            });
        }
    }

    for (key, live) in created {
        if dependencies.contains(key) && !desired.contains(key) {
            let sidecar = renderpilot_domain::managed_sidecar_path(live)
                .map_err(LumaActiveUpdateError::domain)?;
            authority
                .authorized_root(&sidecar)
                .map_err(LumaActiveUpdateError::authority)?;
            guards::validate_endpoint_against_active(before, &sidecar)?;
            planned_endpoints.push(live.clone());
            planned_endpoints.push(sidecar.clone());
            candidates.push(Candidate {
                live: live.clone(),
                sidecar,
                kind: CandidateKind::Removed,
                created: true,
                backed: backed.contains_key(key),
            });
        }
    }
    guards::validate_candidate_endpoints(before, &candidates, &planned_endpoints, dependencies)?;
    candidates.sort_by_key(Candidate::key);
    Ok(candidates)
}

fn target_payload(
    target: &crate::addons::luma::peer::active_dgvoodoo::ActiveDgVoodooTargetView,
) -> Result<CandidatePayload, LumaActiveUpdateError> {
    match target.payload() {
        ActiveDgVoodooTargetViewPayload::Runtime { bytes } => Ok(CandidatePayload::Runtime {
            bytes: bytes.clone(),
        }),
        ActiveDgVoodooTargetViewPayload::Config { default, sections } => {
            std::str::from_utf8(default).map_err(|_| {
                guards::invalid_detail(format!(
                    "dgVoodoo config default is not UTF-8: {}",
                    target.live()
                ))
            })?;
            Ok(CandidatePayload::Config {
                default: default.clone(),
                sections: sections.clone(),
            })
        }
    }
}
