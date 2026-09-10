//! Closed phase-one route for Luma updates.

use renderpilot_application::ProxyTopologyRepository;
use renderpilot_domain::{AddonKind, InstalledAddon};

use crate::addons::engine;
use crate::addons::luma::errors;
use crate::addons::luma::game_context::require_game;
use crate::addons::luma::types::LumaManifest;
use crate::addons::luma::use_cases::update_target::resolve_update_target;
use crate::addons::records;
use crate::game_mutation_lock::GameMutationGuard;
use crate::{Context, ServiceError};

use super::active::ActiveUpdatePhase1;
use super::active::snapshot::snapshot_active_update;

/// Closed update route selected from one locked phase-one read.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum UpdatePhase1 {
    Inactive(Box<InactiveUpdatePhase1>),
    Active(Box<ActiveUpdatePhase1>),
}

/// Exact inactive phase-one state. The inactive route has no topology; its
/// sentinel is anchored at the freshly resolved target game directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InactiveUpdatePhase1 {
    pub(crate) record: InstalledAddon,
    pub(crate) had_torn_marker: bool,
}

/// Snapshots one update route while the recovered per-game guard is held.
pub(crate) fn snapshot_update_route(
    context: &Context,
    manifest: &LumaManifest,
    guard: &GameMutationGuard,
) -> Result<UpdatePhase1, ServiceError> {
    let game_id = guard.game_id();
    let game = require_game(context, game_id)?;
    let record = records::record_of_kind(context, game_id, AddonKind::Luma)?
        .ok_or_else(errors::not_installed)?;
    let target = resolve_update_target(context, manifest, game_id)?.ok_or_else(|| {
        errors::invalid(
            "Luma no longer has a live installable profile for this game; update is unavailable until the catalogue match is restored"
                .to_owned(),
        )
    })?;
    let topology = context.storage().get_proxy_topology(game_id)?;

    match topology {
        None => Ok(UpdatePhase1::Inactive(Box::new(InactiveUpdatePhase1 {
            record,
            had_torn_marker: engine::is_install_torn(&target.game_dir, AddonKind::Luma),
        }))),
        Some(topology) => Ok(UpdatePhase1::Active(Box::new(snapshot_active_update(
            manifest,
            game_id,
            record,
            target,
            topology,
            game.install_path().clone(),
        )?))),
    }
}

#[cfg(test)]
mod tests;
