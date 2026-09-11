//! Active-topology RenoDX update phase and pure prepared-artifact lowering.

mod lowering;
mod ordinary;
mod shared;
mod snapshot;
mod validation;

#[cfg(test)]
mod tests;

pub(super) use lowering::{ActiveLoweredUpdate, lower_active_update};
pub(super) use snapshot::{ActiveUpdatePhase1, snapshot_active_update};

use crate::ServiceError;
use crate::addons::renodx::use_cases::commands::update::prepare::prepare_update_artifacts;
use crate::addons::renodx::use_cases::commands::update::route::{
    UpdatePhase1, snapshot_update_route,
};
use crate::addons::renodx::use_cases::commands::update_reshade::PreparedReShadeUpdate;

pub(super) async fn update(
    request: super::UpdateRequest<'_>,
    phase1: ActiveUpdatePhase1,
) -> Result<renderpilot_domain::InstalledAddon, ServiceError> {
    let super::UpdateRequest {
        context,
        manifest,
        reshade_sources,
        game_id,
        safety,
        progress,
    } = request;
    let prepared = prepare_update_artifacts(phase1.base(), progress).await?;
    let shared_update = match phase1.base().shared_vulkan_channel() {
        Some(channel) => {
            Some(PreparedReShadeUpdate::prepare(reshade_sources, channel, progress).await?)
        }
        None => None,
    };
    let guards = crate::mutation_boundary::enter_mutation_boundary_async(
        context,
        game_id,
        shared_update.is_some(),
    )
    .await?;
    let game_guard = game_guard(&guards);
    let phase3_route = snapshot_update_route(context, manifest, reshade_sources, game_guard)?;
    let UpdatePhase1::Active(phase3) = phase3_route else {
        return Err(crate::addons::renodx::errors::state_changed_retry_update());
    };
    phase1.ensure_phase3_matches(&phase3)?;
    let lowered = lower_active_update(&phase1, &phase3, &prepared)?;
    let shared = match (&guards, shared_update.as_ref()) {
        (crate::mutation_boundary::GameMutationBoundary::GameShared(_), Some(update)) => {
            Some(update.plan_locked(context)?)
        }
        (crate::mutation_boundary::GameMutationBoundary::Game(_), None) => None,
        _ => {
            return Err(crate::ServiceError::command_failed(
                "active RenoDX shared preparation lost its combined mutation boundary",
            ));
        }
    };
    let combined = matches!(shared.as_ref(), Some(update) if !update.plan.is_noop());
    if !combined && lowered.is_noop() {
        return Ok(phase3.record().clone());
    }
    crate::addons::progress::emit_tool_finalizing(progress, renderpilot_domain::AddonKind::RenoDx);
    if combined {
        let Some(shared) = shared else {
            return Err(crate::ServiceError::command_failed(
                "active RenoDX shared update has no locked plan",
            ));
        };
        shared::commit(shared::ActiveSharedUpdateCommit {
            context,
            game_id,
            safety: &safety,
            guards,
            phase1,
            phase3: *phase3,
            lowered,
            shared,
        })
    } else {
        let guard = match guards {
            crate::mutation_boundary::GameMutationBoundary::Game(guard) => guard,
            crate::mutation_boundary::GameMutationBoundary::GameShared(guards) => {
                guards.into_game()
            }
        };
        ordinary::commit(ordinary::ActiveOrdinaryUpdateCommit {
            context,
            game_id,
            safety: &safety,
            guard,
            phase1,
            phase3: *phase3,
            lowered,
        })
    }
}

fn game_guard(
    boundary: &crate::mutation_boundary::GameMutationBoundary,
) -> &crate::game_mutation_lock::GameMutationGuard {
    match boundary {
        crate::mutation_boundary::GameMutationBoundary::Game(guard) => guard,
        crate::mutation_boundary::GameMutationBoundary::GameShared(guards) => guards.game(),
    }
}

impl ActiveUpdatePhase1 {
    /// Compares every locked fact used by preparation and composition.
    pub(super) fn phase3_matches(&self, other: &Self) -> bool {
        self.base == other.base
            && self.topology == other.topology
            && self.root_seal == other.root_seal
            && self.addon_path == other.addon_path
            && self.addon_snapshot == other.addon_snapshot
            && self.host_path == other.host_path
            && self.host_snapshot == other.host_snapshot
    }

    /// Maps route drift to the existing retry contract.
    pub(super) fn ensure_phase3_matches(&self, other: &Self) -> Result<(), ServiceError> {
        if self.phase3_matches(other) {
            Ok(())
        } else {
            Err(crate::addons::renodx::errors::state_changed_retry_update())
        }
    }

    pub(super) fn base(&self) -> &super::snapshot::UpdateSnapshot {
        &self.base
    }

    pub(super) fn topology(&self) -> &renderpilot_domain::GameProxyTopology {
        &self.topology
    }

    pub(super) fn root_seal(&self) -> &crate::addons::renodx::peer::RenoDxRootSeal {
        &self.root_seal
    }

    pub(super) fn addon_path(&self) -> &renderpilot_domain::PathRef {
        &self.addon_path
    }

    pub(super) fn record(&self) -> &renderpilot_domain::InstalledAddon {
        self.base.record()
    }
}

impl ActiveLoweredUpdate {
    pub(super) fn composition(
        &self,
    ) -> &crate::addons::renodx::peer::RenoDxActiveUpdateComposition {
        &self.composition
    }

    pub(super) fn addon_mtime(&self) -> Option<&str> {
        self.addon_mtime.as_deref()
    }

    pub(super) fn is_noop(&self) -> bool {
        matches!(
            self.composition,
            crate::addons::renodx::peer::RenoDxActiveUpdateComposition::Noop
        )
    }
}
