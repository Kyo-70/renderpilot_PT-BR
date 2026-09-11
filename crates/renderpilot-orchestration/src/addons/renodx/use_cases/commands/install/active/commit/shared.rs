use std::path::PathBuf;
use std::time::SystemTime;

use renderpilot_domain::{GameProxyTopology, InstalledAddon, InstalledAddonHostKind};

use crate::addons::renodx::peer::{
    ActiveSharedMutationError, ActiveSharedMutationRequest, SharedLayerSource,
    execute_active_shared_mutation,
};
use crate::addons::renodx::use_cases::commands::shared_vulkan_layer::{
    PreparedInstallChange, SharedLayerTransactionInput,
};
use crate::{Context, ServiceError};

/// Complete sealed input for a shared-Vulkan active RenoDX install commit.
pub(super) struct ActiveSharedCommit<'a> {
    pub(super) context: &'a Context,
    pub(super) game_id: &'a renderpilot_domain::GameId,
    pub(super) feature: &'static str,
    pub(super) safety: crate::GameMutationSafetyPermits,
    pub(super) guards: crate::mutation_boundary::GameMutationBoundary,
    pub(super) shared_change: PreparedInstallChange,
    pub(super) locked_plan: renderpilot_platform_windows::vulkan_layer::SharedVulkanLayerPlan,
    pub(super) record: InstalledAddon,
    pub(super) game_intents: Vec<crate::addons::shared_vulkan_mutation::FileIntent>,
    pub(super) topology: GameProxyTopology,
    pub(super) game_root: PathBuf,
    pub(super) reshade_ini_authority: Option<renderpilot_domain::RenoDxReshadeIniAuthority>,
    pub(super) addon_path: PathBuf,
    pub(super) source_last_modified: Option<String>,
    pub(super) source_mtime: Option<SystemTime>,
}

pub(super) fn commit_shared(
    request: ActiveSharedCommit<'_>,
) -> Result<InstalledAddon, ServiceError> {
    let ActiveSharedCommit {
        context,
        game_id,
        feature,
        safety,
        guards,
        shared_change,
        locked_plan,
        record,
        game_intents,
        topology,
        game_root,
        reshade_ini_authority,
        addon_path,
        source_last_modified,
        source_mtime,
    } = request;
    let crate::mutation_boundary::GameMutationBoundary::GameShared(guards) = guards else {
        return Err(ServiceError::command_failed(
            "active RenoDX shared install requires the combined mutation boundary",
        ));
    };
    if record.host_kind() != Some(InstalledAddonHostKind::SharedVulkanLayer) {
        return Err(ServiceError::command_failed(
            "active RenoDX shared install produced a non-Vulkan record",
        ));
    }
    let input = shared_change
        .into_transaction_input(locked_plan)
        .ok_or_else(|| ServiceError::command_failed("active RenoDX shared install has no input"))?;
    let SharedLayerTransactionInput {
        plan: shared_plan,
        layer_dir,
        source,
    } = input;
    let registry = crate::addons::renodx::platform::vulkan::native_registry()
        .ok_or_else(crate::addons::renodx::errors::vulkan_unsupported_platform)?;
    crate::FileSafetyAuthority::new().authorize_game_shared_commit(
        context,
        feature,
        &guards,
        &safety,
        || {
            let source: Option<SharedLayerSource<'_>> =
                source.as_ref().map(|(source, download)| (source, download));
            let result = execute_active_shared_mutation(ActiveSharedMutationRequest {
                context,
                feature,
                game_id,
                game_root: &game_root,
                topology: &topology,
                before_record: None,
                after_record: record,
                game_intents,
                shared_plan,
                reshade_ini_authority: reshade_ini_authority.as_ref(),
                layer_dir: &layer_dir,
                source,
                shared_record: None,
                registry,
            })
            .map_err(shared_mutation_error)?;
            crate::fs::stamp_mtime_best_effort(
                &addon_path,
                source_last_modified.as_deref(),
                source_mtime,
            );
            Ok(result)
        },
    )
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
