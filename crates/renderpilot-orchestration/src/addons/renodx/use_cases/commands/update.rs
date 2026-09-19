//! Applies updates to installed RenoDX add-ons and host artifacts.

use renderpilot_domain::GameId;

use super::engine_config;
use crate::addons::renodx::types::RenoDxManifest;
use crate::addons::reshade::types::ReshadeSourceCatalog;
use crate::net::ProgressObserver;
use crate::{Context, ServiceError};

mod active;
mod commit;
mod inactive;
mod prepare;
mod route;
mod snapshot;

#[cfg(test)]
mod tests;

/// Complete request for a generic RenoDX update.
pub struct UpdateRequest<'a> {
    /// Application services and storage.
    pub context: &'a Context,
    /// RenoDX manifest used to resolve the update.
    pub manifest: &'a RenoDxManifest,
    /// ReShade sources used when the host must be updated.
    pub reshade_sources: &'a ReshadeSourceCatalog,
    /// Game whose RenoDX installation is being updated.
    pub game_id: &'a GameId,
    /// Fresh permits for every mutation scope the resolved update may require.
    pub safety: crate::GameMutationSafetyPermits,
    /// Optional download progress observer.
    pub progress: Option<&'a ProgressObserver<'a>>,
}

/// Applies an update to the main RenoDX add-on and host artifacts only.
/// DLSS-Fix has an independent update/repair command and its source/path
/// projection is copied byte-for-byte through this generic transaction.
///
/// Network prepare for per-game artifacts runs **outside** the per-game lock
/// (same 3-phase contract as Luma update). Shared Vulkan layer updates still
/// apply under the lock in phase 3 (system-wide mutation).
pub async fn update(request: UpdateRequest<'_>) -> Result<(), ServiceError> {
    let context = request.context;
    let manifest = request.manifest;
    let reshade_sources = request.reshade_sources;
    let game_id = request.game_id;
    let safety = request.safety.game().clone();
    let phase1 = {
        let guard =
            crate::mutation_boundary::enter_game_mutation_boundary_async(context, game_id).await?;
        route::snapshot_update_route(context, manifest, reshade_sources, &guard)?
    };
    match phase1 {
        route::UpdatePhase1::Inactive(snapshot) => inactive::update(request, *snapshot).await,
        route::UpdatePhase1::Active(snapshot) => {
            active::update(request, *snapshot).await.map(|_| ())
        }
    }?;
    engine_config::reconcile_after_commit(context, manifest, game_id, safety).await?;
    Ok(())
}
