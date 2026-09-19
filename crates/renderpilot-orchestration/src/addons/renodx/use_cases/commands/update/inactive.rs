//! Topology-free RenoDX update ceremony.

use crate::ServiceError;
use crate::addons::renodx::use_cases::commands::update::commit;
use crate::addons::renodx::use_cases::commands::update::prepare::{
    prepare_config_update, prepare_update_artifacts,
};
use crate::addons::renodx::use_cases::commands::update::route::{
    UpdatePhase1, ensure_update_route_matches, snapshot_update_route,
};
use crate::addons::renodx::use_cases::commands::update::snapshot::UpdateSnapshot;
use crate::addons::renodx::use_cases::commands::update_reshade::PreparedReShadeUpdate;

pub(super) async fn update(
    request: super::UpdateRequest<'_>,
    phase1: UpdateSnapshot,
) -> Result<(), ServiceError> {
    let super::UpdateRequest {
        context,
        manifest,
        reshade_sources,
        game_id,
        safety,
        progress,
    } = request;
    let prepared = prepare_update_artifacts(&phase1, progress).await?;
    let shared_update = match phase1.shared_vulkan_channel() {
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
    let phase3_route =
        snapshot_update_route(context, manifest, reshade_sources, game_guard(&guards))?;
    ensure_update_route_matches(&UpdatePhase1::Inactive(Box::new(phase1)), &phase3_route)?;
    let UpdatePhase1::Inactive(revalidated) = phase3_route else {
        return Err(crate::addons::renodx::errors::state_changed_retry_update());
    };
    let config = prepare_config_update(&revalidated)?;
    let prepared = prepared.with_config(config);

    crate::addons::progress::emit_tool_finalizing(progress, renderpilot_domain::AddonKind::RenoDx);
    let replacement_paths = prepared.replacement_paths();
    let host_install_path = prepared.host_install_path();
    let targets = crate::addons::renodx::mutation_targets::update_targets(
        revalidated.record(),
        &replacement_paths,
        host_install_path.as_deref(),
        prepared
            .config
            .as_ref()
            .and_then(|config| config.physical_changed.then_some(config.path.as_path())),
    )?;
    match shared_update {
        Some(shared_update) => commit::authorize_combined_update(commit::CombinedUpdateRequest {
            context,
            guards,
            safety: &safety,
            shared_update,
            artifacts: prepared,
            current: revalidated.record(),
            targets,
            game_id,
        }),
        None => commit::authorize_update_commit(context, guards, &safety, |guard| {
            commit::apply_update(commit::UpdateCommit {
                context,
                guard,
                artifacts: prepared,
                current: revalidated.record(),
                targets,
                game_id,
            })
        }),
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
