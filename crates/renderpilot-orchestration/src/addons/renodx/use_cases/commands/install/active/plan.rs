use std::path::PathBuf;

use renderpilot_domain::{Architecture, RenoDxReshadeIniFeature};

use crate::ServiceError;
use crate::addons::game_analysis::{GameAnalysis, analyze_game, install_target_dir};
use crate::addons::renodx::errors;
use crate::addons::renodx::game_context::{analyze_and_resolve, executable_override, require_game};
use crate::addons::renodx::matcher::{
    ResolvedInstall, generic_file_install_plan, resolve_external_install,
};
use crate::addons::renodx::peer::InstallCommandVariant;

use super::model::{ActiveInstallSource, ResolveActiveInstallRequest};
use super::validation;

/// Resolved game analysis, RenoDX plan, and the typed configuration feature.
pub(super) struct ActiveInstallPlan {
    pub(super) analysis: GameAnalysis,
    pub(super) plan: ResolvedInstall,
    pub(super) feature: RenoDxReshadeIniFeature,
    pub(super) variant: InstallCommandVariant,
}

/// Resolves the same catalogue or local-file plan for both active phases.
pub(super) fn resolve(
    request: &ResolveActiveInstallRequest<'_>,
) -> Result<ActiveInstallPlan, ServiceError> {
    let game = require_game(request.context, request.game_id)?;
    let override_path = executable_override(request.context, request.game_id);
    let resolved = match request.source {
        ActiveInstallSource::Catalog => {
            let (analysis, resolution) =
                analyze_and_resolve(&game, request.manifest, override_path.as_deref());
            let plan = validation::catalog_plan(resolution)?;
            ActiveInstallPlan {
                analysis,
                plan,
                feature: request.source.reshade_ini_feature(),
                variant: request.source.command_variant(),
            }
        }
        ActiveInstallSource::InstallFromFile { architecture } => {
            let analysis = analyze_game(&game, override_path.as_deref());
            ensure_game_architecture(&analysis, architecture)?;
            let plan = resolve_external_install(request.manifest, &analysis.facts)
                .or_else(|| generic_file_install_plan(&analysis.facts, architecture))
                .ok_or_else(|| {
                    errors::invalid(
                        "RenoDX cannot be installed for this game: its renderer is not Direct3D"
                            .to_owned(),
                    )
                })?;
            validation::ensure_file_architecture(architecture, plan.arch)?;
            ActiveInstallPlan {
                analysis,
                plan,
                feature: request.source.reshade_ini_feature(),
                variant: request.source.command_variant(),
            }
        }
    };
    Ok(resolved)
}

fn ensure_game_architecture(
    analysis: &GameAnalysis,
    file_architecture: Architecture,
) -> Result<(), ServiceError> {
    if let Some(game_architecture) = analysis.facts.graphics.architecture()
        && game_architecture != file_architecture
    {
        return Err(errors::invalid(format!(
            "this add-on is {} but the game is {} — download the matching add-on",
            architecture_label(file_architecture),
            architecture_label(game_architecture),
        )));
    }
    Ok(())
}

pub(super) fn target_dir(analysis: &GameAnalysis) -> Result<PathBuf, ServiceError> {
    install_target_dir(analysis)
}

pub(super) fn architecture_label(architecture: Architecture) -> &'static str {
    match architecture {
        Architecture::X64 => "64-bit",
        Architecture::X86 => "32-bit",
    }
}
