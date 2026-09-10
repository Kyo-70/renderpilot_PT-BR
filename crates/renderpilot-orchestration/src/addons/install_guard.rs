//! Shared install preflight: resolve scan roots and enforce exclusivity + torn recovery.
//!
//! Callers hold the per-game [`crate::game_mutation_lock`] before calling these helpers.

use renderpilot_domain::{AddonKind, GameId};

use crate::addons::engine;
use crate::addons::exclusivity;
use crate::addons::game_analysis::{GameAnalysis, install_target_dir};
use crate::addons::reshade::InstallRoots;
use crate::{Context, ServiceError};

/// Resolves install scan roots from analysis (exe parent + optional split AddonPath).
pub(crate) fn resolve_install_scan_roots(
    analysis: &GameAnalysis,
) -> Result<InstallRoots, ServiceError> {
    let dir = install_target_dir(analysis)?;
    Ok(InstallRoots::resolve_from_ini(&dir))
}

/// Ensures the requesting tool is not blocked by a peer, then recovers a torn
/// framework install when its sentinel is present. OptiScaler owns a separate
/// journal and never enters this framework-only guard.
///
/// The user-facing block message comes from
/// [`crate::addons::tool::AddonTool::exclusive_block_message`].
pub(crate) fn guard_exclusivity_and_torn(
    context: &Context,
    game_id: &GameId,
    kind: AddonKind,
    roots: &InstallRoots,
) -> Result<(), ServiceError> {
    let scan_dirs = roots.scan_dir_paths();
    let feature = match kind {
        AddonKind::Luma => renderpilot_domain::mutation_features::LUMA_INSTALL,
        AddonKind::RenoDx => renderpilot_domain::mutation_features::RENODX_INSTALL,
        AddonKind::OptiScaler => {
            return Err(ServiceError::invalid_input(
                "shared peer install guard does not own OptiScaler",
            ));
        }
    };
    crate::file_mutation::ensure_feature_allowed_with_proxy_topology(context, game_id, feature)?;
    exclusivity::ensure_not_blocked(context, game_id, kind, Some(scan_dirs.as_slice()))?;
    if engine::is_install_torn(roots.sentinel_dir(), kind) {
        match kind {
            AddonKind::Luma => {
                crate::addons::luma::install::recover_torn_install(scan_dirs.as_slice());
            }
            AddonKind::RenoDx => {
                crate::addons::renodx::install::recover_torn_install(scan_dirs.as_slice());
            }
            AddonKind::OptiScaler => unreachable!("OptiScaler is excluded above"),
        }
    }
    Ok(())
}
