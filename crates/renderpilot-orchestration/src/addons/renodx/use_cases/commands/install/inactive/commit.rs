use std::path::Path;
use std::time::SystemTime;

use renderpilot_domain::{GameId, InstalledAddon};

use crate::addons::renodx::errors;
use crate::addons::renodx::install::install as install_files;
use crate::addons::renodx::use_cases::commands::shared_vulkan_layer;
use crate::addons::reshade::proxy::HostKind;
use crate::addons::reshade::types::ReshadeChannel;
use crate::{Context, ServiceError};

/// Applies the ordinary game-only commit safety authorization.
pub(super) fn authorize_install_commit<T>(
    context: &Context,
    feature: &'static str,
    guards: crate::mutation_boundary::GameMutationBoundary,
    safety: &crate::GameMutationSafetyPermits,
    game_commit: impl FnOnce(&crate::game_mutation_lock::GameMutationGuard) -> Result<T, ServiceError>,
) -> Result<T, ServiceError> {
    let authority = crate::FileSafetyAuthority::new();
    match guards {
        crate::mutation_boundary::GameMutationBoundary::Game(guard) => authority
            .authorize_game_commit(context, feature, &guard, safety.game(), || {
                game_commit(&guard)
            }),
        crate::mutation_boundary::GameMutationBoundary::GameShared(_) => {
            Err(ServiceError::command_failed(
                "a shared Vulkan install requires the combined mutation boundary",
            ))
        }
    }
}

/// Executes a Vulkan install whose game-file and shared-layer changes must be
/// published by one SVAM reservation. The engine and platform planners have
/// already captured their exact before/after states; this function only
/// composes them and supplies the durable database projections.
pub(super) struct CombinedVulkanInstallRequest<'a> {
    pub(super) context: &'a Context,
    pub(super) feature: &'static str,
    pub(super) guards: crate::mutation_boundary::GameMutationBoundary,
    pub(super) safety: &'a crate::GameMutationSafetyPermits,
    pub(super) game_id: &'a GameId,
    pub(super) game_dir: &'a Path,
    pub(super) prepared: &'a crate::addons::renodx::install::PreparedInstall,
    pub(super) registered_exe_path: Option<&'a Path>,
    pub(super) shared_change: shared_vulkan_layer::PreparedInstallChange,
    pub(super) source_last_modified: Option<&'a str>,
    pub(super) source_mtime: Option<SystemTime>,
    pub(super) targets: crate::addons::mutation_targets::MutationTargets,
}

pub(super) fn authorize_combined_vulkan_install(
    request: CombinedVulkanInstallRequest<'_>,
) -> Result<InstalledAddon, ServiceError> {
    let CombinedVulkanInstallRequest {
        context,
        feature,
        guards,
        safety,
        game_id,
        game_dir,
        prepared,
        registered_exe_path,
        shared_change,
        source_last_modified,
        source_mtime,
        targets,
    } = request;
    let crate::mutation_boundary::GameMutationBoundary::GameShared(guards) = guards else {
        return Err(ServiceError::command_failed(
            "a shared Vulkan install requires the combined mutation boundary",
        ));
    };
    let locked_plan = shared_change.resolve_locked_plan()?.ok_or_else(|| {
        ServiceError::command_failed("combined Vulkan install has no shared-layer plan")
    })?;
    let authority = crate::FileSafetyAuthority::new();
    if locked_plan.is_noop() {
        return authority.authorize_game_commit(
            context,
            feature,
            guards.game(),
            safety.game(),
            || {
                crate::addons::durable::run_install_mutation(
                    context,
                    guards.game(),
                    targets,
                    feature,
                    game_id,
                    || {
                        let (record, commit) = install_files(game_dir, prepared)?;
                        let record = super::phase::annotate_install_record(
                            record,
                            HostKind::Vulkan,
                            prepared.reshade_channel.unwrap_or(ReshadeChannel::Stable),
                            registered_exe_path,
                        )?;
                        if source_last_modified.is_some() || source_mtime.is_some() {
                            crate::fs::stamp_mtime_best_effort(
                                Path::new(record.addon_file().as_str()),
                                source_last_modified,
                                source_mtime,
                            );
                        }
                        Ok((record, commit))
                    },
                )
            },
        );
    }
    let input = shared_change
        .into_transaction_input(locked_plan)
        .ok_or_else(|| {
            ServiceError::command_failed("combined Vulkan install has no shared-layer plan")
        })?;
    authority.authorize_game_shared_commit(context, feature, &guards, safety, || {
        let shared_vulkan_layer::SharedLayerTransactionInput {
            plan: shared_plan,
            layer_dir,
            source,
        } = input;
        let participants =
            crate::addons::renodx::install::build_vulkan_game_participants(prepared, game_dir)?;
        let record =
            crate::addons::renodx::install::build_vulkan_record(prepared, game_dir, &participants)?;
        let record = super::phase::annotate_install_record(
            record,
            HostKind::Vulkan,
            prepared.reshade_channel.unwrap_or(ReshadeChannel::Stable),
            registered_exe_path,
        )?;
        let game_scope = crate::file_mutation::MutationScope::single(game_dir)?;
        let roots = crate::addons::shared_vulkan_mutation::TrustedRoots::game_shared(
            &game_scope,
            &layer_dir,
        )?;
        let writes_canonical = shared_plan.files.iter().any(|file| {
            matches!(
                file.path.file_name().and_then(|name| name.to_str()),
                Some("ReShade64.dll" | "ReShade64.json")
            )
        });
        let shared_record = if writes_canonical {
            source
                .as_ref()
                .map(|(source, download)| {
                    crate::addons::renodx::platform::vulkan::shared_artifact::downloaded_record(
                        &layer_dir, source, download,
                    )
                })
                .transpose()?
        } else {
            None
        };
        let composed =
            crate::addons::shared_vulkan_mutation::compose(Some(participants), Some(shared_plan))?;
        let shared_artifact = match shared_record.as_ref() {
            Some(record) => renderpilot_storage_sqlite::SharedArtifactMutation::Upsert(record),
            None => renderpilot_storage_sqlite::SharedArtifactMutation::Keep,
        };
        let registry = crate::addons::renodx::platform::vulkan::native_registry()
            .ok_or_else(errors::vulkan_unsupported_platform)?;
        let mutation_id = ulid::Ulid::generate().to_string();
        let identity = crate::addons::shared_vulkan_mutation::MutationIdentity::new(
            &mutation_id,
            crate::addons::shared_vulkan_mutation::ScopeSpec::game_upsert(game_id, &record),
            feature,
        );
        let physical = crate::addons::shared_vulkan_mutation::PhysicalParticipants::new(
            roots,
            composed,
            Some(registry),
        );
        let projection =
            crate::addons::shared_vulkan_mutation::CatalogProjection::new(shared_artifact);
        crate::addons::shared_vulkan_mutation::execute(
            crate::addons::shared_vulkan_mutation::Request::new(
                context, identity, physical, projection,
            ),
        )?;
        if source_last_modified.is_some() || source_mtime.is_some() {
            crate::fs::stamp_mtime_best_effort(
                Path::new(record.addon_file().as_str()),
                source_last_modified,
                source_mtime,
            );
        }
        Ok(record)
    })
}
