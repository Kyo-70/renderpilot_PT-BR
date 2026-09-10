use renderpilot_domain::{
    AddonKind, FileOwnership, GameProxyTopology, InstalledAddon, ManagedAddonFile,
    ManagedFileBaseline, ManagedFileMode, PathRef, ProxyImplementation, normalized_path_key,
};

use crate::{
    addons::{
        luma::peer::active_update::error::LumaActiveUpdateError,
        reshade::{host_policy::TopologyHostAssessment, scan::ReshadeIdentity},
    },
    peer_mutation_executor::PeerPathSnapshot,
};

use super::super::model::LumaActiveUpdateHostInput;

pub(super) fn validate_persisted_host_claim<'a>(
    before: &'a InstalledAddon,
    topology: &GameProxyTopology,
    live_path: &PathRef,
) -> Result<&'a ManagedAddonFile, LumaActiveUpdateError> {
    if before.kind() != AddonKind::Luma {
        return Err(LumaActiveUpdateError::invalid_input(
            "active Luma update requires a Luma record",
        ));
    }
    if before.game_id() != &topology.game_id {
        return Err(LumaActiveUpdateError::invalid_input(
            "active Luma record and proxy topology belong to different games",
        ));
    }
    let Some(downstream) = topology.downstream.as_ref() else {
        return Err(LumaActiveUpdateError::invalid_input(
            "active Luma topology has no persisted ReShade downstream",
        ));
    };
    if downstream.implementation != ProxyImplementation::ReShade {
        return Err(LumaActiveUpdateError::invalid_input(
            "active Luma topology downstream is not ReShade",
        ));
    }
    if normalized_path_key(downstream.path.as_str()) != normalized_path_key(live_path.as_str()) {
        return Err(LumaActiveUpdateError::invalid_input(
            "active Luma host path differs from topology downstream",
        ));
    }

    let mut matches = before.managed_files().iter().filter(|managed| {
        normalized_path_key(managed.path().as_str()) == normalized_path_key(live_path.as_str())
    });
    let persisted = matches.next().ok_or_else(|| {
        LumaActiveUpdateError::invalid_input(
            "active Luma topology has no persisted managed host claim",
        )
    })?;
    if matches.next().is_some() {
        return Err(LumaActiveUpdateError::invalid_input(
            "active Luma topology has duplicate persisted managed host claims",
        ));
    }
    if before
        .created_files()
        .iter()
        .chain(before.backed_up_files())
        .any(|path| normalized_path_key(path.as_str()) == normalized_path_key(live_path.as_str()))
    {
        return Err(LumaActiveUpdateError::invalid_input(
            "active Luma host claim is also generic engine-owned",
        ));
    }
    if normalized_path_key(persisted.path().as_str())
        != normalized_path_key(downstream.path.as_str())
        || persisted.installed_sha256() != downstream.receipt.digest()
    {
        return Err(LumaActiveUpdateError::invalid_input(
            "active Luma managed host claim differs from topology receipt",
        ));
    }
    let expected_ownership = match persisted.mode() {
        ManagedFileMode::Owned => FileOwnership::Owned,
        ManagedFileMode::Reused => FileOwnership::Reused,
    };
    if downstream.receipt.ownership() != expected_ownership {
        return Err(LumaActiveUpdateError::invalid_input(
            "active Luma managed host ownership differs from topology receipt",
        ));
    }
    match (persisted.mode(), persisted.baseline()) {
        (ManagedFileMode::Reused, ManagedFileBaseline::Present { sha256 })
            if sha256 == persisted.installed_sha256() => {}
        (ManagedFileMode::Reused, _) => {
            return Err(LumaActiveUpdateError::invalid_input(
                "reused active Luma host requires a present baseline equal to installed bytes",
            ));
        }
        (ManagedFileMode::Owned, ManagedFileBaseline::Present { sha256 })
            if sha256 == persisted.installed_sha256() =>
        {
            return Err(LumaActiveUpdateError::invalid_input(
                "owned active Luma host installed bytes equal its baseline",
            ));
        }
        (ManagedFileMode::Owned, _) => {}
    }
    Ok(persisted)
}

pub(super) fn validate_live_receipt(
    persisted: &ManagedAddonFile,
    topology: &GameProxyTopology,
    path: &PathRef,
    snapshot: &PeerPathSnapshot,
) -> Result<(), LumaActiveUpdateError> {
    let Some(downstream) = topology.downstream.as_ref() else {
        return Err(LumaActiveUpdateError::invalid_input(
            "active Luma topology has no downstream receipt",
        ));
    };
    let file = snapshot.file().ok_or_else(|| {
        LumaActiveUpdateError::invalid_input(
            "active Luma topology downstream is missing from the retained live snapshot",
        )
    })?;
    if file.identity() != downstream.receipt.identity()
        || file.digest() != persisted.installed_sha256()
        || file.digest() != downstream.receipt.digest()
    {
        return Err(LumaActiveUpdateError::invalid_input_detail(format!(
            "active Luma host digest or identity differs from the persisted receipt at {path}"
        )));
    }
    if file.identity().trim().is_empty() {
        return Err(LumaActiveUpdateError::invalid_input(
            "active Luma host identity is empty",
        ));
    }
    Ok(())
}

pub(super) fn validate_owned_policy(
    assessment: &TopologyHostAssessment,
    input: &LumaActiveUpdateHostInput,
) -> Result<(), LumaActiveUpdateError> {
    if assessment.initial_is_conflict()
        || assessment
            .snapshot()
            .identity
            .is_none_or(|identity| identity < ReshadeIdentity::Probable)
    {
        return Err(LumaActiveUpdateError::invalid_input(
            "persisted owned ReShade host has conflicting or weak evidence",
        ));
    }
    match input {
        LumaActiveUpdateHostInput::Preserve => {
            if !matches!(
                assessment.snapshot().lifecycle,
                crate::addons::reshade::host_policy::HostLifecycle::ReuseUser
                    | crate::addons::reshade::host_policy::HostLifecycle::AdoptEmpty
            ) || assessment.snapshot().requires_host_download
                || assessment.assessment().initial_writes_host()
            {
                return Err(LumaActiveUpdateError::invalid_input(
                    "owned ReShade preserve requires a non-writing compatible assessment",
                ));
            }
        }
        LumaActiveUpdateHostInput::Replace { .. } => {
            if !matches!(
                assessment.snapshot().lifecycle,
                crate::addons::reshade::host_policy::HostLifecycle::AdoptEmpty
                    | crate::addons::reshade::host_policy::HostLifecycle::RepairEmpty
            ) {
                return Err(LumaActiveUpdateError::invalid_input(
                    "owned ReShade replacement requires an empty compatible assessment",
                ));
            }
        }
    }
    Ok(())
}
