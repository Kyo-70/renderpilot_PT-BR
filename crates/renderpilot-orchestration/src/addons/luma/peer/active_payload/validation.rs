use std::{
    collections::{BTreeMap, HashSet},
    path::{Path, PathBuf},
};

use renderpilot_domain::{
    NormalizedPathRelation, PathRef, normalized_path_key, normalized_path_relation,
};

use crate::{addons::engine, addons::luma::fetch::types::LumaPayloadFile};

use super::super::root_authority::LumaPeerRootAuthority;
use super::model::{
    ActivePayloadAuthorityError, ActivePayloadError, ActivePayloadStructuralError, GenericTarget,
    ValidatedActivePayload, ValidatedActivePayloadTarget, ValidatedPayloadFile,
};

const DLSS_FILE_NAME: &str = renderpilot_detection::NVNGX_DLSS_FILE_NAME;

/// Structural payload projection before it is converted into the public
/// validated representation. The three values stay coupled: `main_addon`
/// names one of `targets`, while `dlss_bytes` is deliberately excluded from
/// the generic target list.
type ValidatedPayloadStructure = (PathRef, Vec<GenericTarget>, Option<Vec<u8>>);

/// Validates the complete extracted payload against the sealed effective
/// payload root without touching the filesystem.  Both initial install and
/// active update lowering consume this single boundary.
pub(crate) fn validate_active_payload(
    authority: &LumaPeerRootAuthority,
    payload: Vec<LumaPayloadFile>,
    main_addon_rel: &str,
) -> Result<ValidatedActivePayload, ActivePayloadError> {
    let root = effective_root(authority);
    let (main_addon, targets, dlss_bytes) =
        validate_structure(authority, root, payload, main_addon_rel)?;
    let targets = targets
        .into_iter()
        .map(|target| {
            let GenericTarget {
                live,
                sidecar,
                bytes,
            } = target;
            ValidatedActivePayloadTarget::new(live, sidecar, bytes)
        })
        .collect();
    Ok(ValidatedActivePayload::new(main_addon, targets, dlss_bytes))
}

pub(super) fn validate_structure(
    authority: &LumaPeerRootAuthority,
    root: &PathRef,
    payload: Vec<LumaPayloadFile>,
    main_addon_rel: &str,
) -> Result<ValidatedPayloadStructure, ActivePayloadError> {
    let main_relative = validate_relative_path("main add-on", main_addon_rel)?;
    let main_name = main_relative
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| ActivePayloadStructuralError::InvalidMainAddon {
            value: main_addon_rel.to_owned(),
        })?;
    if main_relative.components().count() != 1
        || !main_name.to_ascii_lowercase().ends_with(".addon")
        || main_name.eq_ignore_ascii_case(DLSS_FILE_NAME)
    {
        return Err(ActivePayloadStructuralError::InvalidMainAddon {
            value: main_addon_rel.to_owned(),
        }
        .into());
    }

    let mut paths = Vec::with_capacity(payload.len());
    let mut path_keys = HashSet::with_capacity(payload.len());
    for file in payload {
        let relative = validate_relative_path("payload", &file.relative_path)?;
        let relative_key = normalized_path_key(&relative.to_string_lossy());
        if !path_keys.insert(relative_key.clone()) {
            let path = make_path_ref(root, &relative)?;
            let duplicate_error = if is_exact_dlss_relative(&relative) {
                ActivePayloadStructuralError::DuplicateDlss(path)
            } else {
                ActivePayloadStructuralError::DuplicateTarget(path)
            };
            return Err(duplicate_error.into());
        }
        paths.push(ValidatedPayloadFile {
            relative,
            key: relative_key,
            bytes: file.bytes,
        });
    }

    let main_key = normalized_path_key(&main_relative.to_string_lossy());
    let root_addons: Vec<&ValidatedPayloadFile> = paths
        .iter()
        .filter(|entry| {
            entry.relative.components().count() == 1
                && entry
                    .relative
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.to_ascii_lowercase().ends_with(".addon"))
        })
        .collect();
    if root_addons.len() > 1 {
        return Err(ActivePayloadStructuralError::MultipleMainAddons {
            first: make_path_ref(root, &root_addons[0].relative)?,
            second: make_path_ref(root, &root_addons[1].relative)?,
        }
        .into());
    }
    if !paths.iter().any(|entry| entry.key == main_key) {
        return Err(ActivePayloadStructuralError::MissingMainAddon {
            value: main_addon_rel.to_owned(),
        }
        .into());
    }
    let main_path = make_path_ref(root, &main_relative)?;

    let mut targets = Vec::new();
    let mut dlss_bytes = None;
    let mut endpoint_paths = Vec::new();
    for entry in paths {
        let live = make_path_ref(root, &entry.relative)?;
        let file_name = entry.relative.file_name().and_then(|name| name.to_str());
        authority_check(authority, &live)?;
        if is_exact_dlss_relative(&entry.relative) {
            // DLSS is validated by the dedicated planner and is not a generic
            // effect or record path.
            dlss_bytes = Some(entry.bytes);
            continue;
        }
        if is_reserved_generic_target(file_name) {
            return Err(ActivePayloadStructuralError::ReservedTarget(live).into());
        }
        let sidecar = renderpilot_domain::managed_sidecar_path(&live)
            .map_err(ActivePayloadStructuralError::SidecarPath)?;
        authority_check(authority, &sidecar)?;
        endpoint_paths.push(live.clone());
        endpoint_paths.push(sidecar.clone());
        targets.push(GenericTarget {
            live,
            sidecar,
            bytes: entry.bytes,
        });
    }

    if targets.is_empty() {
        return Err(ActivePayloadStructuralError::MissingMainAddon {
            value: main_addon_rel.to_owned(),
        }
        .into());
    }

    validate_endpoint_overlaps(&endpoint_paths)?;

    // Duplicate normalized payload paths were rejected above. Return the
    // payload target's spelling, not an aliased caller anchor spelling.
    let main = targets
        .iter()
        .find(|target| {
            normalized_path_key(target.live.as_str()) == normalized_path_key(main_path.as_str())
        })
        .map(|target| target.live.clone())
        .ok_or_else(|| ActivePayloadStructuralError::MissingMainAddon {
            value: main_addon_rel.to_owned(),
        })?;
    Ok((main, targets, dlss_bytes))
}

fn validate_relative_path(field: &'static str, raw: &str) -> Result<PathBuf, ActivePayloadError> {
    let relative = engine::ensure_safe_relative_path(field, raw).map_err(|_| {
        ActivePayloadStructuralError::InvalidRelativePath {
            field,
            value: raw.to_owned(),
        }
    })?;
    if raw
        .split(['/', '\\'])
        .any(|component| component.trim() != component)
    {
        return Err(ActivePayloadStructuralError::InvalidRelativePath {
            field,
            value: raw.to_owned(),
        }
        .into());
    }
    Ok(relative)
}

fn make_path_ref(root: &PathRef, relative: &Path) -> Result<PathRef, ActivePayloadError> {
    PathRef::new(
        Path::new(root.as_str())
            .join(relative)
            .to_string_lossy()
            .into_owned(),
    )
    .map_err(|error| ActivePayloadStructuralError::PathConversion(error).into())
}

fn authority_check(
    authority: &LumaPeerRootAuthority,
    path: &PathRef,
) -> Result<(), ActivePayloadError> {
    authority
        .authorized_root(path)
        .map(|_| ())
        .map_err(|error| {
            ActivePayloadError::Authority(ActivePayloadAuthorityError {
                path: path.clone(),
                error,
            })
        })
}

pub(super) fn effective_root(authority: &LumaPeerRootAuthority) -> &PathRef {
    authority.effective_addon_root_ref()
}

pub(crate) fn is_exact_dlss_relative(relative: &Path) -> bool {
    relative.components().count() == 1
        && relative
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case(DLSS_FILE_NAME))
}

fn is_reserved_generic_target(file_name: Option<&str>) -> bool {
    let Some(file_name) = file_name else {
        return true;
    };
    let lower = file_name.to_ascii_lowercase();
    Path::new(file_name)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("bak"))
        || matches!(
            lower.as_str(),
            "dxgi.dll" | "reshade.ini" | "reshade64.dll" | "reshade32.dll" | DLSS_FILE_NAME
        )
}

fn validate_endpoint_overlaps(paths: &[PathRef]) -> Result<(), ActivePayloadError> {
    let mut indexed = BTreeMap::new();
    for path in paths {
        let key = normalized_path_key(path.as_str());
        if indexed.insert(key, path).is_some() {
            return Err(ActivePayloadStructuralError::DuplicateTarget(path.clone()).into());
        }
    }
    for (index, (_, path)) in indexed.iter().enumerate() {
        for (_, other) in indexed.iter().skip(index + 1) {
            let (first, second) = match normalized_path_relation(path.as_str(), other.as_str()) {
                NormalizedPathRelation::LeftAncestor => (*path, *other),
                NormalizedPathRelation::RightAncestor => (*other, *path),
                NormalizedPathRelation::Equal | NormalizedPathRelation::Disjoint => continue,
            };
            return Err(ActivePayloadStructuralError::OverlappingTargets {
                first: first.clone(),
                second: second.clone(),
            }
            .into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use renderpilot_domain::PathRef;

    use super::validate_endpoint_overlaps;

    #[test]
    fn endpoint_overlap_recognizes_a_drive_root_as_an_ancestor() {
        let root = PathRef::new("c:/").expect("root");
        let child = PathRef::new("c:/payload/addon.dll").expect("child");

        assert!(validate_endpoint_overlaps(&[root, child]).is_err());
    }
}
