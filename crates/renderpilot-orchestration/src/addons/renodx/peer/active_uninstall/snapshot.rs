use std::collections::BTreeSet;
use std::path::Path;

use renderpilot_domain::{
    AddonKind, GameProxyTopology, InstalledAddon, InstalledAddonHostKind, ManagedFileBaseline,
    NormalizedPathRelation, PathRef, ProxyImplementation, managed_sidecar_path,
    normalized_path_key, normalized_path_relation,
};

use crate::ServiceError;
use crate::addons::renodx::peer::RenoDxRootAuthority;
use crate::peer_mutation_executor::{PeerPathSnapshot, observe_peer_path_snapshot};

use super::model::{
    ActiveUninstallBackupOwned, ActiveUninstallEndpointOwned, ActiveUninstallInputOwned,
};

/// Seals the active uninstall input while the game mutation guard is held.
/// Every endpoint is observed no-follow exactly once and the root authority is
/// never resolved again by the composer or commit route.
pub(crate) fn snapshot_active_uninstall(
    record: &InstalledAddon,
    topology: &GameProxyTopology,
    authority: &RenoDxRootAuthority,
) -> Result<ActiveUninstallInputOwned, ServiceError> {
    if record.kind() != AddonKind::RenoDx {
        return Err(crate::failed(
            "RenoDX uninstall record has the wrong add-on kind",
        ));
    }
    if record.game_id() != &topology.game_id {
        return Err(crate::failed(
            "RenoDX uninstall record and topology belong to different games",
        ));
    }
    topology
        .validate()
        .map_err(|error| crate::failed(format!("invalid active RenoDX topology: {error}")))?;
    if topology.outer.implementation != ProxyImplementation::OptiScaler {
        return Err(crate::failed(
            "active RenoDX uninstall requires an OptiScaler outer topology",
        ));
    }
    let root = authority.seal().clone();
    if root.roots().require_game_path(&topology.root_slot).is_err() {
        return Err(crate::failed(
            "active RenoDX topology root is outside the sealed game root",
        ));
    }
    let ini_path = path_ref(root.exact_ini_path(), "ReShade.ini")?;
    let mut keys = BTreeSet::new();
    let mut endpoints = Vec::new();
    for path in record.created_files() {
        if matches!(
            normalized_path_relation(path.as_str(), ini_path.as_str()),
            NormalizedPathRelation::Equal
        ) {
            continue;
        }
        let key = normalized_path_key(path.as_str());
        if !keys.insert(key) {
            // `InstalledAddon::new` establishes the add-on payload claim and
            // callers may repeat that same canonical claim while assembling a
            // record. It is one physical endpoint, not a duplicate effect.
            if matches!(
                normalized_path_relation(path.as_str(), record.addon_file().as_str()),
                NormalizedPathRelation::Equal
            ) {
                continue;
            }
            return Err(crate::failed(format!(
                "RenoDX uninstall record contains duplicate created path: {}",
                path.as_str()
            )));
        }
        let snapshot = observe(path, &root)?;
        let backup = if record.backed_up_files().iter().any(|backed| {
            matches!(
                normalized_path_relation(backed.as_str(), path.as_str()),
                NormalizedPathRelation::Equal
            )
        }) {
            let sidecar = managed_sidecar_path(path).map_err(|error| {
                crate::failed(format!("cannot derive RenoDX uninstall sidecar: {error}"))
            })?;
            Some(ActiveUninstallBackupOwned::new(
                sidecar.clone(),
                observe(&sidecar, &root)?,
            ))
        } else {
            None
        };
        endpoints.push(ActiveUninstallEndpointOwned::new(
            path.clone(),
            snapshot,
            backup,
        ));
    }
    let mut backed_keys = BTreeSet::new();
    for path in record.backed_up_files() {
        if !backed_keys.insert(normalized_path_key(path.as_str())) {
            return Err(crate::failed(format!(
                "RenoDX uninstall record contains duplicate backed path: {}",
                path.as_str()
            )));
        }
        if !record.created_files().iter().any(|created| {
            matches!(
                normalized_path_relation(created.as_str(), path.as_str()),
                NormalizedPathRelation::Equal
            )
        }) {
            return Err(crate::failed(format!(
                "RenoDX uninstall backed claim has no live created claim: {}",
                path.as_str()
            )));
        }
    }

    if matches!(record.host_kind(), Some(InstalledAddonHostKind::Proxy)) {
        let Some(downstream) = topology.downstream.as_ref() else {
            return Err(crate::failed(
                "active RenoDX proxy uninstall has no topology downstream",
            ));
        };
        if downstream.implementation != ProxyImplementation::ReShade {
            return Err(crate::failed(
                "active RenoDX proxy uninstall downstream is not ReShade",
            ));
        }
        let host = record
            .managed_files()
            .iter()
            .filter(|file| {
                matches!(
                    normalized_path_relation(file.path().as_str(), downstream.path.as_str()),
                    NormalizedPathRelation::Equal
                )
            })
            .collect::<Vec<_>>();
        if host.len() != 1 {
            return Err(crate::failed(
                "active RenoDX proxy uninstall requires one managed host claim",
            ));
        }
        if !endpoints.iter().any(|endpoint| {
            matches!(
                normalized_path_relation(endpoint.path().as_str(), downstream.path.as_str()),
                NormalizedPathRelation::Equal
            )
        }) {
            let snapshot = observe(&downstream.path, &root)?;
            let backup = match host[0].baseline() {
                ManagedFileBaseline::Absent => None,
                ManagedFileBaseline::Present { .. } => {
                    let sidecar = managed_sidecar_path(&downstream.path).map_err(|error| {
                        crate::failed(format!("cannot derive RenoDX host sidecar: {error}"))
                    })?;
                    Some(ActiveUninstallBackupOwned::new(
                        sidecar.clone(),
                        observe(&sidecar, &root)?,
                    ))
                }
            };
            endpoints.push(ActiveUninstallEndpointOwned::new(
                downstream.path.clone(),
                snapshot,
                backup,
            ));
        }
    } else if record.host_kind() == Some(InstalledAddonHostKind::SharedVulkanLayer)
        && topology.downstream.is_some()
    {
        return Err(crate::failed(
            "active RenoDX Vulkan uninstall has a proxy downstream",
        ));
    }

    // The exact config may be unclaimed by the record, but it is still a
    // typed endpoint candidate and must be sealed from the same root.
    let ini_snapshot = observe(&ini_path, &root)?;
    validate_sealed_ini(&root, &ini_snapshot)?;
    Ok(ActiveUninstallInputOwned::new(
        record.clone(),
        topology.clone(),
        root,
        endpoints,
        ini_snapshot,
    ))
}

fn observe(
    path: &PathRef,
    root: &crate::addons::renodx::peer::RenoDxRootSeal,
) -> Result<PeerPathSnapshot, ServiceError> {
    let authorized = if root.roots().require_game_path(path).is_ok() {
        root.canonical_game_root_ref()
    } else if root.roots().require_sealed_path(path).is_ok() {
        root.payload_root_ref().ok_or_else(|| {
            crate::failed(format!(
                "RenoDX path is outside sealed roots: {}",
                path.as_str()
            ))
        })?
    } else {
        return Err(crate::failed(format!(
            "RenoDX uninstall path is outside sealed roots: {}",
            path.as_str()
        )));
    };
    observe_peer_path_snapshot(path, authorized)
}

fn path_ref(path: &Path, label: &str) -> Result<PathRef, ServiceError> {
    let value = path
        .to_str()
        .ok_or_else(|| crate::failed(format!("RenoDX {label} is not valid UTF-8")))?;
    PathRef::new(value.to_owned())
        .map_err(|error| crate::failed(format!("RenoDX {label} is invalid: {error}")))
}

fn validate_sealed_ini(
    root: &crate::addons::renodx::peer::RenoDxRootSeal,
    snapshot: &PeerPathSnapshot,
) -> Result<(), ServiceError> {
    match (root.config_source(), snapshot) {
        (
            crate::addons::renodx::peer::RenoDxConfigSourceSeal::Absent { .. },
            PeerPathSnapshot::Absent,
        ) => Ok(()),
        (
            crate::addons::renodx::peer::RenoDxConfigSourceSeal::File {
                identity,
                digest,
                length,
                owned_bytes,
                ..
            },
            PeerPathSnapshot::File(_),
        ) if snapshot.file().is_some_and(|value| {
            value.identity() == identity && value.digest() == digest && value.length() == *length
        }) && snapshot.bytes() == Some(owned_bytes.as_slice()) =>
        {
            Ok(())
        }
        _ => Err(crate::failed(
            "ReShade.ini changed between root resolution and endpoint snapshot",
        )),
    }
}
