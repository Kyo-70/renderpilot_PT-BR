use std::path::Path;

use renderpilot_domain::{
    PathRef, PeerEndpointEvidence, PeerEndpointIntent, PeerEndpointOperation, PeerEndpointRole,
    PeerFileImage, RenoDxReshadeIniAuthority, Sha256Hash, normalized_path_key,
};
use serde_json::{Value, json};

use super::super::manifest::{FileAfter, FileBefore};
use super::{MutationError, MutationPlan, UnchangedDownstreamSnapshots};

#[derive(Debug, Clone)]
pub(in crate::addons::shared_vulkan_mutation) struct SharedEndpointSnapshot {
    pub(in crate::addons::shared_vulkan_mutation) path: PathRef,
    pub(in crate::addons::shared_vulkan_mutation) role: PeerEndpointRole,
    pub(in crate::addons::shared_vulkan_mutation) image: Option<PeerFileImage>,
}

pub(in crate::addons::shared_vulkan_mutation) fn capture_shared_endpoint_snapshots(
    plan: &MutationPlan,
    excluded_downstream: &[String],
    renodx_reshade_ini: Option<&RenoDxReshadeIniAuthority>,
) -> Result<Vec<SharedEndpointSnapshot>, MutationError> {
    let mut typed_count = 0usize;
    let snapshots = plan
        .manifest
        .files
        .iter()
        .filter(|file| is_changed_file(file))
        .map(|file| {
            let path = plan.roots.resolve(&file.live_path)?;
            let path_ref = PathRef::new(path.to_string_lossy().into_owned())
                .map_err(|error| MutationError::conflict(error.to_string()))?;
            if excluded_downstream
                .iter()
                .any(|excluded| *excluded == normalized_path_key(path_ref.as_str()))
            {
                return Err(MutationError::conflict(format!(
                    "shared peer endpoint overlaps topology downstream: {}",
                    path_ref.as_str()
                )));
            }
            let role = if let Some(authority) = renodx_reshade_ini {
                if authority.matches_ini_path(&path_ref) {
                    typed_count += 1;
                    if file.live_path.root_id() != "game-0"
                        || !file
                            .live_path
                            .relative()
                            .eq_ignore_ascii_case("ReShade.ini")
                    {
                        return Err(MutationError::conflict(
                            "RenoDX ReShade.ini endpoint must be retained under game-0",
                        ));
                    }
                    PeerEndpointRole::RenoDxReshadeIni
                } else {
                    PeerEndpointRole::Disjoint
                }
            } else {
                PeerEndpointRole::Disjoint
            };
            let image = observe_shared_endpoint(&path)?;
            Ok(SharedEndpointSnapshot {
                path: path_ref,
                role,
                image,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    if renodx_reshade_ini.is_some() && typed_count != 1 {
        return Err(MutationError::conflict(format!(
            "RenoDX ReShade.ini authority requires exactly one changed endpoint, found {typed_count}"
        )));
    }
    Ok(snapshots)
}

pub(in crate::addons::shared_vulkan_mutation) fn build_shared_peer_program(
    plan: &MutationPlan,
    before: &[SharedEndpointSnapshot],
    mutation_id: &str,
) -> Result<Value, MutationError> {
    let changed = plan
        .manifest
        .files
        .iter()
        .filter(|file| is_changed_file(file))
        .collect::<Vec<_>>();
    if changed.len() != before.len() {
        return Err(MutationError::conflict(
            "shared peer program participants changed while preparing",
        ));
    }
    let mut stage = Vec::new();
    let mut custody = Vec::new();
    let mut created_ancestors = Vec::new();
    let mut endpoints = Vec::with_capacity(before.len());
    for (file, observed) in changed.into_iter().zip(before) {
        let path = observed.path.as_str();
        let path_token = capability_token(&file.live_path);
        if let Some(stage_path) = file.stage_path.as_ref() {
            stage.push(capability_token(stage_path));
            custody.push(capability_token(stage_path));
        }
        if let Some(tomb_path) = file.tomb_path.as_ref() {
            custody.push(capability_token(tomb_path));
        }
        created_ancestors.push(path_token.clone());
        let (operation, planned_sha256, planned_length) = match &file.after {
            FileAfter::Present { sha256, len } => {
                let operation = if observed.image.is_some() {
                    PeerEndpointOperation::Replace
                } else {
                    PeerEndpointOperation::Create
                };
                (operation, Some(sha256.clone()), Some(*len))
            }
            FileAfter::Absent => {
                if observed.image.is_none() {
                    return Err(MutationError::conflict(format!(
                        "shared peer endpoint is absent before removal: {path}"
                    )));
                }
                (PeerEndpointOperation::Remove, None, None)
            }
        };
        let before_json = observed.image.as_ref().map(image_json);
        endpoints.push(json!({
            "ordinal": endpoints.len(),
            "path": path,
            "role": role_name(observed.role),
            "operation": operation_name(operation),
            "planned_sha256": planned_sha256,
            "planned_length": planned_length,
            "before": before_json,
            "read_guards": [path_token],
            "subtree_publishes": [],
        }));
    }
    Ok(json!({
        "format": 1,
        "transaction_owner": mutation_id,
        "execution_class": "shared",
        "roots": plan.roots.root_ids(),
        "stage": stage,
        "custody": custody,
        "created_ancestors": created_ancestors,
        "endpoints": endpoints,
    }))
}

pub(super) fn excluded_downstream_paths(peer: UnchangedDownstreamSnapshots<'_>) -> Vec<String> {
    [peer.before_topology, peer.after_topology]
        .into_iter()
        .flatten()
        .filter_map(|topology| topology.downstream.as_ref())
        .map(|downstream| normalized_path_key(downstream.path.as_str()))
        .collect()
}

pub(in crate::addons::shared_vulkan_mutation) fn build_shared_peer_evidence(
    before: &[SharedEndpointSnapshot],
    after: &[SharedEndpointSnapshot],
) -> Result<Vec<PeerEndpointEvidence>, MutationError> {
    if before.len() != after.len() {
        return Err(MutationError::conflict(
            "shared peer endpoint set changed during publication",
        ));
    }
    let mut evidence = Vec::with_capacity(before.len());
    for (before, after) in before.iter().zip(after.iter()) {
        if before.path != after.path {
            return Err(MutationError::conflict(
                "shared peer endpoint order changed during publication",
            ));
        }
        if before.role != after.role {
            return Err(MutationError::conflict(
                "shared peer endpoint role changed during publication",
            ));
        }
        let operation = match (&before.image, &after.image) {
            (None, Some(_)) => PeerEndpointOperation::Create,
            (Some(_), Some(_)) => PeerEndpointOperation::Replace,
            (Some(_), None) => PeerEndpointOperation::Remove,
            (None, None) => {
                return Err(MutationError::conflict(format!(
                    "shared peer endpoint is a no-op: {}",
                    before.path.as_str()
                )));
            }
        };
        let planned_digest = after.image.as_ref().map(|image| image.sha256().clone());
        let planned_length = after.image.as_ref().map(PeerFileImage::length);
        let intent = PeerEndpointIntent::new(
            before.path.clone(),
            before.role,
            operation,
            planned_digest,
            planned_length,
        )
        .map_err(|error| MutationError::conflict(error.to_string()))?;
        evidence.push(PeerEndpointEvidence::new(
            intent,
            before.image.clone(),
            after.image.clone(),
        ));
    }
    Ok(evidence)
}

fn role_name(role: PeerEndpointRole) -> &'static str {
    match role {
        PeerEndpointRole::Disjoint => "disjoint",
        PeerEndpointRole::TopologyDownstream => "topology_downstream",
        PeerEndpointRole::RenoDxReshadeIni => "renodx_reshade_ini",
        PeerEndpointRole::OptiScalerConfig => "optiscaler_config",
        PeerEndpointRole::DlssFix => "dlss_fix",
    }
}

fn capability_token(path: &super::super::capability::CapabilityPath) -> String {
    format!("{}:{}", path.root_id(), path.relative())
}

fn image_json(image: &PeerFileImage) -> Value {
    json!({
        "identity": image.identity(),
        "sha256": image.sha256().as_str(),
        "length": image.length(),
    })
}

fn operation_name(operation: PeerEndpointOperation) -> &'static str {
    match operation {
        PeerEndpointOperation::Create => "create",
        PeerEndpointOperation::Replace => "replace",
        PeerEndpointOperation::Remove => "remove",
    }
}

fn is_changed_file(file: &super::super::manifest::FileParticipant) -> bool {
    match (&file.before, &file.after) {
        (FileBefore::Absent, FileAfter::Absent) => false,
        (
            FileBefore::Snapshot {
                sha256: before_digest,
                len: before_length,
                ..
            },
            FileAfter::Present {
                sha256: after_digest,
                len: after_length,
            },
        ) => before_digest != after_digest || before_length != after_length,
        _ => true,
    }
}

fn observe_shared_endpoint(path: &Path) -> Result<Option<PeerFileImage>, MutationError> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(MutationError::io(error)),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(MutationError::conflict(format!(
            "shared peer endpoint is not a regular file: {}",
            path.display()
        )));
    }
    let (parent, leaf) = crate::fs::verified_parent(path)
        .map_err(|error| MutationError::conflict(error.to_string()))?;
    let observation = parent
        .observe_leaf(&leaf)
        .map_err(|error| MutationError::conflict(error.to_string()))?
        .ok_or_else(|| {
            MutationError::conflict(format!(
                "shared peer endpoint disappeared while observing: {}",
                path.display()
            ))
        })?;
    if observation.kind != crate::fs::EntryKind::File {
        return Err(MutationError::conflict(format!(
            "shared peer endpoint is not a regular file: {}",
            path.display()
        )));
    }
    let (bytes, observation) = parent
        .read_regular_file(&leaf, Some(&observation))
        .map_err(|error| MutationError::conflict(error.to_string()))?;
    let digest = observation
        .digest
        .ok_or_else(|| MutationError::conflict("shared peer endpoint has no digest"))?;
    let digest =
        Sha256Hash::new(digest).map_err(|error| MutationError::conflict(error.to_string()))?;
    PeerFileImage::new(observation.identity, digest, bytes.len() as u64)
        .map(Some)
        .map_err(|error| MutationError::conflict(error.to_string()))
}
