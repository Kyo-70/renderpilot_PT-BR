use std::collections::{BTreeMap, HashSet};

use renderpilot_domain::{
    InstalledAddon, ManagedAddonFile, PathRef, normalized_path_key, normalized_path_relation,
};

use crate::addons::luma::peer::active_update::{
    error::LumaActiveUpdateError, model::LumaActiveUpdateClaimDelta,
};

pub(super) fn merge(
    before: &InstalledAddon,
    payload: LumaActiveUpdateClaimDelta,
    dgvoodoo: LumaActiveUpdateClaimDelta,
    addon_file: &PathRef,
) -> Result<(Vec<PathRef>, Vec<PathRef>), LumaActiveUpdateError> {
    let (payload_add_created, payload_remove_created, payload_add_backed, payload_remove_backed) =
        payload.into_parts();
    let (dg_add_created, dg_remove_created, dg_add_backed, dg_remove_backed) =
        dgvoodoo.into_parts();

    let add_created = collect_paths(
        "created additions",
        payload_add_created.into_iter().chain(dg_add_created),
    )?;
    let remove_created = collect_paths(
        "created removals",
        payload_remove_created.into_iter().chain(dg_remove_created),
    )?;
    let add_backed = collect_paths(
        "backed-up additions",
        payload_add_backed.into_iter().chain(dg_add_backed),
    )?;
    let remove_backed = collect_paths(
        "backed-up removals",
        payload_remove_backed.into_iter().chain(dg_remove_backed),
    )?;

    reject_overlap("created", &add_created, &remove_created)?;
    reject_overlap("backed-up", &add_backed, &remove_backed)?;

    let created = merge_claim_set(
        before.created_files(),
        &add_created,
        &remove_created,
        "created",
    )?;
    let backed_up = merge_claim_set(
        before.backed_up_files(),
        &add_backed,
        &remove_backed,
        "backed-up",
    )?;

    validate_backed_subset(&created, &backed_up)?;
    let addon_key = normalized_path_key(addon_file.as_str());
    if !created
        .iter()
        .any(|path| normalized_path_key(path.as_str()) == addon_key)
    {
        return Err(invalid_detail(format!(
            "main add-on is not present in final created claims: {addon_file}"
        )));
    }

    Ok((created, backed_up))
}

pub(super) fn validate_managed_disjoint(
    created: &[PathRef],
    backed_up: &[PathRef],
    managed: &[ManagedAddonFile],
) -> Result<(), LumaActiveUpdateError> {
    for binding in managed {
        for generic in created.iter().chain(backed_up) {
            if normalized_path_relation(binding.path().as_str(), generic.as_str()).overlaps() {
                return Err(invalid_detail(format!(
                    "managed endpoint overlaps a generic claim: {} and {}",
                    binding.path(),
                    generic
                )));
            }
        }
    }
    Ok(())
}

fn collect_paths(
    label: &str,
    paths: impl IntoIterator<Item = PathRef>,
) -> Result<BTreeMap<String, PathRef>, LumaActiveUpdateError> {
    let mut collected = BTreeMap::new();
    for path in paths {
        let key = normalized_path_key(path.as_str());
        if collected.insert(key, path.clone()).is_some() {
            return Err(invalid_detail(format!(
                "{label} contain a normalized duplicate: {path}"
            )));
        }
    }
    Ok(collected)
}

fn reject_overlap(
    label: &str,
    additions: &BTreeMap<String, PathRef>,
    removals: &BTreeMap<String, PathRef>,
) -> Result<(), LumaActiveUpdateError> {
    if let Some((key, path)) = additions
        .iter()
        .find(|(key, _)| removals.contains_key(*key))
    {
        return Err(invalid_detail(format!(
            "{label} claim is both added and removed in one update: {path} ({key})"
        )));
    }
    Ok(())
}

fn merge_claim_set(
    before: &[PathRef],
    additions: &BTreeMap<String, PathRef>,
    removals: &BTreeMap<String, PathRef>,
    label: &str,
) -> Result<Vec<PathRef>, LumaActiveUpdateError> {
    let mut existing = BTreeMap::new();
    for path in before {
        let key = normalized_path_key(path.as_str());
        if existing.insert(key, path.clone()).is_some() {
            return Err(invalid_detail(format!(
                "{label} claims contain a normalized duplicate: {path}"
            )));
        }
    }

    for path in removals.values() {
        let key = normalized_path_key(path.as_str());
        if existing.remove(&key).is_none() {
            return Err(invalid_detail(format!(
                "{label} removal does not match a persisted claim: {path}"
            )));
        }
    }

    for path in additions.values() {
        let key = normalized_path_key(path.as_str());
        if existing.contains_key(&key) {
            return Err(invalid_detail(format!(
                "{label} addition is not genuinely new: {path}"
            )));
        }
    }

    let removed: HashSet<&String> = removals.keys().collect();
    let mut result = before
        .iter()
        .filter(|path| !removed.contains(&normalized_path_key(path.as_str())))
        .cloned()
        .collect::<Vec<_>>();
    result.extend(additions.values().cloned());
    Ok(result)
}

fn validate_backed_subset(
    created: &[PathRef],
    backed_up: &[PathRef],
) -> Result<(), LumaActiveUpdateError> {
    let created_keys = created
        .iter()
        .map(|path| normalized_path_key(path.as_str()))
        .collect::<HashSet<_>>();
    if let Some(path) = backed_up
        .iter()
        .find(|path| !created_keys.contains(&normalized_path_key(path.as_str())))
    {
        return Err(invalid_detail(format!(
            "backed-up claim has no corresponding created claim: {path}"
        )));
    }
    Ok(())
}

fn invalid_detail(reason: String) -> LumaActiveUpdateError {
    LumaActiveUpdateError::invalid_input_detail(reason)
}
