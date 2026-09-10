use std::collections::{BTreeMap, HashSet};

use renderpilot_domain::{
    AddonKind, InstalledAddon, NormalizedPathRelation, PathRef, TrackedSource, TrackedSourceRole,
    managed_sidecar_path, normalized_path_key, normalized_path_relation,
};

use crate::addons::luma::{fetch::types::LumaPayload, peer::root_authority::LumaPeerRootAuthority};

use super::{
    super::error::LumaActiveUpdateError,
    model::{Candidate, CandidatePlan, FreshTarget},
};

pub(super) fn validate_before_record(before: &InstalledAddon) -> Result<(), LumaActiveUpdateError> {
    if before.kind() != AddonKind::Luma {
        return Err(LumaActiveUpdateError::invalid_input(
            "active Luma update requires a Luma record",
        ));
    }
    if !before.created_files().iter().any(|path| {
        normalized_path_key(path.as_str()) == normalized_path_key(before.addon_file().as_str())
    }) {
        return Err(LumaActiveUpdateError::invalid_input(
            "active Luma record does not claim its main add-on in generic created files",
        ));
    }
    Ok(())
}

/// Full updates require exactly one authoritative payload source. Advisory
/// payload entries are deliberately rejected even when an authoritative entry
/// is also present: they cannot participate in source identity and allowing
/// them would make malformed persisted source lists silently look valid.
pub(super) fn validate_payload_source(
    payload: &LumaPayload,
    refreshed_sources: &[TrackedSource],
) -> Result<(), LumaActiveUpdateError> {
    let sources: Vec<&TrackedSource> = refreshed_sources
        .iter()
        .filter(|source| source.role() == TrackedSourceRole::AddonPayload)
        .collect();
    let authoritative: Vec<&TrackedSource> = sources
        .iter()
        .copied()
        .filter(|source| !source.is_advisory())
        .collect();
    if authoritative.len() != 1 {
        return Err(LumaActiveUpdateError::invalid_input_detail(format!(
            "expected exactly one refreshed non-advisory AddonPayload source, found {}",
            authoritative.len()
        )));
    }
    if sources.iter().any(|source| source.is_advisory()) {
        return Err(LumaActiveUpdateError::invalid_input(
            "refreshed AddonPayload sources must not include advisory entries",
        ));
    }
    let source = authoritative[0];
    if source.url().trim().is_empty() {
        return Err(LumaActiveUpdateError::invalid_input(
            "refreshed AddonPayload source URL is empty",
        ));
    }
    if source.digest() != payload.zip_digest
        || source.etag() != payload.etag.as_deref()
        || source.last_modified() != payload.last_modified.as_deref()
    {
        return Err(LumaActiveUpdateError::invalid_input_detail(
            "refreshed AddonPayload source does not match the prepared payload identity".to_owned(),
        ));
    }
    Ok(())
}

pub(super) fn build_candidate_plan(
    before: &InstalledAddon,
    authority: &LumaPeerRootAuthority,
    dependency_paths: &[PathRef],
    fresh_targets: Vec<FreshTarget>,
) -> Result<CandidatePlan, LumaActiveUpdateError> {
    let dependency_keys = validate_dependencies(authority, dependency_paths)?;
    let created = collect_claim_map(before.created_files(), "created")?;
    let backed_up = collect_claim_map(before.backed_up_files(), "backed-up")?;
    validate_backed_up_subset(&created, &backed_up)?;
    validate_managed_claim_disjointness(
        before,
        &created,
        &backed_up,
        &dependency_keys,
        &fresh_targets,
    )?;
    validate_existing_claim_roots(authority, &created, &backed_up, &dependency_keys)?;

    let mut candidates = Vec::with_capacity(fresh_targets.len() + created.len());
    let mut fresh_keys = HashSet::with_capacity(fresh_targets.len());
    for target in fresh_targets {
        let FreshTarget {
            live: fresh_live,
            sidecar: fresh_sidecar,
            bytes,
        } = target;
        let key = normalized_path_key(fresh_live.as_str());
        if !fresh_keys.insert(key.clone()) {
            return Err(LumaActiveUpdateError::invalid_input_detail(format!(
                "fresh payload target is duplicated after normalization: {fresh_live}"
            )));
        }
        let retained_live = created.get(&key).cloned();
        let backed_path = backed_up.get(&key).cloned();
        let sidecar = retained_live
            .as_ref()
            .map(Candidate::expected_sidecar_for)
            .transpose()
            .map_err(LumaActiveUpdateError::domain)?
            .unwrap_or(fresh_sidecar);
        candidates.push(Candidate::Fresh {
            fresh_live,
            sidecar,
            bytes,
            retained_live,
            backed: backed_path.is_some(),
            backed_path,
        });
    }
    for (key, live) in &created {
        if dependency_keys.contains(key) || fresh_keys.contains(key) {
            continue;
        }
        let backed_path = backed_up.get(key).cloned();
        let sidecar = managed_sidecar_path(live).map_err(LumaActiveUpdateError::domain)?;
        candidates.push(Candidate::Removed {
            live: live.clone(),
            sidecar,
            backed: backed_path.is_some(),
            backed_path,
        });
    }
    candidates.sort_by_key(Candidate::key);
    validate_dependency_disjointness(&dependency_keys, &candidates)?;
    ensure_candidate_paths_are_unique(&candidates)?;
    Ok(CandidatePlan { candidates })
}

fn validate_dependencies(
    authority: &LumaPeerRootAuthority,
    dependency_paths: &[PathRef],
) -> Result<HashSet<String>, LumaActiveUpdateError> {
    let mut keys = HashSet::with_capacity(dependency_paths.len());
    for path in dependency_paths {
        let key = normalized_path_key(path.as_str());
        if !keys.insert(key) {
            return Err(LumaActiveUpdateError::invalid_input_detail(format!(
                "dependency path is duplicated after normalization: {path}"
            )));
        }
        authority
            .authorized_root(path)
            .map_err(LumaActiveUpdateError::authority)?;
    }
    Ok(keys)
}

fn collect_claim_map(
    paths: &[PathRef],
    label: &str,
) -> Result<BTreeMap<String, PathRef>, LumaActiveUpdateError> {
    let mut map = BTreeMap::new();
    for path in paths {
        let key = normalized_path_key(path.as_str());
        if map.insert(key, path.clone()).is_some() {
            return Err(LumaActiveUpdateError::invalid_input_detail(format!(
                "{label} generic claim is duplicated after normalization: {path}"
            )));
        }
    }
    Ok(map)
}

fn validate_backed_up_subset(
    created: &BTreeMap<String, PathRef>,
    backed_up: &BTreeMap<String, PathRef>,
) -> Result<(), LumaActiveUpdateError> {
    for key in backed_up.keys() {
        if !created.contains_key(key) {
            return Err(LumaActiveUpdateError::invalid_input_detail(format!(
                "backed-up generic claim has no corresponding created claim: {key}"
            )));
        }
    }
    Ok(())
}

fn validate_existing_claim_roots(
    authority: &LumaPeerRootAuthority,
    created: &BTreeMap<String, PathRef>,
    backed_up: &BTreeMap<String, PathRef>,
    dependency_keys: &HashSet<String>,
) -> Result<(), LumaActiveUpdateError> {
    let root = effective_root(authority);
    for (key, path) in created.iter().chain(backed_up.iter()) {
        if dependency_keys.contains(key) {
            continue;
        }
        if !strictly_under_root(path, root) {
            return Err(LumaActiveUpdateError::invalid_input_detail(format!(
                "generic claim is outside the sealed effective payload root: {path}"
            )));
        }
    }
    Ok(())
}

fn validate_managed_claim_disjointness(
    before: &InstalledAddon,
    created: &BTreeMap<String, PathRef>,
    backed_up: &BTreeMap<String, PathRef>,
    dependency_keys: &HashSet<String>,
    fresh_targets: &[FreshTarget],
) -> Result<(), LumaActiveUpdateError> {
    let generic_keys: HashSet<&String> = created.keys().chain(backed_up.keys()).collect();
    for managed in before.managed_files() {
        let managed_key = normalized_path_key(managed.path().as_str());
        if generic_keys.contains(&managed_key) {
            return Err(LumaActiveUpdateError::invalid_input_detail(format!(
                "managed path aliases a generic claim: {}",
                managed.path()
            )));
        }
        if dependency_keys.contains(&managed_key) {
            return Err(LumaActiveUpdateError::invalid_input_detail(format!(
                "managed path aliases a dependency claim: {}",
                managed.path()
            )));
        }
        if fresh_targets.iter().any(|target| {
            managed_key == normalized_path_key(target.live.as_str())
                || managed_key == normalized_path_key(target.sidecar.as_str())
        }) {
            return Err(LumaActiveUpdateError::invalid_input_detail(format!(
                "fresh payload endpoint aliases a managed path: {}",
                managed.path()
            )));
        }
    }
    Ok(())
}

fn validate_dependency_disjointness(
    dependency_keys: &HashSet<String>,
    candidates: &[Candidate],
) -> Result<(), LumaActiveUpdateError> {
    for candidate in candidates {
        for path in [candidate.live(), candidate.sidecar()] {
            let endpoint_key = normalized_path_key(path.as_str());
            if dependency_keys.iter().any(|dependency_key| {
                normalized_path_relation(dependency_key, &endpoint_key).overlaps()
            }) {
                return Err(LumaActiveUpdateError::invalid_input_detail(format!(
                    "dependency path overlaps selected payload endpoint: {path}"
                )));
            }
        }
    }
    Ok(())
}

fn ensure_candidate_paths_are_unique(
    candidates: &[Candidate],
) -> Result<(), LumaActiveUpdateError> {
    let mut paths = BTreeMap::new();
    for candidate in candidates {
        for path in [candidate.live(), candidate.sidecar()] {
            let key = normalized_path_key(path.as_str());
            if paths.insert(key, path).is_some() {
                return Err(LumaActiveUpdateError::invalid_input_detail(format!(
                    "payload endpoint aliases another selected endpoint: {path}"
                )));
            }
        }
    }
    for (index, (_, path)) in paths.iter().enumerate() {
        for (_, other) in paths.iter().skip(index + 1) {
            let (ancestor, descendant) =
                match normalized_path_relation(path.as_str(), other.as_str()) {
                    NormalizedPathRelation::LeftAncestor => (*path, *other),
                    NormalizedPathRelation::RightAncestor => (*other, *path),
                    NormalizedPathRelation::Equal | NormalizedPathRelation::Disjoint => continue,
                };
            return Err(LumaActiveUpdateError::invalid_input_detail(format!(
                "payload endpoints overlap: {ancestor} contains {descendant}"
            )));
        }
    }
    Ok(())
}

pub(super) fn projected_main_addon(before: &InstalledAddon, fresh_main: &PathRef) -> PathRef {
    if normalized_path_key(before.addon_file().as_str()) == normalized_path_key(fresh_main.as_str())
    {
        before.addon_file().clone()
    } else {
        fresh_main.clone()
    }
}

pub(super) fn effective_root(authority: &LumaPeerRootAuthority) -> &PathRef {
    authority.effective_addon_root_ref()
}

fn strictly_under_root(path: &PathRef, root: &PathRef) -> bool {
    matches!(
        normalized_path_relation(root.as_str(), path.as_str()),
        NormalizedPathRelation::LeftAncestor
    )
}

#[cfg(test)]
mod tests {
    use renderpilot_domain::PathRef;

    use super::strictly_under_root;

    #[test]
    fn strictly_under_root_accepts_a_child_of_a_drive_root() {
        let root = PathRef::new("c:/").expect("root");
        let child = PathRef::new("c:/payload/addon.dll").expect("child");

        assert!(strictly_under_root(&child, &root));
    }
}
