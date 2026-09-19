use std::path::Path;

use crate::addons::engine::FileOp;

use super::PreparedInstall;
use crate::addons::reshade::ini_schema::ini_merge_strategy;
use crate::addons::reshade::scan as reshade;
use crate::addons::reshade::types::ReshadeIniTweaks;

pub(super) fn combined_ops(
    prepared: &PreparedInstall,
    writes_host: bool,
    ini_op: Option<&FileOp>,
) -> Vec<FileOp> {
    let mut ops = vec![addon_op(prepared)];
    if writes_host {
        ops.push(host_op(prepared));
    }
    if let Some(ini_op) = ini_op {
        ops.push(ini_op.clone());
    }
    ops
}

/// The RenoDX add-on file op: a rolling upstream snapshot RenoDx already
/// PE-sanity-checked, so a pre-existing file at that path (a prior install) is
/// simply overwritten — nothing about the old bytes is worth preserving.
pub(super) fn addon_op(prepared: &PreparedInstall) -> FileOp {
    FileOp::Replace {
        name: prepared.addon_file_name.clone(),
        bytes: prepared.addon_bytes.clone(),
    }
}

/// The ReShade host DLL op: an official redistributable RenoDx fetched itself,
/// so a pre-existing file in that slot is overwritten with no on-disk backup —
/// its identity is confirmed by
/// [`assess`](crate::addons::reshade::host_policy::assess) before this ever runs.
pub(super) fn host_op(prepared: &PreparedInstall) -> FileOp {
    FileOp::Replace {
        name: prepared.proxy_dll_name.clone(),
        bytes: prepared.reshade_dll_bytes.clone(),
    }
}

pub(super) fn host_ops(
    prepared: &PreparedInstall,
    writes_host: bool,
    ini_op: Option<&FileOp>,
) -> Vec<FileOp> {
    let mut ops = Vec::new();
    if writes_host {
        ops.push(host_op(prepared));
    }
    if let Some(ini_op) = ini_op {
        ops.push(ini_op.clone());
    }
    ops
}

/// The `ReShade.ini` merge operation: additively set RenoDX's `[ADDON]` keys,
/// creating the file from empty when none exists. Uses `UpdateText` rather than
/// `MergeText` — RenoDX never keeps a `.bak` of a config file that may carry the
/// user's own hand-tuned ReShade settings. The engine itself tracks a from-empty
/// write as `created_files` (see `engine::InstallChanges::into_receipt`), so
/// `install_plans`/`build_vulkan_plan` need no extra book-keeping for it.
pub(super) fn ini_op_for_game(
    game_dir: &Path,
    prepared: &PreparedInstall,
) -> Result<Option<FileOp>, crate::ServiceError> {
    let tweaks = effective_ini_tweaks(game_dir, &prepared.ini_tweaks);
    let strategy = ini_merge_strategy(&tweaks);
    if let Some(desired) = prepared.processing_path.desired_set_path() {
        let path = reshade::reshade_ini_path(game_dir)
            .unwrap_or_else(|| game_dir.join(reshade::RESHADE_INI_FILE_NAME));
        let expected_before = read_ini_preimage(&path)?;
        let path_str = path
            .to_str()
            .ok_or_else(|| crate::addons::errors::invalid("invalid ReShade.ini path"))?;
        let path_ref = renderpilot_domain::PathRef::new(path_str)
            .map_err(|error| crate::addons::errors::invalid(error.to_string()))?;
        let planned = crate::addons::renodx::reshade_ini::plan_set_path(
            path_ref,
            expected_before.as_deref().unwrap_or_default(),
            desired,
        )
        .map_err(|error| {
            crate::addons::errors::invalid(format!("cannot plan RenoDX Set_Path: {error}"))
        })?;
        let planned_text = std::str::from_utf8(&planned.after).map_err(|_| {
            crate::addons::errors::invalid("RenoDX Set_Path planner returned non-UTF-8")
        })?;
        let after = if strategy.has_writes() {
            strategy.apply(planned_text).into_bytes()
        } else {
            planned.after
        };
        return Ok(Some(FileOp::RenoDxSetPath {
            name: reshade::RESHADE_INI_FILE_NAME.to_owned(),
            expected_before,
            after,
            receipt: planned.receipt,
        }));
    }
    Ok(ini_tweaks_write_keys(&tweaks).then(|| FileOp::UpdateText {
        name: reshade::RESHADE_INI_FILE_NAME.to_owned(),
        default: String::new(),
        strategy,
    }))
}

fn read_ini_preimage(path: &Path) -> Result<Option<Vec<u8>>, crate::ServiceError> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(crate::addons::errors::io(
                "read ReShade.ini metadata",
                path,
                &error,
            ));
        }
    };
    if !metadata.file_type().is_file() {
        return Err(crate::addons::errors::invalid(format!(
            "cannot manage RenoDX ReShade.ini `{}`: not a regular file",
            path.display()
        )));
    }
    std::fs::read(path)
        .map(Some)
        .map_err(|error| crate::addons::errors::io("read ReShade.ini", path, &error))
}

pub(super) fn effective_ini_tweaks(game_dir: &Path, tweaks: &ReshadeIniTweaks) -> ReshadeIniTweaks {
    let mut effective = tweaks.clone();
    // `DisabledAddons` is a RenoDX default for an empty setup, never a reason
    // to alter a user runtime whose effects/add-ons cannot be ruled out.
    if !reshade::assess_reshade_content(game_dir, &[]).is_empty() {
        effective.disabled_addons.clear();
    }
    effective
}

pub(super) fn ini_tweaks_write_keys(tweaks: &ReshadeIniTweaks) -> bool {
    !tweaks.disabled_addons.is_empty() || tweaks.addon_path.is_some() || tweaks.dlss_fix.is_some()
}
