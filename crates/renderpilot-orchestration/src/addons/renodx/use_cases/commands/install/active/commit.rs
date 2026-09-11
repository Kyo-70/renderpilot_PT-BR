mod game;
mod shared;

use std::path::Path;
use std::time::SystemTime;

use renderpilot_domain::{GameProxyTopology, InstalledAddon};

use crate::addons::renodx::install::PreparedInstall;
use crate::addons::renodx::peer::{
    PreparedActiveInstall, RenoDxActiveInstallError, compose_active_install,
};
use crate::addons::renodx::use_cases::commands::shared_vulkan_layer::PreparedInstallChange;
use crate::addons::reshade::types::ReshadeChannel;
use crate::net::ProgressObserver;
use crate::{Context, ServiceError};

use super::model::{ActiveInstallResolution, ActiveInstallSource, ResolveActiveInstallRequest};
use super::phase::resolve_phase3;

use game::{ActiveGameCommit, commit_game};
use shared::{ActiveSharedCommit, commit_shared};

pub(super) struct ActiveInstallCommit<'a> {
    pub(super) context: &'a Context,
    pub(super) manifest: &'a crate::addons::renodx::types::RenoDxManifest,
    pub(super) game_id: &'a renderpilot_domain::GameId,
    pub(super) requested_channel: ReshadeChannel,
    pub(super) selected_topology: GameProxyTopology,
    pub(super) source: ActiveInstallSource,
    pub(super) phase1: ActiveInstallResolution,
    pub(super) prepared: PreparedInstall,
    pub(super) shared_change: PreparedInstallChange,
    pub(super) safety: crate::GameMutationSafetyPermits,
    pub(super) progress: Option<&'a ProgressObserver<'a>>,
    pub(super) source_last_modified: Option<String>,
    pub(super) source_mtime: Option<SystemTime>,
    pub(super) feature: &'static str,
}

pub(super) async fn commit_prepared(
    request: ActiveInstallCommit<'_>,
) -> Result<InstalledAddon, ServiceError> {
    let ActiveInstallCommit {
        context,
        manifest,
        game_id,
        requested_channel,
        selected_topology,
        source,
        phase1,
        prepared,
        shared_change,
        safety,
        progress,
        source_last_modified,
        source_mtime,
        feature,
    } = request;
    let use_shared_boundary = shared_change.mutates_shared_resource();
    let guards = crate::mutation_boundary::enter_mutation_boundary_async(
        context,
        game_id,
        use_shared_boundary,
    )
    .await?;
    let phase3_request = ResolveActiveInstallRequest {
        context,
        manifest,
        game_id,
        requested_channel,
        selected_topology,
        source,
    };
    let phase3 = resolve_phase3(&phase3_request, &phase1)?;
    let before_topology =
        phase3.snapshot().topology().cloned().ok_or_else(|| {
            ServiceError::command_failed("active RenoDX install lost its topology")
        })?;
    let game_root = phase3
        .snapshot()
        .root_seal()
        .canonical_game_root()
        .to_path_buf();
    let payload_root = phase3
        .snapshot()
        .root_seal()
        .payload_root()
        .map(Path::to_path_buf);
    let (_, phase1_snapshot) = phase1.into_parts();
    let (_, phase3_snapshot) = phase3.into_parts();
    let composition = compose_active_install(PreparedActiveInstall::new(
        phase1_snapshot,
        prepared,
        phase3_snapshot,
    ))
    .map_err(active_install_error)?;
    let (record, program, payloads, game_intents, planned_topology, reshade_ini_authority) =
        composition.into_parts();
    let planned_topology = planned_topology.ok_or_else(|| {
        ServiceError::command_failed("active RenoDX install produced no planned topology")
    })?;
    let addon_path = std::path::PathBuf::from(record.addon_file().as_str());

    let locked_plan = if use_shared_boundary {
        Some(shared_change.resolve_locked_plan()?.ok_or_else(|| {
            ServiceError::command_failed("active RenoDX shared install has no locked plan")
        })?)
    } else {
        None
    };

    crate::addons::progress::emit_tool_finalizing(progress, renderpilot_domain::AddonKind::RenoDx);
    match locked_plan {
        Some(locked_plan) if !locked_plan.is_noop() => commit_shared(ActiveSharedCommit {
            context,
            game_id,
            feature,
            safety,
            guards,
            shared_change,
            locked_plan,
            record,
            game_intents,
            topology: before_topology,
            game_root,
            reshade_ini_authority,
            addon_path,
            source_last_modified,
            source_mtime,
        }),
        _ => {
            let game_guard = match guards {
                crate::mutation_boundary::GameMutationBoundary::Game(guard) => guard,
                crate::mutation_boundary::GameMutationBoundary::GameShared(guards) => {
                    guards.into_game()
                }
            };
            commit_game(ActiveGameCommit {
                context,
                game_id,
                feature,
                safety: &safety,
                guard: game_guard,
                record,
                program,
                payloads,
                before_topology,
                planned_topology,
                game_root,
                payload_root,
                reshade_ini_authority,
                addon_path,
                source_last_modified,
                source_mtime,
            })
        }
    }
}

fn active_install_error(error: RenoDxActiveInstallError) -> ServiceError {
    match error {
        RenoDxActiveInstallError::Retry(error) => error,
        error => ServiceError::command_failed(error.to_string()),
    }
}
