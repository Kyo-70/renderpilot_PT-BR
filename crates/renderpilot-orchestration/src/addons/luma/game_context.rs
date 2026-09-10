//! Luma game-context: the shared game loaders plus the manifest-typed
//! analyze-and-resolve step.

use std::path::Path;

use renderpilot_application::GameRepository;
use renderpilot_domain::{GameId, GameInstallation};

use crate::addons::game_analysis::{GameAnalysis, analyze_game};
use crate::{Context, ServiceError};

use super::matcher::{LumaResolution, resolve};
use super::types::LumaManifest;

pub(super) use crate::addons::game_context::{executable_override, require_game};

/// Inspects the game on disk and resolves it against the manifest in one step.
pub(super) fn analyze_and_resolve(
    game: &GameInstallation,
    manifest: &LumaManifest,
    override_path: Option<&Path>,
) -> (GameAnalysis, LumaResolution) {
    let analysis = analyze_game(game, override_path);
    let resolution = resolve(manifest, &analysis.facts);
    (analysis, resolution)
}

/// Pure launch-args derivation from an already-resolved manifest title.
#[must_use]
pub(super) fn effective_launch_args(resolution: &LumaResolution) -> Vec<String> {
    let LumaResolution::Installable(plan) = resolution else {
        return Vec::new();
    };

    plan.launch_args.clone()
}

/// The launch arguments a matched title requires, re-resolved from the manifest
/// at query time rather than read from the install record (which never
/// persists them — see [`renderpilot_domain::LumaInstallState::Installed::launch_args`]).
/// Empty when the game can no longer be resolved (e.g. removed from the
/// library) or no longer matches an installable title.
pub(super) fn resolve_launch_args(
    context: &Context,
    manifest: &LumaManifest,
    game_id: &GameId,
) -> Result<Vec<String>, ServiceError> {
    let Some(game) = context.storage().find_game(game_id)? else {
        return Ok(Vec::new());
    };
    let override_path = executable_override(context, game_id);
    let (_analysis, resolution) = analyze_and_resolve(&game, manifest, override_path.as_deref());
    Ok(effective_launch_args(&resolution))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn effective_launch_args_are_declared_by_the_resolved_title() {
        let plan = crate::addons::luma::matcher::ResolvedLumaInstall {
            asset: "Luma-Title.zip".to_owned(),
            addon_file: "Luma-Title.addon".to_owned(),
            arch: renderpilot_domain::Architecture::X64,
            proxy_dll_name: "dxgi.dll".to_owned(),
            confidence: crate::addons::matching::MatchConfidence::Verified,
            launch_args: vec!["-dx11".to_owned()],
            features: None,
            guidance: Vec::new(),
            external_requirement: None,
            profile: super::super::types::LumaProfile::Game,
        };
        assert_eq!(
            effective_launch_args(&LumaResolution::Installable(Box::new(plan))),
            vec!["-dx11"]
        );
    }
}
