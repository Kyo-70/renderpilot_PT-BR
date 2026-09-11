use std::path::Path;
use std::time::SystemTime;

use renderpilot_domain::{Architecture, GameProxyTopology, InstalledAddon};

use crate::ServiceError;
use crate::addons::renodx::fetch::{LocalAddonSource, prepare_install, prepare_install_from_file};
use crate::addons::renodx::use_cases::commands::install::InstallRequest;
use crate::addons::renodx::use_cases::commands::shared_vulkan_layer;
use crate::addons::reshade::types::ReshadeChannel;
use crate::net::ProgressObserver;

use super::commit::{ActiveInstallCommit, commit_prepared};
use super::local_file::read_addon_file;
use super::model::{ActiveInstallSource, ResolveActiveInstallRequest};
use super::phase::resolve_phase1;

enum ActiveSource {
    Catalog,
    File {
        bytes: Vec<u8>,
        source_mtime: Option<SystemTime>,
        architecture: Architecture,
    },
}

pub(super) async fn install(
    request: InstallRequest<'_>,
    selected_topology: GameProxyTopology,
) -> Result<InstalledAddon, ServiceError> {
    ensure_requested_channel(request.reshade_sources, request.requested_channel)?;
    run(request, selected_topology, ActiveSource::Catalog).await
}

pub(super) async fn install_from_file(
    request: InstallRequest<'_>,
    file_path: &str,
    selected_topology: GameProxyTopology,
) -> Result<InstalledAddon, ServiceError> {
    ensure_requested_channel(request.reshade_sources, request.requested_channel)?;
    let (bytes, source_mtime) = read_addon_file(file_path)?;
    let architecture = crate::addons::renodx::arch_from_addon_file(file_path).ok_or_else(|| {
        crate::addons::renodx::errors::invalid(
            "the selected file is not a RenoDX add-on (.addon64 / .addon32)".to_owned(),
        )
    })?;
    run(
        request,
        selected_topology,
        ActiveSource::File {
            bytes,
            source_mtime,
            architecture,
        },
    )
    .await
}

async fn run(
    request: InstallRequest<'_>,
    selected_topology: GameProxyTopology,
    source: ActiveSource,
) -> Result<InstalledAddon, ServiceError> {
    let InstallRequest {
        context,
        manifest,
        reshade_sources,
        game_id,
        requested_channel,
        safety,
        allow_shared_vulkan_layer_install,
        progress,
    } = request;
    let (variant, phase1, prepared, source_last_modified, source_mtime) = match source {
        ActiveSource::Catalog => {
            let variant = ActiveInstallSource::Catalog;
            let phase1 = resolve_phase1_under_game_boundary(
                context,
                manifest,
                game_id,
                requested_channel,
                selected_topology.clone(),
                variant,
            )
            .await?;
            let prepared = prepare_install(
                phase1.plan(),
                reshade_sources,
                game_id.clone(),
                requested_channel,
                phase1.snapshot().writes_host(),
                progress,
            )
            .await?;
            let source_last_modified = prepared.source_last_modified.clone();
            (variant, phase1, prepared, source_last_modified, None)
        }
        ActiveSource::File {
            bytes,
            source_mtime,
            architecture,
        } => {
            let variant = ActiveInstallSource::InstallFromFile { architecture };
            let phase1 = resolve_phase1_under_game_boundary(
                context,
                manifest,
                game_id,
                requested_channel,
                selected_topology.clone(),
                variant,
            )
            .await?;
            let prepared = prepare_install_from_file(
                phase1.plan(),
                reshade_sources,
                game_id.clone(),
                LocalAddonSource {
                    bytes,
                    last_modified: source_mtime.map(crate::fs::format_http_date),
                },
                requested_channel,
                phase1.snapshot().writes_host(),
                progress,
            )
            .await?;
            (variant, phase1, prepared, None, source_mtime)
        }
    };
    let shared_change = prepare_shared(
        phase1.plan(),
        phase1.snapshot().registered_executable(),
        reshade_sources,
        requested_channel,
        allow_shared_vulkan_layer_install,
        progress,
    )
    .await?;
    let feature = feature_for_source(variant);
    commit_prepared(ActiveInstallCommit {
        context,
        manifest,
        game_id,
        requested_channel,
        selected_topology,
        source: variant,
        phase1,
        prepared,
        shared_change,
        safety,
        progress,
        source_last_modified,
        source_mtime,
        feature,
    })
    .await
}

async fn resolve_phase1_under_game_boundary(
    context: &crate::Context,
    manifest: &crate::addons::renodx::types::RenoDxManifest,
    game_id: &renderpilot_domain::GameId,
    requested_channel: ReshadeChannel,
    selected_topology: GameProxyTopology,
    source: ActiveInstallSource,
) -> Result<super::model::ActiveInstallResolution, ServiceError> {
    let _guard =
        crate::mutation_boundary::enter_game_mutation_boundary_async(context, game_id).await?;
    let request = ResolveActiveInstallRequest {
        context,
        manifest,
        game_id,
        requested_channel,
        selected_topology,
        source,
    };
    resolve_phase1(&request)
}

async fn prepare_shared(
    plan: &crate::addons::renodx::matcher::ResolvedInstall,
    registered_executable: Option<&renderpilot_domain::PathRef>,
    reshade_sources: &crate::addons::reshade::types::ReshadeSourceCatalog,
    channel: ReshadeChannel,
    allow_shared_vulkan_layer_install: bool,
    progress: Option<&ProgressObserver<'_>>,
) -> Result<shared_vulkan_layer::PreparedInstallChange, ServiceError> {
    shared_vulkan_layer::prepare_for_install(shared_vulkan_layer::PrepareInstallRequest {
        plan,
        reshade_config: reshade_sources,
        channel,
        allow_shared_vulkan_layer_install,
        exe_path: registered_executable.map(|path| Path::new(path.as_str())),
        progress,
    })
    .await
}

fn ensure_requested_channel(
    reshade_sources: &crate::addons::reshade::types::ReshadeSourceCatalog,
    channel: ReshadeChannel,
) -> Result<(), ServiceError> {
    if reshade_sources.supports_channel(channel) {
        Ok(())
    } else {
        Err(crate::addons::renodx::errors::channel_unavailable(channel))
    }
}

fn feature_for_source(source: ActiveInstallSource) -> &'static str {
    match source {
        ActiveInstallSource::Catalog => crate::addons::mutation_features::RENODX_INSTALL,
        ActiveInstallSource::InstallFromFile { .. } => {
            crate::addons::mutation_features::RENODX_INSTALL_FROM_FILE
        }
    }
}

#[cfg(test)]
mod tests {
    use renderpilot_domain::Architecture;

    use super::*;

    #[test]
    fn catalog_and_file_sources_keep_distinct_mutation_features() {
        assert_eq!(
            feature_for_source(ActiveInstallSource::Catalog),
            crate::addons::mutation_features::RENODX_INSTALL
        );
        assert_eq!(
            feature_for_source(ActiveInstallSource::InstallFromFile {
                architecture: Architecture::X64,
            }),
            crate::addons::mutation_features::RENODX_INSTALL_FROM_FILE
        );
    }
}
