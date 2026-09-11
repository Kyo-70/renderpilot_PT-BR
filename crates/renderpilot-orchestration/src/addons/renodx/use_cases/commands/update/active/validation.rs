//! Fail-closed local invariants for the active RenoDX route.

use std::path::Path;

use renderpilot_domain::{
    FileOwnership, GameProxyTopology, InstalledAddon, InstalledAddonHostKind, PathRef,
    ProxyImplementation,
};

use crate::ServiceError;
use crate::addons::renodx::errors;
use crate::addons::renodx::peer::RenoDxRootAuthority;
use crate::addons::renodx::use_cases::commands::update::snapshot::UpdateSnapshot;
use crate::peer_mutation_executor::PeerPathSnapshot;

use crate::addons::reshade::proxy::HostKind;

pub(super) fn explicit_host_kind(record: &InstalledAddon) -> Result<HostKind, ServiceError> {
    match record.host_kind() {
        Some(InstalledAddonHostKind::Proxy) => Ok(HostKind::Proxy),
        Some(InstalledAddonHostKind::SharedVulkanLayer) => Ok(HostKind::Vulkan),
        None => Err(invalid(
            "active RenoDX update requires an explicit host kind",
        )),
    }
}

pub(super) fn topology(
    topology: &GameProxyTopology,
    game_id: &renderpilot_domain::GameId,
    game_root: &Path,
) -> Result<(), ServiceError> {
    topology
        .validate()
        .map_err(|error| invalid(format!("active RenoDX proxy topology is invalid: {error}")))?;
    if topology.game_id != *game_id
        || topology.outer.implementation != ProxyImplementation::OptiScaler
    {
        return Err(invalid(
            "active RenoDX topology must belong to this OptiScaler game",
        ));
    }
    for path in topology.participant_paths() {
        game_descendant(path, game_root)?;
    }
    Ok(())
}

pub(super) fn record_paths(
    record: &InstalledAddon,
    authority: &RenoDxRootAuthority,
) -> Result<(), ServiceError> {
    for path in record
        .created_files()
        .iter()
        .chain(record.backed_up_files())
        .chain(record.managed_files().iter().map(|file| file.path()))
    {
        authority.authorized_root(path).map_err(|_| {
            invalid(format!(
                "active RenoDX record path is outside sealed roots: {path}"
            ))
        })?;
    }
    Ok(())
}

pub(super) fn proxy_host(
    record: &InstalledAddon,
    topology: &GameProxyTopology,
    host_path: &PathRef,
    game_root: &Path,
) -> Result<(), ServiceError> {
    let Some(downstream) = topology.downstream.as_ref() else {
        return Err(invalid(
            "active RenoDX proxy topology has no ReShade downstream",
        ));
    };
    if downstream.implementation != ProxyImplementation::ReShade
        || !crate::paths::same_path(
            Path::new(downstream.path.as_str()),
            Path::new(host_path.as_str()),
        )
    {
        return Err(invalid(
            "active RenoDX proxy topology downstream is not the sealed ReShade64.dll host",
        ));
    }
    let expected = game_root.join("ReShade64.dll");
    if !crate::paths::same_path(Path::new(host_path.as_str()), &expected) {
        return Err(invalid(
            "active RenoDX ReShade host is not the direct game-root ReShade64.dll",
        ));
    }
    let claims = record.managed_files();
    if claims.len() != 1
        || !crate::paths::same_path(
            Path::new(claims[0].path().as_str()),
            Path::new(host_path.as_str()),
        )
    {
        return Err(invalid(
            "active RenoDX proxy record must contain exactly one matching managed host claim",
        ));
    }
    if !matches!(
        claims[0].mode(),
        renderpilot_domain::ManagedFileMode::Owned | renderpilot_domain::ManagedFileMode::Reused
    ) {
        return Err(invalid(
            "active RenoDX managed host claim has an unknown mode",
        ));
    }
    Ok(())
}

pub(super) fn host_snapshot(
    topology: &GameProxyTopology,
    record: &InstalledAddon,
    snapshot: &PeerPathSnapshot,
) -> Result<(), ServiceError> {
    let Some(file) = snapshot.file() else {
        return Err(invalid("active RenoDX ReShade64.dll host is absent"));
    };
    let downstream = topology
        .downstream
        .as_ref()
        .ok_or_else(|| invalid("active RenoDX proxy topology has no ReShade downstream"))?;
    if downstream.receipt.identity() != file.identity()
        || downstream.receipt.digest() != file.digest()
    {
        return Err(errors::state_changed_retry_update());
    }
    let claim = &record.managed_files()[0];
    if claim.installed_sha256() != file.digest() {
        return Err(invalid(
            "active RenoDX managed host claim does not match the retained host image",
        ));
    }
    if !matches!(
        downstream.receipt.ownership(),
        FileOwnership::Owned | FileOwnership::Reused
    ) {
        return Err(invalid(
            "active RenoDX downstream receipt has invalid ownership",
        ));
    }
    Ok(())
}

pub(super) fn vulkan_record(
    record: &InstalledAddon,
    base: &UpdateSnapshot,
    registered_exe: Option<&Path>,
) -> Result<(), ServiceError> {
    if !record.managed_files().is_empty() || base.host().is_some() || base.host_target().is_some() {
        return Err(invalid(
            "active Vulkan RenoDX record contains proxy host evidence",
        ));
    }
    if record
        .created_files()
        .iter()
        .chain(record.backed_up_files())
        .any(|path| {
            path.file_name()
                .is_some_and(|name| name.eq_ignore_ascii_case("ReShade64.dll"))
        })
    {
        return Err(invalid(
            "active Vulkan RenoDX record contains a proxy host path",
        ));
    }
    let registered_exe = registered_exe.ok_or_else(|| {
        invalid("active Vulkan RenoDX record has no sealed registered executable")
    })?;
    if record
        .registered_exe_path()
        .is_none_or(|path| !crate::paths::same_path(Path::new(path.as_str()), registered_exe))
    {
        return Err(invalid(
            "active Vulkan RenoDX registered executable drifted from the sealed path",
        ));
    }
    Ok(())
}

fn game_descendant(path: &PathRef, root: &Path) -> Result<(), ServiceError> {
    if path
        .as_str()
        .split(['/', '\\'])
        .any(|component| component == "..")
        || !crate::paths::is_within(Path::new(path.as_str()), root)
    {
        return Err(invalid(format!(
            "active RenoDX topology path is outside the canonical game root: {path}"
        )));
    }
    Ok(())
}

fn invalid(message: impl Into<String>) -> ServiceError {
    errors::invalid(message.into())
}
