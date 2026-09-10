use std::{
    collections::HashSet,
    path::{Component, Path},
};

use renderpilot_domain::{PathRef, TrackedSource, managed_sidecar_path, normalized_path_key};

use super::super::root_authority::LumaPeerRootAuthority;
use super::{
    ActiveDgVoodooError, ActiveDgVoodooTargetKind,
    model::{ActiveDgVoodooPlan, ActiveDgVoodooTarget, ActiveDgVoodooTargetPayload},
};
use crate::addons::{
    engine::ensure_safe_relative_path,
    luma::dgvoodoo::{DgVoodooInstall, PreparedDgVoodoo},
};

pub(crate) fn plan_active_dgvoodoo(
    authority: &LumaPeerRootAuthority,
    install: Option<DgVoodooInstall>,
) -> Result<Option<ActiveDgVoodooPlan>, ActiveDgVoodooError> {
    let Some(install) = install else {
        return Ok(None);
    };

    let (mut targets, tracked_source) = match install {
        DgVoodooInstall::Managed(prepared) => managed_targets(authority, prepared)?,
        DgVoodooInstall::Reused(config) => (
            vec![config_target(
                authority,
                config.config_file,
                config.config_default,
                config.config_sections,
            )?],
            None,
        ),
        DgVoodooInstall::Adopted(_) => {
            return Err(ActiveDgVoodooError::AdoptedOwnershipUnsupported);
        }
    };

    validate_endpoint_set(&targets)?;
    targets.sort_by_key(|target| normalized_path_key(target.live.as_str()));
    let mut observation_paths = targets
        .iter()
        .flat_map(|target| [&target.live, &target.sidecar])
        .cloned()
        .collect::<Vec<_>>();
    observation_paths.sort_by_key(|path| normalized_path_key(path.as_str()));

    Ok(Some(ActiveDgVoodooPlan {
        targets,
        observation_paths,
        tracked_source,
    }))
}

fn managed_targets(
    authority: &LumaPeerRootAuthority,
    prepared: PreparedDgVoodoo,
) -> Result<(Vec<ActiveDgVoodooTarget>, Option<TrackedSource>), ActiveDgVoodooError> {
    let tracked_source = Some(prepared.tracked_source());
    let PreparedDgVoodoo {
        files,
        config_file,
        config_default,
        config_sections,
        ..
    } = prepared;
    let mut targets = files
        .into_iter()
        .map(|file| runtime_target(authority, file.dest, file.bytes))
        .collect::<Result<Vec<_>, _>>()?;
    targets.push(config_target(
        authority,
        config_file,
        config_default,
        config_sections,
    )?);
    Ok((targets, tracked_source))
}

fn runtime_target(
    authority: &LumaPeerRootAuthority,
    name: String,
    bytes: Vec<u8>,
) -> Result<ActiveDgVoodooTarget, ActiveDgVoodooError> {
    target(
        authority,
        ActiveDgVoodooTargetKind::Runtime,
        ActiveDgVoodooTargetPayload::Runtime { bytes },
        name,
    )
}

fn config_target(
    authority: &LumaPeerRootAuthority,
    name: String,
    default: String,
    sections: Vec<crate::addons::engine::IniSection>,
) -> Result<ActiveDgVoodooTarget, ActiveDgVoodooError> {
    target(
        authority,
        ActiveDgVoodooTargetKind::Config,
        ActiveDgVoodooTargetPayload::Config {
            default: default.into_bytes(),
            sections,
        },
        name,
    )
}

fn target(
    authority: &LumaPeerRootAuthority,
    error_kind: ActiveDgVoodooTargetKind,
    kind: ActiveDgVoodooTargetPayload,
    name: String,
) -> Result<ActiveDgVoodooTarget, ActiveDgVoodooError> {
    let relative = ensure_direct_file_name(error_kind, &name)?;
    let root = Path::new(authority.canonical_game_root_ref().as_str());
    let live_path =
        PathRef::new(root.join(relative).to_string_lossy().into_owned()).map_err(|_| {
            ActiveDgVoodooError::InvalidTargetName {
                kind: error_kind,
                name,
            }
        })?;
    authority
        .authorized_root(&live_path)
        .map_err(|_| ActiveDgVoodooError::UnauthorizedTarget {
            kind: error_kind,
            path: live_path.clone(),
        })?;
    let sidecar =
        managed_sidecar_path(&live_path).map_err(|error| ActiveDgVoodooError::SidecarPath {
            live: live_path.clone(),
            error,
        })?;
    authority
        .authorized_root(&sidecar)
        .map_err(|_| ActiveDgVoodooError::UnauthorizedTarget {
            kind: ActiveDgVoodooTargetKind::Sidecar,
            path: sidecar.clone(),
        })?;
    Ok(ActiveDgVoodooTarget {
        live: live_path,
        sidecar,
        kind,
    })
}

fn ensure_direct_file_name(
    error_kind: ActiveDgVoodooTargetKind,
    raw: &str,
) -> Result<std::path::PathBuf, ActiveDgVoodooError> {
    let field = match error_kind {
        ActiveDgVoodooTargetKind::Runtime => "runtime",
        ActiveDgVoodooTargetKind::Config => "config",
        ActiveDgVoodooTargetKind::Sidecar => "sidecar",
    };
    let relative = ensure_safe_relative_path(field, raw).map_err(|_| {
        ActiveDgVoodooError::InvalidTargetName {
            kind: error_kind,
            name: raw.to_owned(),
        }
    })?;
    let mut components = relative.components();
    if !matches!(components.next(), Some(Component::Normal(_))) || components.next().is_some() {
        return Err(ActiveDgVoodooError::InvalidTargetName {
            kind: error_kind,
            name: raw.to_owned(),
        });
    }
    Ok(relative)
}

fn validate_endpoint_set(targets: &[ActiveDgVoodooTarget]) -> Result<(), ActiveDgVoodooError> {
    let mut seen = HashSet::new();
    let mut prior: Vec<&PathRef> = Vec::new();
    for target in targets {
        for path in [&target.live, &target.sidecar] {
            let key = normalized_path_key(path.as_str());
            if !seen.insert(key) {
                let first = prior
                    .iter()
                    .find(|candidate| {
                        normalized_path_key(candidate.as_str())
                            == normalized_path_key(path.as_str())
                    })
                    .copied()
                    .unwrap_or(path)
                    .clone();
                return Err(ActiveDgVoodooError::DuplicateTarget {
                    first,
                    duplicate: path.clone(),
                });
            }
            prior.push(path);
        }
    }
    Ok(())
}
