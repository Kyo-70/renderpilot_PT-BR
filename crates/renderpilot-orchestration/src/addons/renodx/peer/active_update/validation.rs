use std::path::Path;

use renderpilot_domain::{GameProxyTopology, InstalledAddon, ProxyImplementation};
use sha2::{Digest, Sha256};

use crate::peer_mutation_executor::{PeerPathSnapshot, VerifiedPeerFile};

use super::error::RenoDxActiveUpdateError;
use super::model::{
    RenoDxActiveUpdateConfigInput, RenoDxActiveUpdateHostInput, RenoDxActiveUpdateInput,
};
use super::{paths, record};

pub(super) struct ValidatedUpdate<'a> {
    pub(super) before: &'a InstalledAddon,
    pub(super) after: &'a InstalledAddon,
    pub(super) topology: &'a GameProxyTopology,
    pub(super) addon_path: renderpilot_domain::PathRef,
    pub(super) addon_before: &'a VerifiedPeerFile,
    pub(super) addon_before_bytes: &'a [u8],
    pub(super) addon_bytes: Vec<u8>,
    pub(super) host: Option<ValidatedHost>,
    pub(super) config: Option<RenoDxActiveUpdateConfigInput>,
    pub(super) has_physical_change: bool,
}

pub(super) struct ValidatedHost {
    pub(super) path: renderpilot_domain::PathRef,
    pub(super) before: VerifiedPeerFile,
    pub(super) before_bytes: Vec<u8>,
    pub(super) bytes: Vec<u8>,
}

pub(super) fn validate_input<'a>(
    input: RenoDxActiveUpdateInput<'a>,
) -> Result<ValidatedUpdate<'a>, RenoDxActiveUpdateError> {
    let before = input.before_peer;
    let after = input.after_peer;
    let topology = input.topology;
    let canonical_game_root = input.canonical_game_root;
    let payload_root = input.payload_root;
    let addon_path = input.addon_path;
    let addon_snapshot = input.addon_snapshot;
    let addon_bytes = input.addon_bytes;
    let host = input.host;
    let config = input.config;

    record::validate_records(before, after, topology)?;
    topology
        .validate()
        .map_err(|_| RenoDxActiveUpdateError::InvalidInput("active topology is invalid"))?;
    if topology.outer.implementation != ProxyImplementation::OptiScaler {
        return Err(RenoDxActiveUpdateError::InvalidInput(
            "active update requires an OptiScaler outer topology",
        ));
    }
    validate_topology_paths(topology, canonical_game_root)?;
    record::validate_record_paths(before, canonical_game_root, payload_root)?;
    record::validate_record_paths(after, canonical_game_root, payload_root)?;
    record::validate_addon_claims(before, after, &addon_path)?;
    paths::require_under_roots(&addon_path, canonical_game_root, payload_root)?;

    let (addon_before, addon_before_bytes) = retained_file(&addon_path, addon_snapshot)?;
    ensure_bytes_match(&addon_path, addon_before_bytes, addon_before)?;
    let addon_changed = !same_image(addon_before, &addon_bytes)?;

    let validated_host = host
        .map(|host| validate_host(host, before, after, topology, canonical_game_root))
        .transpose()?;
    let host_changed = validated_host
        .as_ref()
        .map(|host| same_image(&host.before, &host.bytes))
        .transpose()?
        .is_some_and(|same| !same);
    let config_changed = validate_config(config.as_ref(), canonical_game_root)?;

    if let Some(host) = validated_host.as_ref() {
        record::ensure_non_host_claims_unchanged(before, after, &host.path)?;
    } else {
        record::ensure_no_physical_claim_changes(before, after)?;
    }

    Ok(ValidatedUpdate {
        before,
        after,
        topology,
        addon_path,
        addon_before,
        addon_before_bytes,
        addon_bytes,
        host: validated_host,
        config,
        has_physical_change: addon_changed || host_changed || config_changed,
    })
}

fn validate_config(
    config: Option<&RenoDxActiveUpdateConfigInput>,
    canonical_game_root: &Path,
) -> Result<bool, RenoDxActiveUpdateError> {
    let Some(config) = config else {
        return Ok(false);
    };
    let (path, before, before_bytes, after) = config.parts();
    paths::require_under_roots(path, canonical_game_root, None)?;
    if path
        .parent()
        .is_none_or(|parent| !crate::paths::same_path(Path::new(parent), canonical_game_root))
        || !path
            .file_name()
            .is_some_and(|name| name.eq_ignore_ascii_case("ReShade.ini"))
    {
        return Err(RenoDxActiveUpdateError::InvalidPath(path.clone()));
    }
    if before.is_some() != before_bytes.is_some() {
        return Err(RenoDxActiveUpdateError::InvalidInput(
            "active config preimage bytes and metadata are not paired",
        ));
    }
    if let (Some(before), Some(before_bytes)) = (before, before_bytes) {
        ensure_bytes_match(path, before_bytes, before)?;
    }
    if after.is_none() && before.is_some() {
        return Err(RenoDxActiveUpdateError::InvalidInput(
            "active Set_Path update cannot remove ReShade.ini",
        ));
    }
    Ok(before_bytes != after)
}

fn validate_topology_paths(
    topology: &GameProxyTopology,
    canonical_game_root: &Path,
) -> Result<(), RenoDxActiveUpdateError> {
    for path in topology.participant_paths() {
        paths::require_under_roots(path, canonical_game_root, None)?;
    }
    Ok(())
}

fn validate_host(
    host: RenoDxActiveUpdateHostInput<'_>,
    before: &InstalledAddon,
    after: &InstalledAddon,
    topology: &GameProxyTopology,
    canonical_game_root: &Path,
) -> Result<ValidatedHost, RenoDxActiveUpdateError> {
    let (path, snapshot, bytes) = host.into_parts();
    paths::require_under_roots(&path, canonical_game_root, None)?;
    if !path
        .file_name()
        .is_some_and(|name| name.eq_ignore_ascii_case("ReShade64.dll"))
        || path
            .parent()
            .is_none_or(|parent| !crate::paths::same_path(Path::new(parent), canonical_game_root))
    {
        return Err(RenoDxActiveUpdateError::InvalidPath(path));
    }
    let Some(downstream) = topology.downstream.as_ref() else {
        return Err(RenoDxActiveUpdateError::InvalidInput(
            "active host update cannot create a topology downstream",
        ));
    };
    if downstream.implementation != ProxyImplementation::ReShade
        || !paths::same_path(&downstream.path, &path)
    {
        return Err(RenoDxActiveUpdateError::InvalidInput(
            "active host update cannot relocate the topology downstream",
        ));
    }
    let before_claim = record::owned_host_claim(before, &path)?;
    let after_claim = record::owned_host_claim(after, &path)?;
    let (before_file, before_bytes) = retained_file(&path, snapshot)?;
    ensure_bytes_match(&path, before_bytes, before_file)?;
    if before_claim.installed_sha256() != before_file.digest()
        || after_claim.baseline() != before_claim.baseline()
    {
        return Err(RenoDxActiveUpdateError::InvalidRecord(
            "host record claim does not match its sealed preimage",
        ));
    }
    if bytes.is_empty() {
        return Err(RenoDxActiveUpdateError::InvalidInput(
            "prepared ReShade host bytes are empty",
        ));
    }
    let prepared_digest = digest(&bytes)?;
    if after_claim.installed_sha256() != &prepared_digest {
        return Err(RenoDxActiveUpdateError::InvalidRecord(
            "after host claim does not match prepared ReShade bytes",
        ));
    }
    Ok(ValidatedHost {
        path,
        before: before_file.clone(),
        before_bytes: before_bytes.to_vec(),
        bytes,
    })
}

fn retained_file<'a>(
    path: &renderpilot_domain::PathRef,
    snapshot: &'a PeerPathSnapshot,
) -> Result<(&'a VerifiedPeerFile, &'a [u8]), RenoDxActiveUpdateError> {
    let file = snapshot
        .file()
        .ok_or_else(|| RenoDxActiveUpdateError::BeforeImageMismatch(path.clone()))?;
    let bytes = snapshot
        .bytes()
        .ok_or(RenoDxActiveUpdateError::InvalidInput(
            "present update preimage has no retained bytes",
        ))?;
    Ok((file, bytes))
}

fn ensure_bytes_match(
    path: &renderpilot_domain::PathRef,
    bytes: &[u8],
    image: &VerifiedPeerFile,
) -> Result<(), RenoDxActiveUpdateError> {
    if image.length() != bytes.len() as u64 || image.digest() != &digest(bytes)? {
        return Err(RenoDxActiveUpdateError::BeforeImageMismatch(path.clone()));
    }
    Ok(())
}

pub(super) fn same_image(
    image: &VerifiedPeerFile,
    bytes: &[u8],
) -> Result<bool, RenoDxActiveUpdateError> {
    Ok(image.length() == bytes.len() as u64 && image.digest() == &digest(bytes)?)
}

fn digest(bytes: &[u8]) -> Result<renderpilot_domain::Sha256Hash, RenoDxActiveUpdateError> {
    renderpilot_domain::Sha256Hash::new(hex::encode(Sha256::digest(bytes)))
        .map_err(|_| RenoDxActiveUpdateError::InvalidInput("endpoint digest could not be computed"))
}
