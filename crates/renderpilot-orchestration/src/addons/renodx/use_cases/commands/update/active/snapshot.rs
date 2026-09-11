//! Locked, local-only snapshot for an active RenoDX update.

use std::path::Path;

use renderpilot_domain::{GameProxyTopology, PathRef};

use crate::ServiceError;
use crate::addons::renodx::errors;
use crate::addons::renodx::game_context::require_game;
use crate::addons::renodx::peer::RenoDxRootAuthority;
use crate::addons::reshade::proxy::HostKind;
use crate::peer_mutation_executor::{PeerPathSnapshot, observe_peer_path_snapshot};

use super::super::snapshot::UpdateSnapshot;
use super::validation;

/// Complete immutable local evidence for one active RenoDX update.
#[derive(Debug)]
pub(crate) struct ActiveUpdatePhase1 {
    pub(super) base: UpdateSnapshot,
    pub(super) topology: GameProxyTopology,
    pub(super) root_seal: crate::addons::renodx::peer::RenoDxRootSeal,
    pub(super) addon_path: PathRef,
    pub(super) addon_snapshot: PeerPathSnapshot,
    pub(super) host_path: Option<PathRef>,
    pub(super) host_snapshot: Option<PeerPathSnapshot>,
}

pub(crate) fn snapshot_active_update(
    context: &crate::Context,
    base: UpdateSnapshot,
    topology: GameProxyTopology,
    game_id: &renderpilot_domain::GameId,
) -> Result<ActiveUpdatePhase1, ServiceError> {
    let game = require_game(context, game_id)?;
    let record = base.record();
    if record.game_id() != game_id || topology.game_id != *game_id {
        return Err(errors::state_changed_retry_update());
    }
    let host_kind = validation::explicit_host_kind(record)?;
    let registered_exe = match host_kind {
        HostKind::Proxy => None,
        HostKind::Vulkan => Some(record.registered_exe_path().ok_or_else(|| {
            errors::invalid("active Vulkan RenoDX record has no registered executable".to_owned())
        })?),
    };
    let authority = RenoDxRootAuthority::resolve(
        Path::new(game.install_path().as_str()),
        host_kind,
        None,
        registered_exe.map(|path| Path::new(path.as_str())),
    )?;
    validation::topology(&topology, game_id, authority.canonical_game_root())?;
    validation::record_paths(record, &authority)?;

    let addon_path = record.addon_file().clone();
    let addon_root = authority.authorized_root(&addon_path)?.clone();
    let addon_snapshot = observe_peer_path_snapshot(&addon_path, &addon_root)?;
    if addon_snapshot.file().is_none() {
        return Err(errors::invalid(
            "active RenoDX add-on payload is absent".to_owned(),
        ));
    }

    let (host_path, host_snapshot) = match host_kind {
        HostKind::Proxy => {
            let host_path = authority
                .exact_proxy_host()
                .ok_or_else(|| errors::failed("RenoDX proxy authority has no exact host path"))?;
            let host_path = authority.path_ref(host_path, "ReShade64.dll host path")?;
            validation::proxy_host(
                record,
                &topology,
                &host_path,
                authority.canonical_game_root(),
            )?;
            if let Some(target) = base.host_target()
                && !crate::paths::same_path(&target.target_path, Path::new(host_path.as_str()))
            {
                return Err(errors::invalid(
                    "RenoDX active host update target differs from the sealed ReShade64.dll path"
                        .to_owned(),
                ));
            }
            let host_root = authority.authorized_root(&host_path)?.clone();
            let snapshot = observe_peer_path_snapshot(&host_path, &host_root)?;
            validation::host_snapshot(&topology, record, &snapshot)?;
            (Some(host_path), Some(snapshot))
        }
        HostKind::Vulkan => {
            validation::vulkan_record(record, &base, authority.seal().canonical_registered_exe())?;
            (None, None)
        }
    };

    Ok(ActiveUpdatePhase1 {
        base,
        topology,
        root_seal: authority.seal().clone(),
        addon_path,
        addon_snapshot,
        host_path,
        host_snapshot,
    })
}
