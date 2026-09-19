use std::path::{Path, PathBuf};

use renderpilot_application::ProxyTopologyRepository;
use renderpilot_domain::{AddonKind, GameId, InstalledAddon, InstalledAddonHostKind};

use crate::addons::renodx::errors;
use crate::addons::renodx::peer::{compose_active_uninstall, snapshot_active_uninstall};
use crate::{Context, ServiceError};

pub(super) fn uninstall_shared_locked(
    context: &Context,
    guards: &crate::mutation_boundary::GameSharedMutationGuards,
    game_id: &GameId,
    record: &InstalledAddon,
) -> Result<(), ServiceError> {
    let executable = registered_vulkan_exe(record).ok_or_else(|| {
        ServiceError::invalid_input("active shared RenoDX uninstall has no registered executable")
    })?;
    uninstall_shared_locked_under_boundary(context, guards, game_id, record, &executable)
}

fn uninstall_shared_locked_under_boundary(
    context: &Context,
    guards: &crate::mutation_boundary::GameSharedMutationGuards,
    game_id: &GameId,
    record: &InstalledAddon,
    executable: &Path,
) -> Result<(), ServiceError> {
    let topology = context
        .storage()
        .get_proxy_topology(game_id)?
        .ok_or_else(|| {
            ServiceError::invalid_input("active shared RenoDX uninstall has no topology")
        })?;
    let authority = super::active::resolve_authority(context, game_id, record)?;
    let prepared = snapshot_active_uninstall(record, &topology, &authority)?;
    let game_root = prepared.root().canonical_game_root().to_path_buf();
    let payload_root = prepared.root().payload_root().map(Path::to_path_buf);
    let composition =
        compose_active_uninstall(prepared.input()).map_err(super::active::map_active_error)?;
    let layer_dir = crate::addons::renodx::platform::vulkan::program_data::layer_dir()
        .ok_or_else(errors::vulkan_unsupported_platform)?;
    let registry = crate::addons::renodx::platform::vulkan::native_registry()
        .ok_or_else(errors::vulkan_unsupported_platform)?;
    let observation = renderpilot_platform_windows::vulkan_layer::observe_shared_vulkan_layer(
        registry, &layer_dir,
    )
    .map_err(|error| errors::failed(format!("failed to inspect shared Vulkan layer: {error}")))?;
    let shared_plan = renderpilot_platform_windows::vulkan_layer::plan_unregister_app_only(
        observation,
        executable,
    )
    .map_err(|error| errors::failed(error.to_string()))?;
    if shared_plan.unregister_outcome
        == Some(renderpilot_platform_windows::vulkan_layer::AppUnregisterOutcome::TargetAbsent)
    {
        return super::active::uninstall_locked_with_record(
            context,
            guards.game(),
            game_id,
            record,
        );
    }
    let removes_layer = shared_plan.authorizes_canonical_layer_removal();
    let mut participants = crate::addons::shared_vulkan_mutation::compose(None, Some(shared_plan))?;
    participants.prepend_files(composition.game_intents)?;
    let mut scope_roots = vec![game_root];
    if let Some(payload_root) = payload_root {
        scope_roots.push(payload_root);
    }
    let roots = crate::addons::shared_vulkan_mutation::TrustedRoots::game_shared(
        &crate::file_mutation::MutationScope::new(scope_roots)?,
        &layer_dir,
    )?;
    let artifact = if removes_layer {
        renderpilot_storage_sqlite::SharedArtifactMutation::Delete(
            renderpilot_domain::SharedArtifactKind::RenoDxVulkanLayer,
        )
    } else {
        renderpilot_storage_sqlite::SharedArtifactMutation::Keep
    };
    let id = ulid::Ulid::generate().to_string();
    let identity = crate::addons::shared_vulkan_mutation::MutationIdentity::new(
        &id,
        crate::addons::shared_vulkan_mutation::ScopeSpec::game_delete(game_id, AddonKind::RenoDx),
        crate::addons::mutation_features::RENODX_UNINSTALL,
    );
    let physical = crate::addons::shared_vulkan_mutation::PhysicalParticipants::new(
        roots,
        participants,
        Some(registry),
    );
    crate::addons::engine_config::service::release_record(
        context.storage(),
        game_id,
        record,
        &format!("renodx-release-{}", ulid::Ulid::generate()),
    )
    .map_err(|error| {
        ServiceError::command_failed(format!(
            "RenoDX Engine.ini release blocked uninstall: {error}"
        ))
    })?;
    let record_after_release =
        crate::addons::records::record_of_kind(context, game_id, AddonKind::RenoDx)?.ok_or_else(
            || ServiceError::command_failed("RenoDX record disappeared during Engine.ini release"),
        )?;
    let record_after_release = crate::addons::records::verify_engine_config_release(
        record,
        record_after_release,
        "RenoDX",
    )?;
    let request = crate::addons::shared_vulkan_mutation::Request::new(
        context,
        identity,
        physical,
        crate::addons::shared_vulkan_mutation::CatalogProjection::new(artifact),
    );
    let request = match composition.reshade_ini_authority.as_ref() {
        Some(authority) => {
            crate::addons::shared_vulkan_mutation::PeerUnchangedRequest::new_with_renodx_reshade_ini(
                request,
                game_id,
                Some(&record_after_release),
                None,
                Some(&topology),
                Some(&topology),
                authority,
            )
        }
        None => crate::addons::shared_vulkan_mutation::PeerUnchangedRequest::new(
            request,
            game_id,
            Some(&record_after_release),
            None,
            Some(&topology),
            Some(&topology),
        ),
    };
    crate::addons::shared_vulkan_mutation::execute_peer_unchanged(request)?;
    remove_logs_best_effort(&record_after_release);
    Ok(())
}

pub(super) fn registered_vulkan_exe(record: &InstalledAddon) -> Option<PathBuf> {
    matches!(
        record.host_kind(),
        Some(InstalledAddonHostKind::SharedVulkanLayer)
    )
    .then(|| {
        record
            .registered_exe_path()
            .map(|path| PathBuf::from(path.as_str()))
    })
    .flatten()
}

pub(super) fn remove_logs_best_effort(record: &InstalledAddon) {
    if let Some(host) = crate::addons::tracking::owned_proxy_host_path(record)
        && let Some(dir) = host.parent()
    {
        let base =
            crate::addons::reshade::scan::resolve_paths(dir, Some(&host)).effective_base_path;
        crate::addons::reshade::scan::remove_reshade_logs_best_effort(&base);
    }
}
