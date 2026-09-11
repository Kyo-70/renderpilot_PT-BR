//! RenoDX DLSS-Fix lifecycle commands.
//!
//! Route selection is intentionally explicit: active-topology games use the
//! typed durable peer transaction, while games without persisted topology keep
//! the behavior-preserving inactive implementation.

mod active;
mod inactive;

use renderpilot_application::ProxyTopologyRepository;
use renderpilot_domain::{GameId, RenoDxInstallState};

use crate::net::ProgressObserver;
use crate::{Context, ServiceError};

/// Explicitly installs/claims DLSS-Fix for a game.
pub async fn install_dlss_fix(
    context: &Context,
    game_id: &GameId,
    safety: crate::GameSafetyPermit,
    progress: Option<&ProgressObserver<'_>>,
) -> Result<RenoDxInstallState, ServiceError> {
    if active_topology_async(context, game_id).await? {
        active::install(context, game_id, safety, progress).await
    } else {
        inactive::install_dlss_fix(context, game_id, safety, progress).await
    }
}

/// Updates or repairs DLSS-Fix for a game.
pub async fn update_dlss_fix(
    context: &Context,
    game_id: &GameId,
    safety: crate::GameSafetyPermit,
    progress: Option<&ProgressObserver<'_>>,
) -> Result<RenoDxInstallState, ServiceError> {
    if active_topology_async(context, game_id).await? {
        active::update(context, game_id, safety, progress).await
    } else {
        inactive::update_dlss_fix(context, game_id, safety, progress).await
    }
}

/// Retries pending DLSS-Fix recovery for a game.
pub fn retry_dlss_fix_recovery(
    context: &Context,
    game_id: &GameId,
) -> Result<RenoDxInstallState, ServiceError> {
    // Recovery is feature-scoped and does not need an active route decision;
    // the storage classifier already fences claim-only DLSS rows safely.
    inactive::retry_dlss_fix_recovery(context, game_id)
}

/// Removes the DLSS-Fix companion and its typed active INI projection.
pub fn uninstall_dlss_fix(
    context: &Context,
    game_id: &GameId,
) -> Result<RenoDxInstallState, ServiceError> {
    if active_topology(context, game_id)? {
        active::uninstall(context, game_id)
    } else {
        inactive::uninstall_dlss_fix(context, game_id)
    }
}

async fn active_topology_async(context: &Context, game_id: &GameId) -> Result<bool, ServiceError> {
    let _guard =
        crate::mutation_boundary::enter_game_mutation_boundary_async(context, game_id).await?;
    active_topology_locked(context, game_id)
}

fn active_topology(context: &Context, game_id: &GameId) -> Result<bool, ServiceError> {
    let _guard = crate::mutation_boundary::enter_game_mutation_boundary(context, game_id)?;
    active_topology_locked(context, game_id)
}

fn active_topology_locked(context: &Context, game_id: &GameId) -> Result<bool, ServiceError> {
    Ok(context.storage().get_proxy_topology(game_id)?.is_some())
}
