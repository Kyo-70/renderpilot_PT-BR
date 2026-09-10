use std::path::Path;

use renderpilot_domain::{
    FileOwnership, GameId, GameProxyTopology, PathRef, ProxyImplementation, normalized_path_key,
};

use crate::ServiceError;
use crate::addons::luma::dgvoodoo;
use crate::addons::luma::errors;
use crate::addons::luma::matcher::{LumaResolution, ResolvedLumaInstall};
use crate::peer_mutation_executor::PeerPathSnapshot;

use super::model::DgVoodooPrepKind;

pub(super) fn install_plan(
    resolution: LumaResolution,
) -> Result<ResolvedLumaInstall, ServiceError> {
    match resolution {
        LumaResolution::Installable(plan) => Ok(*plan),
        LumaResolution::Incompatible { reason } => Err(errors::invalid(format!(
            "Luma is not compatible with this game: {reason:?}"
        ))),
        LumaResolution::Blacklisted { message } => Err(errors::invalid(format!(
            "Luma is not supported for this game: {}",
            message.fallback_text
        ))),
        LumaResolution::NoMatch => Err(errors::invalid(
            "Luma has no profile for this game".to_owned(),
        )),
    }
}

pub(super) fn validate_topology(
    game_id: &GameId,
    game_root: &Path,
    topology: &GameProxyTopology,
    host_path: &PathRef,
) -> Result<(), ServiceError> {
    if &topology.game_id != game_id
        || topology.outer.implementation != ProxyImplementation::OptiScaler
    {
        return Err(errors::invalid(
            "active Luma proxy topology does not belong to this OptiScaler game",
        ));
    }
    let Some(parent) = Path::new(topology.root_slot.as_str()).parent() else {
        return Err(errors::invalid(
            "active Luma proxy topology root slot has no parent",
        ));
    };
    if normalized_path_key(&parent.to_string_lossy())
        != normalized_path_key(&game_root.to_string_lossy())
    {
        return Err(errors::invalid(
            "active Luma proxy topology root slot is not directly under the game root",
        ));
    }
    if let Some(downstream) = topology.downstream.as_ref()
        && (downstream.implementation != ProxyImplementation::ReShade
            || normalized_path_key(downstream.path.as_str())
                != normalized_path_key(host_path.as_str())
            || downstream.receipt.ownership() != FileOwnership::Reused)
    {
        return Err(errors::invalid(
            "active Luma topology requires a reused ReShade64.dll downstream",
        ));
    }
    Ok(())
}

pub(super) fn validate_host_snapshot(
    topology: &GameProxyTopology,
    snapshot: &PeerPathSnapshot,
) -> Result<(), ServiceError> {
    match topology.downstream.as_ref() {
        None if matches!(snapshot, PeerPathSnapshot::Absent) => Ok(()),
        None => Err(errors::invalid(
            "active Luma topology has no downstream but ReShade64.dll is present",
        )),
        Some(downstream) => {
            let Some(file) = snapshot.file() else {
                return Err(errors::invalid(
                    "active Luma topology downstream ReShade64.dll is absent",
                ));
            };
            if downstream.receipt.digest() != file.digest()
                || downstream.receipt.identity() != file.identity()
            {
                return Err(errors::state_changed_retry_install());
            }
            Ok(())
        }
    }
}

pub(super) fn assess_dgvoodoo_kind(
    plan: &ResolvedLumaInstall,
    target_dir: &Path,
) -> Result<DgVoodooPrepKind, ServiceError> {
    let Some(requirement) = dgvoodoo::requirement(plan.external_requirement.as_ref()) else {
        return Ok(DgVoodooPrepKind::None);
    };
    match dgvoodoo::assess_existing(target_dir, requirement) {
        dgvoodoo::ExistingDgVoodoo::Absent => Ok(DgVoodooPrepKind::Managed),
        dgvoodoo::ExistingDgVoodoo::CompatibleReusable
        | dgvoodoo::ExistingDgVoodoo::CompatibleAdoptable => Ok(DgVoodooPrepKind::Reused),
        dgvoodoo::ExistingDgVoodoo::Conflict(reason) => Err(errors::invalid(format!(
            "the existing dgVoodoo runtime is incompatible with Luma: {reason}"
        ))),
    }
}

pub(super) fn validate_preparation_kind(
    plan: &ResolvedLumaInstall,
    kind: DgVoodooPrepKind,
) -> Result<(), ServiceError> {
    let has_requirement = dgvoodoo::requirement(plan.external_requirement.as_ref()).is_some();
    if (!has_requirement && !matches!(kind, DgVoodooPrepKind::None))
        || (has_requirement && matches!(kind, DgVoodooPrepKind::None))
    {
        return Err(errors::state_changed_retry_install());
    }
    Ok(())
}
