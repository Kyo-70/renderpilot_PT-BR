use std::collections::{BTreeMap, HashSet};

use renderpilot_domain::{
    InstalledAddon, NormalizedPathRelation, PathRef, normalized_path_key, normalized_path_relation,
};

use crate::addons::luma::peer::{
    active_dgvoodoo::ActiveDgVoodooTargetView, active_update::error::LumaActiveUpdateError,
    root_authority::LumaPeerRootAuthority,
};

use super::model::Candidate;

pub(super) fn validate_dependencies(
    authority: &LumaPeerRootAuthority,
    dependency_paths: &[PathRef],
) -> Result<HashSet<String>, LumaActiveUpdateError> {
    let mut keys = HashSet::with_capacity(dependency_paths.len());
    for path in dependency_paths {
        let key = normalized_path_key(path.as_str());
        if !keys.insert(key) {
            return Err(invalid_detail(format!(
                "dependency path is duplicated after normalization: {path}"
            )));
        }
        if !strictly_under(path, authority.canonical_game_root_ref()) {
            return Err(invalid_detail(format!(
                "dependency path is not strictly under the sealed canonical game root: {path}"
            )));
        }
        authority
            .authorized_root(path)
            .map_err(LumaActiveUpdateError::authority)?;
    }
    for (index, path) in dependency_paths.iter().enumerate() {
        for other in dependency_paths.iter().skip(index + 1) {
            if normalized_path_relation(path.as_str(), other.as_str()).overlaps() {
                return Err(invalid_detail(format!(
                    "dependency paths overlap: {path} and {other}"
                )));
            }
        }
    }
    Ok(keys)
}

pub(super) fn collect_claim_map(
    paths: &[PathRef],
    label: &str,
) -> Result<BTreeMap<String, PathRef>, LumaActiveUpdateError> {
    let mut result = BTreeMap::new();
    for path in paths {
        let key = normalized_path_key(path.as_str());
        if result.insert(key, path.clone()).is_some() {
            return Err(invalid_detail(format!(
                "{label} dependency claim is duplicated after normalization: {path}"
            )));
        }
    }
    Ok(result)
}

pub(super) fn validate_active_aliases(
    before: &InstalledAddon,
    dependencies: &HashSet<String>,
) -> Result<(), LumaActiveUpdateError> {
    let mut protected = before
        .managed_files()
        .iter()
        .map(|file| file.path().clone())
        .collect::<Vec<_>>();
    protected.push(before.addon_file().clone());
    if let Some(path) = before.registered_exe_path() {
        protected.push(path.clone());
    }
    for path in &protected {
        for dependency in dependencies {
            if normalized_path_relation(path.as_str(), dependency).overlaps() {
                return Err(invalid_detail(format!(
                    "dgVoodoo dependency aliases an active Luma path: {path}"
                )));
            }
        }
    }
    Ok(())
}

pub(super) fn validate_target_aliases(
    before: &InstalledAddon,
    authority: &LumaPeerRootAuthority,
    target: &ActiveDgVoodooTargetView,
) -> Result<(), LumaActiveUpdateError> {
    for path in [target.live(), target.sidecar()] {
        authority
            .authorized_root(path)
            .map_err(LumaActiveUpdateError::authority)?;
        validate_endpoint_against_active(before, path)?;
    }
    Ok(())
}

pub(super) fn validate_endpoint_against_active(
    before: &InstalledAddon,
    path: &PathRef,
) -> Result<(), LumaActiveUpdateError> {
    if before
        .managed_files()
        .iter()
        .any(|managed| normalized_path_relation(path.as_str(), managed.path().as_str()).overlaps())
        || normalized_path_relation(path.as_str(), before.addon_file().as_str()).overlaps()
        || before.registered_exe_path().is_some_and(|registered| {
            normalized_path_relation(path.as_str(), registered.as_str()).overlaps()
        })
    {
        return Err(invalid_detail(format!(
            "dgVoodoo endpoint aliases an active Luma path: {path}"
        )));
    }
    Ok(())
}

pub(super) fn validate_candidate_endpoints(
    before: &InstalledAddon,
    candidates: &[Candidate],
    planned_endpoints: &[PathRef],
    dependencies: &HashSet<String>,
) -> Result<(), LumaActiveUpdateError> {
    if !planned_endpoints.len().is_multiple_of(2) {
        return Err(invalid_detail(
            "dgVoodoo endpoint planning produced an incomplete live/sidecar pair",
        ));
    }
    let mut endpoints = BTreeMap::<String, &PathRef>::new();
    for candidate in candidates {
        if !dependencies.contains(&normalized_path_key(candidate.live.as_str())) {
            return Err(invalid_detail(format!(
                "owned dgVoodoo candidate is not listed as a dependency: {}",
                candidate.live
            )));
        }
    }
    for pair in planned_endpoints.as_chunks::<2>().0 {
        let live = &pair[0];
        let sidecar = &pair[1];
        for path in [live, sidecar] {
            let key = normalized_path_key(path.as_str());
            if endpoints.insert(key, path).is_some() {
                return Err(invalid_detail(format!(
                    "dgVoodoo candidate endpoints alias each other: {path}"
                )));
            }
        }
        if dependencies
            .iter()
            .any(|dependency| normalized_path_relation(sidecar.as_str(), dependency).overlaps())
        {
            return Err(invalid_detail(format!(
                "dgVoodoo sidecar collides with a dependency path: {sidecar}"
            )));
        }
        for claim in before
            .created_files()
            .iter()
            .chain(before.backed_up_files())
        {
            if normalized_path_relation(sidecar.as_str(), claim.as_str()).overlaps()
                || (normalized_path_relation(live.as_str(), claim.as_str()).overlaps()
                    && normalized_path_key(live.as_str()) != normalized_path_key(claim.as_str()))
            {
                return Err(invalid_detail(format!(
                    "dgVoodoo endpoint collides with an existing generic claim: {claim}"
                )));
            }
        }
    }
    for (index, (_, path)) in endpoints.iter().enumerate() {
        for (_, other) in endpoints.iter().skip(index + 1) {
            let descendant = match normalized_path_relation(path.as_str(), other.as_str()) {
                NormalizedPathRelation::LeftAncestor => *other,
                NormalizedPathRelation::RightAncestor => *path,
                NormalizedPathRelation::Equal | NormalizedPathRelation::Disjoint => continue,
            };
            return Err(invalid_detail(format!(
                "dgVoodoo candidate endpoints overlap: {descendant}"
            )));
        }
    }
    Ok(())
}

fn strictly_under(path: &PathRef, root: &PathRef) -> bool {
    if !matches!(
        normalized_path_relation(root.as_str(), path.as_str()),
        NormalizedPathRelation::LeftAncestor
    ) {
        return false;
    }
    let path_key = normalized_path_key(path.as_str());
    let root_key = normalized_path_key(root.as_str());
    let suffix = if root_key.ends_with('/') {
        path_key.strip_prefix(&root_key)
    } else {
        path_key
            .strip_prefix(&root_key)
            .and_then(|suffix| suffix.strip_prefix('/'))
    };
    let Some(suffix) = suffix else {
        return false;
    };
    !suffix.is_empty()
        && !suffix
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
}

pub(super) fn invalid_detail(reason: impl Into<String>) -> LumaActiveUpdateError {
    LumaActiveUpdateError::invalid_input_detail(reason.into())
}

pub(super) fn invalid(reason: &'static str) -> LumaActiveUpdateError {
    LumaActiveUpdateError::invalid_input(reason)
}

#[cfg(test)]
mod tests {
    use renderpilot_domain::PathRef;

    use super::strictly_under;

    #[test]
    fn strictly_under_accepts_a_child_of_a_drive_root() {
        let root = PathRef::new("c:/").expect("root");
        let child = PathRef::new("c:/dgvoodoo/d3d11.dll").expect("child");

        assert!(strictly_under(&child, &root));
    }
}
