//! Three-phase active-topology Luma update orchestration.
//!
//! Phase one is selected by the parent route while the recovered guard is
//! held. Network preparation then runs without that guard; phase three
//! reacquires it and remains under the same guard through durable commit.

mod execute;
mod model;
mod phase3;
mod prepare;
pub(crate) mod snapshot;

use crate::GameSafetyPermit;
use crate::ServiceError;
use crate::addons::luma::peer::LumaActiveUpdatePrepared;
use crate::addons::reshade::types::ReshadeSourceCatalog;
use crate::game_mutation_lock::GameMutationGuard;
use crate::net::ProgressObserver;

pub(crate) use model::ActiveUpdatePhase1;
pub(crate) use phase3::{ActiveUpdatePhase3, snapshot_active_update_phase3};

fn commit_active_update(
    context: &crate::Context,
    guard: &GameMutationGuard,
    safety: &GameSafetyPermit,
    phase1: &ActiveUpdatePhase1,
    phase3: ActiveUpdatePhase3,
    prepared: LumaActiveUpdatePrepared,
    progress: Option<&ProgressObserver<'_>>,
) -> Result<(), ServiceError> {
    execute::commit_active_update(context, guard, safety, phase1, phase3, prepared, progress)
}

async fn prepare_active_update(
    phase1: &ActiveUpdatePhase1,
    reshade_sources: &ReshadeSourceCatalog,
    force_full: bool,
    progress: Option<&ProgressObserver<'_>>,
) -> Result<LumaActiveUpdatePrepared, ServiceError> {
    prepare::prepare_active_update(phase1, reshade_sources, force_full, progress).await
}

pub(super) async fn update(
    request: super::UpdateRequest<'_>,
    phase1: ActiveUpdatePhase1,
) -> Result<(), ServiceError> {
    let super::UpdateRequest {
        context,
        manifest,
        reshade_sources,
        game_id,
        force_full,
        safety,
        progress,
    } = request;

    let prepared = prepare_active_update(&phase1, reshade_sources, force_full, progress).await?;
    let guard =
        crate::mutation_boundary::enter_game_mutation_boundary_async(context, game_id).await?;
    let phase3 = snapshot_active_update_phase3(context, manifest, &guard, &phase1, &prepared)?;
    commit_active_update(
        context, &guard, &safety, &phase1, phase3, prepared, progress,
    )
}
