//! Sealed phase snapshots for the active RenoDX DLSS-Fix route.

use std::path::Path;

use renderpilot_application::ProxyTopologyRepository;
use renderpilot_domain::{AddonKind, GameId, InstalledAddon, InstalledAddonHostKind};

use crate::addons::game_analysis::{analyze_game, install_target_dir};
use crate::addons::records;
use crate::addons::renodx::dlss_fix::{DlssFixRequest, resolve_dlss_fix};
use crate::addons::renodx::dlss_fix_binding::{self, DlssFixBinding};
use crate::addons::renodx::errors;
use crate::addons::renodx::peer::{RenoDxConfigSourceSeal, RenoDxRootAuthority, RenoDxRootSeal};
use crate::addons::reshade::proxy::HostKind;
use crate::peer_mutation_executor::{PeerPathSnapshot, observe_peer_path_snapshot};
use crate::{Context, ServiceError};

/// Complete immutable phase-one evidence for one active DLSS-Fix operation.
#[derive(Debug)]
pub(super) struct ActiveSnapshot {
    pub(super) record: InstalledAddon,
    pub(super) binding: DlssFixBinding,
    pub(super) topology: renderpilot_domain::GameProxyTopology,
    pub(super) root: RenoDxRootSeal,
    pub(super) companion_path: renderpilot_domain::PathRef,
    pub(super) companion: PeerPathSnapshot,
    pub(super) ini_path: renderpilot_domain::PathRef,
    pub(super) ini: PeerPathSnapshot,
    pub(super) request: Option<DlssFixRequest>,
}

/// Reads and seals aggregate, topology, root/config, companion, and INI facts.
/// The caller invokes this once before download and again under a fresh guard.
pub(super) fn resolve_snapshot(
    context: &Context,
    game_id: &GameId,
    need_request: bool,
) -> Result<ActiveSnapshot, ServiceError> {
    let record = records::record_of_kind(context, game_id, AddonKind::RenoDx)?
        .ok_or_else(errors::not_installed)?;
    let topology = context
        .storage()
        .get_proxy_topology(game_id)?
        .ok_or_else(errors::state_changed_retry_update)?;
    if topology.game_id != *game_id {
        return Err(errors::state_changed_retry_update());
    }
    let binding = dlss_fix_binding::resolve(&record);
    let authority = resolve_authority(context, game_id, &record)?;
    authority
        .roots()
        .require_game_path(&topology.root_slot)
        .map_err(|error| {
            errors::invalid(format!(
                "active DLSS-Fix topology is outside its root: {error}"
            ))
        })?;

    let companion_path = authority.path_ref(&binding.target, "DLSS-Fix companion path")?;
    let companion_root = authority.authorized_root(&companion_path)?.clone();
    let companion = observe_peer_path_snapshot(&companion_path, &companion_root)?;

    let ini_path = authority.path_ref(authority.exact_ini_path(), "ReShade.ini path")?;
    authority.roots().require_game_path(&ini_path)?;
    let ini = observe_peer_path_snapshot(&ini_path, authority.canonical_game_root_ref())?;
    validate_config_snapshot(authority.seal(), &ini)?;

    let request = need_request
        .then(|| resolve_dlss_fix(context.storage(), game_id))
        .transpose()?
        .flatten();
    Ok(ActiveSnapshot {
        record,
        binding,
        topology,
        root: authority.seal().clone(),
        companion_path,
        companion,
        ini_path,
        ini,
        request,
    })
}

fn resolve_authority(
    context: &Context,
    game_id: &GameId,
    record: &InstalledAddon,
) -> Result<RenoDxRootAuthority, ServiceError> {
    let game = crate::addons::renodx::game_context::require_game(context, game_id)?;
    let game_dir = install_target_dir(&analyze_game(
        &game,
        crate::addons::renodx::game_context::executable_override(context, game_id).as_deref(),
    ))?;
    let (host_kind, registered_exe) = match record.host_kind() {
        Some(InstalledAddonHostKind::Proxy) => (HostKind::Proxy, None),
        Some(InstalledAddonHostKind::SharedVulkanLayer) => (
            HostKind::Vulkan,
            Some(record.registered_exe_path().ok_or_else(|| {
                errors::invalid(
                    "active Vulkan RenoDX record has no registered executable".to_owned(),
                )
            })?),
        ),
        None => {
            return Err(errors::invalid(
                "active RenoDX record has no host kind".to_owned(),
            ));
        }
    };
    RenoDxRootAuthority::resolve(
        &game_dir,
        host_kind,
        None,
        registered_exe.map(|path| Path::new(path.as_str())),
    )
}

fn validate_config_snapshot(
    root: &RenoDxRootSeal,
    snapshot: &PeerPathSnapshot,
) -> Result<(), ServiceError> {
    match root.config_source() {
        RenoDxConfigSourceSeal::Absent { .. } => {
            if snapshot.file().is_some() {
                return Err(errors::state_changed_retry_update());
            }
        }
        RenoDxConfigSourceSeal::File {
            identity,
            digest,
            length,
            owned_bytes,
            ..
        } => {
            let Some(file) = snapshot.file() else {
                return Err(errors::state_changed_retry_update());
            };
            if file.identity() != identity
                || file.digest() != digest
                || file.length() != *length
                || snapshot.bytes() != Some(owned_bytes.as_slice())
            {
                return Err(errors::state_changed_retry_update());
            }
        }
    }
    Ok(())
}

/// Proves phase-three reads still describe exactly the phase-one operation.
pub(super) fn ensure_same(
    before: &ActiveSnapshot,
    current: &ActiveSnapshot,
) -> Result<(), ServiceError> {
    if before.record != current.record
        || !same_binding(&before.binding, &current.binding)
        || before.topology != current.topology
        || before.root != current.root
        || before.companion_path != current.companion_path
        || before.companion != current.companion
        || before.ini_path != current.ini_path
        || before.ini != current.ini
        || before.request != current.request
    {
        return Err(errors::state_changed_retry_update());
    }
    Ok(())
}

fn same_binding(left: &DlssFixBinding, right: &DlssFixBinding) -> bool {
    left.state == right.state
        && left.target == right.target
        && left.arch == right.arch
        && left.observation == right.observation
        && left.source == right.source
        && left.isolation_paths == right.isolation_paths
        && left.has_evidence == right.has_evidence
        && left.main_payload_collides() == right.main_payload_collides()
}
