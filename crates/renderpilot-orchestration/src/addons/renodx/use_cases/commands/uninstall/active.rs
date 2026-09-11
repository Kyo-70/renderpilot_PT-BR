use std::path::{Path, PathBuf};

use renderpilot_application::ProxyTopologyRepository;
use renderpilot_domain::{
    AddonKind, GameId, InstalledAddon, InstalledAddonHostKind, PlannedGameProxyTopology,
    ProxyImplementation,
};

use crate::addons::game_analysis::{analyze_game, install_target_dir};
use crate::addons::records;
use crate::addons::renodx::errors;
use crate::addons::renodx::game_context::{executable_override, require_game};
use crate::addons::renodx::peer::{
    ActiveUninstallComposition, RenoDxRootAuthority, compose_active_uninstall,
    snapshot_active_uninstall,
};
use crate::addons::reshade::proxy::HostKind;
use crate::{Context, ServiceError};

pub(crate) fn uninstall_locked(
    context: &Context,
    guard: &crate::game_mutation_lock::GameMutationGuard,
    game_id: &GameId,
) -> Result<(), ServiceError> {
    let record = records::record_of_kind(context, game_id, AddonKind::RenoDx)?
        .ok_or_else(errors::not_installed)?;
    if record.host_kind() == Some(InstalledAddonHostKind::SharedVulkanLayer) {
        let executable = super::shared::registered_vulkan_exe(&record).ok_or_else(|| {
            ServiceError::invalid_input(
                "active shared RenoDX uninstall has no registered executable",
            )
        })?;
        return Err(ServiceError::invalid_input(format!(
            "active shared RenoDX uninstall requires the combined mutation boundary ({})",
            executable.display()
        )));
    }
    uninstall_locked_with_record(context, guard, game_id, &record)
}

pub(super) fn uninstall_locked_with_record(
    context: &Context,
    guard: &crate::game_mutation_lock::GameMutationGuard,
    game_id: &GameId,
    record: &InstalledAddon,
) -> Result<(), ServiceError> {
    let topology = context
        .storage()
        .get_proxy_topology(game_id)?
        .ok_or_else(|| ServiceError::invalid_input("active RenoDX uninstall has no topology"))?;
    let authority = resolve_authority(context, game_id, record)?;
    let prepared = snapshot_active_uninstall(record, &topology, &authority)?;
    let game_root = prepared.root().canonical_game_root().to_path_buf();
    let payload_root = prepared.root().payload_root().map(Path::to_path_buf);
    let composition = compose_active_uninstall(prepared.input()).map_err(map_active_error)?;
    commit_game(ActiveUninstallGameCommit {
        context,
        guard,
        game_id,
        record,
        topology: &topology,
        composition,
        game_root,
        payload_root,
    })
}

struct ActiveUninstallGameCommit<'a> {
    context: &'a Context,
    guard: &'a crate::game_mutation_lock::GameMutationGuard,
    game_id: &'a GameId,
    record: &'a InstalledAddon,
    topology: &'a renderpilot_domain::GameProxyTopology,
    composition: ActiveUninstallComposition,
    game_root: PathBuf,
    payload_root: Option<PathBuf>,
}

fn commit_game(
    ActiveUninstallGameCommit {
        context,
        guard,
        game_id,
        record,
        topology,
        composition,
        game_root,
        payload_root,
    }: ActiveUninstallGameCommit<'_>,
) -> Result<(), ServiceError> {
    let ActiveUninstallComposition {
        program,
        payloads,
        game_intents: _,
        planned_topology,
        reshade_ini_authority,
    } = composition;
    let final_proxy_release = record.host_kind() == Some(InstalledAddonHostKind::Proxy)
        && topology.outer.implementation == ProxyImplementation::OptiScaler
        && matches!(
            &planned_topology,
            PlannedGameProxyTopology::Exact(after) if after.downstream.is_none()
        );
    let optiscaler_config = if final_proxy_release {
        super::super::optiscaler_config::plan(context, game_id, &game_root, false)?
    } else {
        None
    };
    let (package, use_optiscaler_config) = if let Some(config) = optiscaler_config {
        let (companion, endpoint, payload) = config.into_parts();
        let program = program
            .prepend(endpoint)
            .map_err(|error| ServiceError::command_failed(error.to_string()))?;
        let mut payloads = payloads;
        payloads.insert(0, Some(payload));
        let request = crate::addons::peer_lifecycle::package::PeerMutationRequest {
            peer_kind: AddonKind::RenoDx,
            before_peer: Some(record),
            after_peer: None,
            before_topology: topology,
            planned_after_topology: &planned_topology,
            program,
            payloads,
            game_root,
            payload_root,
            component_set: None,
            baseline_mutations: &[],
            catalog_claim: None,
        };
        (
            crate::addons::peer_lifecycle::package::PeerMutationPackage::plan_active_with_renodx_optiscaler_config(
                request,
                reshade_ini_authority,
                companion,
            )?,
            true,
        )
    } else {
        let request = crate::addons::peer_lifecycle::package::PeerMutationRequest {
            peer_kind: AddonKind::RenoDx,
            before_peer: Some(record),
            after_peer: None,
            before_topology: topology,
            planned_after_topology: &planned_topology,
            program,
            payloads,
            game_root,
            payload_root,
            component_set: None,
            baseline_mutations: &[],
            catalog_claim: None,
        };
        let package = match reshade_ini_authority {
            Some(authority) => crate::addons::peer_lifecycle::package::PeerMutationPackage::plan_active_with_renodx_reshade_ini(request, authority)?,
            None => crate::addons::peer_lifecycle::package::PeerMutationPackage::plan_active(request)?,
        };
        (package, false)
    };
    let prepared = if use_optiscaler_config {
        context
            .peer_mutation_executor()
            .prepare_ordinary_file_peer_with_renodx_optiscaler_config(
                context,
                guard,
                crate::addons::mutation_features::RENODX_UNINSTALL,
                Some(game_id.as_str()),
                package,
            )?
    } else {
        context
            .peer_mutation_executor()
            .prepare_ordinary_file_peer(
                context,
                guard,
                crate::addons::mutation_features::RENODX_UNINSTALL,
                Some(game_id.as_str()),
                package,
            )?
    };
    let mut changes = crate::addons::engine::InstallChanges::default();
    let applied = prepared.apply(&mut changes)?;
    changes.sync_touched_dirs();
    applied.commit()?;
    super::shared::remove_logs_best_effort(record);
    Ok(())
}

pub(super) fn resolve_authority(
    context: &Context,
    game_id: &GameId,
    record: &InstalledAddon,
) -> Result<RenoDxRootAuthority, ServiceError> {
    let game = require_game(context, game_id)?;
    let game_dir = install_target_dir(&analyze_game(
        &game,
        executable_override(context, game_id).as_deref(),
    ))?;
    let host_kind = match record.host_kind() {
        Some(InstalledAddonHostKind::Proxy) => HostKind::Proxy,
        Some(InstalledAddonHostKind::SharedVulkanLayer) => HostKind::Vulkan,
        None => {
            return Err(ServiceError::invalid_input(
                "active RenoDX record has no host kind",
            ));
        }
    };
    RenoDxRootAuthority::resolve(
        &game_dir,
        host_kind,
        None,
        record
            .registered_exe_path()
            .map(|path| Path::new(path.as_str())),
    )
}

pub(super) fn map_active_error(
    error: crate::addons::renodx::peer::RenoDxActiveUninstallError,
) -> ServiceError {
    match error {
        crate::addons::renodx::peer::RenoDxActiveUninstallError::Domain(error) => error,
        crate::addons::renodx::peer::RenoDxActiveUninstallError::Invalid(reason) => {
            ServiceError::invalid_input(reason)
        }
        crate::addons::renodx::peer::RenoDxActiveUninstallError::Path(path) => {
            ServiceError::invalid_input(format!("invalid active RenoDX path: {}", path.display()))
        }
    }
}
