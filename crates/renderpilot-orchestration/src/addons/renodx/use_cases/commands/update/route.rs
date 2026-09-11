//! Closed phase-one route for RenoDX updates.

use renderpilot_application::ProxyTopologyRepository;

use crate::addons::renodx::errors;
use crate::addons::renodx::types::RenoDxManifest;
use crate::addons::reshade::types::ReshadeSourceCatalog;
use crate::game_mutation_lock::GameMutationGuard;
use crate::{Context, ServiceError};

use super::active::{ActiveUpdatePhase1, snapshot_active_update};
use super::snapshot::{UpdateSnapshot, resolve_update_snapshot};

/// One exact route chosen while the per-game guard is held.
#[derive(Debug)]
pub(super) enum UpdatePhase1 {
    Inactive(Box<UpdateSnapshot>),
    Active(Box<ActiveUpdatePhase1>),
}

/// Reads the update snapshot and topology exactly once under the held guard.
pub(super) fn snapshot_update_route(
    context: &Context,
    manifest: &RenoDxManifest,
    reshade_sources: &ReshadeSourceCatalog,
    guard: &GameMutationGuard,
) -> Result<UpdatePhase1, ServiceError> {
    let game_id = guard.game_id();
    let base = resolve_update_snapshot(context, manifest, reshade_sources, game_id)?;
    let topology = context.storage().get_proxy_topology(game_id)?;
    match topology {
        None => Ok(UpdatePhase1::Inactive(Box::new(base))),
        Some(topology) => Ok(UpdatePhase1::Active(Box::new(snapshot_active_update(
            context, base, topology, game_id,
        )?))),
    }
}

/// Ensures phase three selected the same route and all its sealed facts.
pub(super) fn ensure_update_route_matches(
    phase1: &UpdatePhase1,
    phase3: &UpdatePhase1,
) -> Result<(), ServiceError> {
    match (phase1, phase3) {
        (UpdatePhase1::Inactive(before), UpdatePhase1::Inactive(after)) => {
            super::snapshot::ensure_update_snapshot_matches(before, after)
        }
        (UpdatePhase1::Active(before), UpdatePhase1::Active(after)) => {
            before.ensure_phase3_matches(after)
        }
        _ => Err(errors::state_changed_retry_update()),
    }
}
