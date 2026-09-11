use std::path::Path;

use renderpilot_domain::InstalledAddon;

use crate::addons::renodx::peer::RenoDxActiveUpdateComposition;
use crate::addons::renodx::peer::{
    ActiveSharedMutationError, ActiveSharedMutationRequest, execute_active_shared_mutation,
};
use crate::addons::renodx::use_cases::commands::update::active::{
    ActiveLoweredUpdate, ActiveUpdatePhase1,
};
use crate::addons::renodx::use_cases::commands::update_reshade::PreparedSharedVulkanUpdate;
use crate::{Context, ServiceError};

pub(super) struct ActiveSharedUpdateCommit<'a> {
    pub(super) context: &'a Context,
    pub(super) game_id: &'a renderpilot_domain::GameId,
    pub(super) safety: &'a crate::GameMutationSafetyPermits,
    pub(super) guards: crate::mutation_boundary::GameMutationBoundary,
    pub(super) phase1: ActiveUpdatePhase1,
    pub(super) phase3: ActiveUpdatePhase1,
    pub(super) lowered: ActiveLoweredUpdate,
    pub(super) shared: PreparedSharedVulkanUpdate,
}

pub(super) fn commit(
    ActiveSharedUpdateCommit {
        context,
        game_id,
        safety,
        guards,
        phase1,
        phase3,
        lowered,
        shared,
    }: ActiveSharedUpdateCommit<'_>,
) -> Result<InstalledAddon, ServiceError> {
    let crate::mutation_boundary::GameMutationBoundary::GameShared(guards) = guards else {
        return Err(ServiceError::command_failed(
            "active RenoDX shared update requires the combined mutation boundary",
        ));
    };
    let (after_record, game_intents) = match lowered.composition() {
        RenoDxActiveUpdateComposition::Noop => (phase1.record().clone(), Vec::new()),
        RenoDxActiveUpdateComposition::Metadata(metadata) => {
            (metadata.after_peer().clone(), Vec::new())
        }
        RenoDxActiveUpdateComposition::Physical(physical) => (
            physical.after_peer().clone(),
            physical.game_intents().to_vec(),
        ),
    };
    let PreparedSharedVulkanUpdate {
        layer_dir,
        plan: shared_plan,
        shared_record,
        changed: _,
    } = shared;
    let registry = crate::addons::renodx::platform::vulkan::native_registry()
        .ok_or_else(crate::addons::renodx::errors::vulkan_unsupported_platform)?;
    crate::FileSafetyAuthority::new().authorize_game_shared_commit(
        context,
        crate::addons::mutation_features::RENODX_UPDATE,
        &guards,
        safety,
        || {
            let result = execute_active_shared_mutation(ActiveSharedMutationRequest {
                context,
                feature: crate::addons::mutation_features::RENODX_UPDATE,
                game_id,
                game_root: phase3.root_seal().canonical_game_root(),
                topology: phase3.topology(),
                before_record: Some(phase1.record()),
                after_record,
                game_intents,
                shared_plan,
                reshade_ini_authority: None,
                layer_dir: &layer_dir,
                source: None,
                shared_record: Some(&shared_record),
                registry,
            })
            .map_err(shared_mutation_error)?;
            stamp_addon_mtime(&phase3, lowered.addon_mtime());
            Ok(result)
        },
    )
}

fn stamp_addon_mtime(phase3: &ActiveUpdatePhase1, mtime: Option<&str>) {
    if let Some(mtime) = mtime {
        crate::fs::stamp_mtime_best_effort(
            Path::new(phase3.addon_path().as_str()),
            Some(mtime),
            None,
        );
    }
}

fn shared_mutation_error(error: ActiveSharedMutationError) -> ServiceError {
    match error {
        ActiveSharedMutationError::Artifact(error)
        | ActiveSharedMutationError::Transaction(error) => error,
        ActiveSharedMutationError::InvalidInput(reason) => ServiceError::command_failed(reason),
        ActiveSharedMutationError::InvalidPath(path) => {
            ServiceError::command_failed(format!("invalid active RenoDX path: {}", path.display()))
        }
    }
}
