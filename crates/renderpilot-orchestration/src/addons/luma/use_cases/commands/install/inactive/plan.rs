use std::path::{Path, PathBuf};

use renderpilot_domain::{AddonKind, Architecture, GameId};

use crate::addons::exclusivity;
use crate::addons::game_analysis::install_target_dir;
use crate::addons::install_guard;
use crate::addons::luma::dgvoodoo;
use crate::addons::luma::errors;
use crate::addons::luma::game_context::{analyze_and_resolve, executable_override, require_game};
use crate::addons::luma::matcher::{LumaResolution, ResolvedLumaInstall};
use crate::addons::luma::types::LumaManifest;
use crate::addons::records;
use crate::addons::reshade::host_policy;
use crate::paths::same_path;
use crate::{Context, ServiceError};

#[derive(Debug, Clone)]
pub(super) struct InstallSnapshot {
    pub(super) target_dir: PathBuf,
    asset: String,
    addon_file: String,
    arch: Architecture,
    proxy_dll_name: String,
    pub(super) writes_host: bool,
    pub(super) dgvoodoo_kind: DgVoodooPrepKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DgVoodooPrepKind {
    None,
    Managed,
    Reused,
    Adopted,
}

impl DgVoodooPrepKind {
    fn from_preparation(prep: Option<&dgvoodoo::DgVoodooPreparation<'_>>) -> Self {
        match prep {
            None => Self::None,
            Some(dgvoodoo::DgVoodooPreparation::Managed(_)) => Self::Managed,
            Some(dgvoodoo::DgVoodooPreparation::Reused(_)) => Self::Reused,
            Some(dgvoodoo::DgVoodooPreparation::Adopted(_)) => Self::Adopted,
        }
    }
}

pub(super) struct ResolvedInstallSnapshot {
    pub(super) snapshot: InstallSnapshot,
    pub(super) plan: ResolvedLumaInstall,
    pub(super) dgvoodoo_kind: DgVoodooPrepKind,
}

pub(super) fn resolve(
    context: &Context,
    manifest: &LumaManifest,
    game_id: &GameId,
) -> Result<ResolvedInstallSnapshot, ServiceError> {
    let game = require_game(context, game_id)?;
    let override_path = executable_override(context, game_id);
    let (analysis, resolution) = analyze_and_resolve(&game, manifest, override_path.as_deref());
    let target_dir = install_target_dir(&analysis)?;
    let roots = install_guard::resolve_install_scan_roots(&analysis)?;
    ensure_not_already_installed(context, game_id)?;
    install_guard::guard_exclusivity_and_torn(context, game_id, AddonKind::Luma, &roots)?;
    exclusivity::ensure_not_unmanaged(
        roots.scan_dir_paths().as_slice(),
        AddonKind::Luma,
        "an existing Luma install was found on disk with no tracked record; remove it manually before installing",
    )?;

    let plan = match resolution {
        LumaResolution::Installable(plan) => *plan,
        LumaResolution::Incompatible { reason } => {
            return Err(errors::invalid(format!(
                "Luma is not compatible with this game: {reason:?}"
            )));
        }
        LumaResolution::Blacklisted { message } => {
            return Err(errors::invalid(format!(
                "Luma is not supported for this game: {}",
                message.fallback_text
            )));
        }
        LumaResolution::NoMatch => {
            return Err(errors::invalid(
                "Luma has no profile for this game".to_owned(),
            ));
        }
    };
    let minimum_host_version = manifest.min_reshade_version_parsed()?;
    let host = host_policy::assess_for_tool(
        &target_dir,
        &plan.proxy_dll_name,
        "Luma",
        Some(&minimum_host_version),
    );
    host.ensure_initial_installable(&plan.proxy_dll_name)?;
    let writes_host = host.initial_writes_host();
    let dgvoodoo = assess_dgvoodoo(&plan, &target_dir, host.lifecycle)?;
    let dgvoodoo_kind = DgVoodooPrepKind::from_preparation(dgvoodoo.as_ref());
    Ok(ResolvedInstallSnapshot {
        snapshot: InstallSnapshot {
            target_dir,
            asset: plan.asset.clone(),
            addon_file: plan.addon_file.clone(),
            arch: plan.arch,
            proxy_dll_name: plan.proxy_dll_name.clone(),
            writes_host,
            dgvoodoo_kind,
        },
        plan,
        dgvoodoo_kind,
    })
}

fn assess_dgvoodoo<'a>(
    plan: &'a ResolvedLumaInstall,
    target_dir: &Path,
    host_lifecycle: host_policy::HostLifecycle,
) -> Result<Option<dgvoodoo::DgVoodooPreparation<'a>>, ServiceError> {
    let Some(requirement) = dgvoodoo::requirement(plan.external_requirement.as_ref()) else {
        return Ok(None);
    };
    match dgvoodoo::assess_existing(target_dir, requirement) {
        dgvoodoo::ExistingDgVoodoo::Absent => {
            Ok(Some(dgvoodoo::DgVoodooPreparation::Managed(requirement)))
        }
        dgvoodoo::ExistingDgVoodoo::CompatibleAdoptable
            if host_lifecycle == host_policy::HostLifecycle::AdoptEmpty =>
        {
            Ok(Some(dgvoodoo::DgVoodooPreparation::Adopted(
                dgvoodoo::adopted_existing(requirement, target_dir),
            )))
        }
        dgvoodoo::ExistingDgVoodoo::CompatibleReusable
        | dgvoodoo::ExistingDgVoodoo::CompatibleAdoptable => Ok(Some(
            dgvoodoo::DgVoodooPreparation::Reused(dgvoodoo::reused_config(requirement)),
        )),
        dgvoodoo::ExistingDgVoodoo::Conflict(reason) => Err(errors::invalid(format!(
            "the existing dgVoodoo runtime is incompatible with Luma: {reason}"
        ))),
    }
}

pub(super) fn preparation_for_plan<'a>(
    plan: &'a ResolvedLumaInstall,
    target_dir: &Path,
    kind: DgVoodooPrepKind,
) -> Result<Option<dgvoodoo::DgVoodooPreparation<'a>>, ServiceError> {
    let Some(requirement) = dgvoodoo::requirement(plan.external_requirement.as_ref()) else {
        return Ok(None);
    };
    Ok(Some(match kind {
        DgVoodooPrepKind::None => return Ok(None),
        DgVoodooPrepKind::Managed => dgvoodoo::DgVoodooPreparation::Managed(requirement),
        DgVoodooPrepKind::Reused => {
            dgvoodoo::DgVoodooPreparation::Reused(dgvoodoo::reused_config(requirement))
        }
        DgVoodooPrepKind::Adopted => dgvoodoo::DgVoodooPreparation::Adopted(
            dgvoodoo::adopted_existing(requirement, target_dir),
        ),
    }))
}

pub(super) fn refresh_adopted(
    prepared: &mut crate::addons::luma::install::PreparedInstall,
    target_dir: &Path,
    plan: &ResolvedLumaInstall,
) -> Result<(), ServiceError> {
    use crate::addons::luma::dgvoodoo::DgVoodooInstall;

    let Some(DgVoodooInstall::Adopted(_)) = prepared.dgvoodoo.as_ref() else {
        return Ok(());
    };
    let Some(requirement) = dgvoodoo::requirement(plan.external_requirement.as_ref()) else {
        return Err(errors::state_changed_retry_install());
    };
    match dgvoodoo::assess_existing(target_dir, requirement) {
        dgvoodoo::ExistingDgVoodoo::CompatibleAdoptable => {
            prepared.dgvoodoo = Some(DgVoodooInstall::Adopted(dgvoodoo::adopted_existing(
                requirement,
                target_dir,
            )));
            Ok(())
        }
        _ => Err(errors::state_changed_retry_install()),
    }
}

pub(super) fn ensure_matches(
    snapshot: &InstallSnapshot,
    current: &InstallSnapshot,
) -> Result<(), ServiceError> {
    if !same_path(&snapshot.target_dir, &current.target_dir)
        || snapshot.asset != current.asset
        || snapshot.addon_file != current.addon_file
        || snapshot.arch != current.arch
        || snapshot.proxy_dll_name != current.proxy_dll_name
        || snapshot.writes_host != current.writes_host
        || snapshot.dgvoodoo_kind != current.dgvoodoo_kind
    {
        return Err(errors::state_changed_retry_install());
    }
    Ok(())
}

fn ensure_not_already_installed(context: &Context, game_id: &GameId) -> Result<(), ServiceError> {
    records::ensure_no_record(
        context,
        game_id,
        AddonKind::Luma,
        "Luma is already installed for this game; uninstall before reinstalling",
    )
}
