use renderpilot_domain::{TrackedSource, TrackedSourceRole};
use std::path::PathBuf;

use crate::ServiceError;
use crate::addons::luma::fetch::types::LumaPayload;
use crate::addons::luma::source;

use super::invalid;

pub(super) fn convert_dependency_paths(
    paths: &[PathBuf],
) -> Result<Vec<renderpilot_domain::PathRef>, ServiceError> {
    paths
        .iter()
        .map(|path| {
            let text = path.to_str().ok_or_else(|| {
                invalid(format!(
                    "active Luma dependency path is not valid UTF-8: {}",
                    path.display()
                ))
            })?;
            renderpilot_domain::PathRef::new(text.to_owned()).map_err(|error| {
                invalid(format!(
                    "invalid active Luma dependency path `{text}`: {error}"
                ))
            })
        })
        .collect()
}

pub(super) fn single_role<'a>(
    sources: &'a [TrackedSource],
    role: TrackedSourceRole,
    label: &str,
) -> Result<Option<(usize, &'a TrackedSource)>, ServiceError> {
    let mut found = sources
        .iter()
        .enumerate()
        .filter(|(_, source)| source.role() == role);
    let first = found.next();
    if found.next().is_some() {
        return Err(invalid(format!(
            "active Luma record has duplicate {label} provenance"
        )));
    }
    Ok(first)
}

pub(super) fn require_payload(
    sources: &[TrackedSource],
) -> Result<(usize, &TrackedSource), ServiceError> {
    single_role(sources, TrackedSourceRole::AddonPayload, "add-on payload")?
        .ok_or_else(|| invalid("active Luma record is missing add-on payload provenance"))
}

pub(super) fn replace_payload(
    sources: &mut [TrackedSource],
    asset: &str,
    payload: &LumaPayload,
) -> Result<(), ServiceError> {
    let (index, _) = require_payload(sources)?;
    sources[index] = TrackedSource::new(
        TrackedSourceRole::AddonPayload,
        source::asset_url(asset),
        payload.etag.clone(),
        payload.zip_digest.clone(),
    )
    .with_last_modified(payload.last_modified.clone());
    Ok(())
}

pub(super) fn replace_host(
    sources: &mut Vec<TrackedSource>,
    replacement: TrackedSource,
) -> Result<(), ServiceError> {
    replace_or_insert(
        sources,
        TrackedSourceRole::HostBinary,
        replacement,
        |sources| require_payload(sources).map(|(index, _)| index + 1),
    )
}

pub(super) fn replace_dgvoodoo(
    sources: &mut [TrackedSource],
    replacement: TrackedSource,
) -> Result<(), ServiceError> {
    let (index, _) = single_role(
        sources,
        TrackedSourceRole::DgVoodooWrapper,
        "dgVoodoo wrapper",
    )?
    .ok_or_else(|| invalid("active Luma record is missing dgVoodoo wrapper provenance"))?;
    sources[index] = replacement;
    Ok(())
}

pub(super) fn remove_role(sources: &mut Vec<TrackedSource>, role: TrackedSourceRole) {
    sources.retain(|source| source.role() != role);
}

fn replace_or_insert(
    sources: &mut Vec<TrackedSource>,
    role: TrackedSourceRole,
    replacement: TrackedSource,
    insert_at: impl FnOnce(&[TrackedSource]) -> Result<usize, ServiceError>,
) -> Result<(), ServiceError> {
    let indices: Vec<usize> = sources
        .iter()
        .enumerate()
        .filter_map(|(index, source)| (source.role() == role).then_some(index))
        .collect();
    if indices.len() > 1 {
        return Err(invalid(format!(
            "active Luma record has duplicate {role:?} provenance"
        )));
    }
    if let Some(index) = indices.first().copied() {
        sources[index] = replacement;
    } else {
        let index = insert_at(sources)?;
        sources.insert(index, replacement);
    }
    Ok(())
}
