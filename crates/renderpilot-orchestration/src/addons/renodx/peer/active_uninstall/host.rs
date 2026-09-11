use std::path::Path;

use renderpilot_domain::{
    FileOwnership, InstalledAddonHostKind, ManagedFileBaseline, ManagedFileMode,
    NormalizedPathRelation, PeerEndpointRole, PlannedGameProxyTopology, ProxyImplementation,
    ProxyRootPrestate, managed_sidecar_path, normalized_path_relation,
};

use super::effects::{
    ActiveUninstallEffects, emit_remove, emit_replace, find_endpoint, present_bytes, present_image,
};
use super::error::RenoDxActiveUninstallError;
use super::model::ActiveUninstallInput;

pub(super) fn compose_host(
    input: &ActiveUninstallInput<'_>,
    effects: &mut ActiveUninstallEffects,
) -> Result<PlannedGameProxyTopology, RenoDxActiveUninstallError> {
    let matching = input
        .record
        .managed_files()
        .iter()
        .filter(|file| {
            input.topology.downstream.as_ref().is_some_and(|link| {
                matches!(
                    normalized_path_relation(file.path().as_str(), link.path.as_str()),
                    NormalizedPathRelation::Equal
                )
            })
        })
        .collect::<Vec<_>>();
    let downstream = input.topology.downstream.as_ref();
    match input.record.host_kind() {
        Some(InstalledAddonHostKind::SharedVulkanLayer) => {
            if !matching.is_empty() || downstream.is_some() {
                return Err(RenoDxActiveUninstallError::Invalid(
                    "Vulkan RenoDX record has a proxy host claim",
                ));
            }
        }
        Some(InstalledAddonHostKind::Proxy) => {
            let downstream = downstream.ok_or(RenoDxActiveUninstallError::Invalid(
                "proxy RenoDX topology has no ReShade downstream",
            ))?;
            if downstream.implementation != ProxyImplementation::ReShade || matching.len() != 1 {
                return Err(RenoDxActiveUninstallError::Invalid(
                    "proxy host claim does not exactly match the ReShade downstream",
                ));
            }
            let host = matching[0];
            let host_endpoint = find_endpoint(input.endpoints, host.path())?;
            let host_image = present_image(host.path(), host_endpoint.snapshot())?;
            if host_image.digest() != host.installed_sha256() {
                return Err(RenoDxActiveUninstallError::Invalid(
                    "managed host bytes differ from the persisted installed image",
                ));
            }
            let expected_ownership = match host.mode() {
                ManagedFileMode::Owned => FileOwnership::Owned,
                ManagedFileMode::Reused => FileOwnership::Reused,
            };
            if downstream.receipt.digest() != host.installed_sha256()
                || downstream.receipt.ownership() != expected_ownership
            {
                return Err(RenoDxActiveUninstallError::Invalid(
                    "managed host claim differs from topology receipt",
                ));
            }
            let direct = input.root.canonical_game_root().join("ReShade64.dll");
            if !Path::new(host.path().as_str()).eq(&direct)
                && renderpilot_domain::normalized_path_key(host.path().as_str())
                    != renderpilot_domain::normalized_path_key(direct.to_string_lossy().as_ref())
            {
                return Err(RenoDxActiveUninstallError::Invalid(
                    "managed RenoDX host is not the direct ReShade64.dll",
                ));
            }
            match host.mode() {
                ManagedFileMode::Reused => {
                    if !matches!(host.baseline(), ManagedFileBaseline::Present { sha256 } if sha256 == host.installed_sha256())
                    {
                        return Err(RenoDxActiveUninstallError::Invalid(
                            "reused host claim has incoherent baseline",
                        ));
                    }
                    if host_endpoint
                        .backup()
                        .is_some_and(|backup| backup.snapshot().file().is_some())
                    {
                        return Err(RenoDxActiveUninstallError::Invalid(
                            "reused host claim has a sidecar",
                        ));
                    }
                }
                ManagedFileMode::Owned => match host.baseline() {
                    ManagedFileBaseline::Absent => {
                        let before_bytes = present_bytes(host.path(), host_endpoint.snapshot())?;
                        let before = present_image(host.path(), host_endpoint.snapshot())?;
                        emit_remove(
                            host.path(),
                            PeerEndpointRole::TopologyDownstream,
                            before,
                            before_bytes,
                            effects,
                        )?;
                        let mut after = input.topology.clone();
                        after.downstream = None;
                        after.downstream_origin = None;
                        after.root_prestate = ProxyRootPrestate::Absent;
                        return Ok(PlannedGameProxyTopology::Exact(after));
                    }
                    ManagedFileBaseline::Present { sha256 } => {
                        if sha256 == host.installed_sha256() {
                            return Err(RenoDxActiveUninstallError::Invalid(
                                "owned host baseline is equal to its installed image",
                            ));
                        }
                        let backup =
                            host_endpoint
                                .backup()
                                .ok_or(RenoDxActiveUninstallError::Invalid(
                                    "owned host replacement has no sealed sidecar",
                                ))?;
                        let restored = present_bytes(backup.path(), backup.snapshot())?;
                        let baseline = present_image(backup.path(), backup.snapshot())?;
                        if baseline.digest() != sha256 {
                            return Err(RenoDxActiveUninstallError::Invalid(
                                "host sidecar digest differs from persisted baseline",
                            ));
                        }
                        let before_bytes = present_bytes(host.path(), host_endpoint.snapshot())?;
                        let before = present_image(host.path(), host_endpoint.snapshot())?;
                        emit_replace(
                            host.path(),
                            PeerEndpointRole::TopologyDownstream,
                            before,
                            before_bytes,
                            &baseline,
                            restored,
                            effects,
                        )?;
                        let sidecar = managed_sidecar_path(host.path()).map_err(|_| {
                            RenoDxActiveUninstallError::Invalid("host sidecar path is invalid")
                        })?;
                        emit_remove(
                            &sidecar,
                            PeerEndpointRole::Disjoint,
                            present_image(backup.path(), backup.snapshot())?,
                            present_bytes(backup.path(), backup.snapshot())?,
                            effects,
                        )?;
                        let mut after = input.topology.clone();
                        after.downstream = None;
                        after.downstream_origin = None;
                        after.root_prestate = ProxyRootPrestate::Absent;
                        return Ok(PlannedGameProxyTopology::Exact(after));
                    }
                },
            }
        }
        None => {
            return Err(RenoDxActiveUninstallError::Invalid(
                "active RenoDX record has no host kind",
            ));
        }
    }
    Ok(PlannedGameProxyTopology::Exact(input.topology.clone()))
}
