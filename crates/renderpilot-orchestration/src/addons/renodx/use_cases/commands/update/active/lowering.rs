//! Pure lowering of prepared update artifacts into the active peer composer.

use std::path::Path;

use renderpilot_domain::{ManagedAddonFile, ManagedFileMode, TrackedSourceRole};
use sha2::{Digest, Sha256};

use crate::ServiceError;
use crate::addons::renodx::errors;
use crate::addons::renodx::peer::{
    RenoDxActiveUpdateComposition, RenoDxActiveUpdateHostInput, RenoDxActiveUpdateInput,
    compose_active_update,
};
use crate::addons::tracking;
use crate::addons::tracking::{
    AddonVersionUpdate, ManagedFilesUpdate, PreserveMetadata, RebuildParts,
};

use super::super::prepare::PreparedUpdateArtifacts;
use super::snapshot::ActiveUpdatePhase1;

/// Immutable result passed to the later active commit route.
#[derive(Debug)]
pub(crate) struct ActiveLoweredUpdate {
    pub(super) composition: RenoDxActiveUpdateComposition,
    pub(super) addon_mtime: Option<String>,
}

pub(crate) fn lower_active_update(
    phase1: &ActiveUpdatePhase1,
    phase3: &ActiveUpdatePhase1,
    prepared: &PreparedUpdateArtifacts,
) -> Result<ActiveLoweredUpdate, ServiceError> {
    phase1.ensure_phase3_matches(phase3)?;
    if prepared.host_install.is_some() {
        return Err(invalid(
            "active RenoDX update cannot relocate or create a ReShade host",
        ));
    }

    let addon_bytes = retained_bytes(&phase3.addon_snapshot, &phase3.addon_path)?;
    let mut addon_bytes = addon_bytes.to_vec();
    let mut host_bytes = None;
    let mut addon_mtime = None;
    let mut addon_seen = false;
    let mut host_seen = false;

    for replacement in &prepared.replacements {
        if same_path(&replacement.path, &phase3.addon_path) {
            if addon_seen {
                return Err(invalid(
                    "active RenoDX update contains duplicate add-on replacements",
                ));
            }
            addon_seen = true;
            addon_mtime.clone_from(&replacement.mtime);
            addon_bytes.clone_from(&replacement.bytes);
        } else if phase3
            .host_path
            .as_ref()
            .is_some_and(|path| same_path(&replacement.path, path))
        {
            if host_seen {
                return Err(invalid(
                    "active RenoDX update contains duplicate host replacements",
                ));
            }
            host_seen = true;
            host_bytes = Some(replacement.bytes.clone());
        } else {
            return Err(invalid(format!(
                "active RenoDX update contains an unknown replacement path: {}",
                replacement.path.display()
            )));
        }
    }

    let after = rebuild_record(phase3, prepared, host_bytes.as_deref())?;
    validate_source_digests(
        &after,
        &addon_bytes,
        phase3.host_path.as_ref(),
        host_bytes.as_deref(),
        phase3,
    )?;

    let host = match (
        host_bytes,
        phase3.host_path.as_ref(),
        phase3.host_snapshot.as_ref(),
    ) {
        (Some(bytes), Some(path), Some(snapshot)) => Some(RenoDxActiveUpdateHostInput::new(
            path.clone(),
            snapshot,
            bytes,
        )),
        (Some(_), _, _) => {
            return Err(invalid("active RenoDX host replacement has no sealed host"));
        }
        (None, _, _) => None,
    };
    let input = RenoDxActiveUpdateInput {
        before_peer: phase3.base.record(),
        after_peer: &after,
        topology: &phase3.topology,
        canonical_game_root: phase3.root_seal.canonical_game_root(),
        payload_root: phase3.root_seal.payload_root(),
        addon_path: phase3.addon_path.clone(),
        addon_snapshot: &phase3.addon_snapshot,
        addon_bytes,
        host,
    };
    let composition = compose_active_update(&input)
        .map_err(|error| invalid(format!("active RenoDX update composition failed: {error}")))?;
    Ok(ActiveLoweredUpdate {
        composition,
        addon_mtime,
    })
}

fn rebuild_record(
    phase3: &ActiveUpdatePhase1,
    prepared: &PreparedUpdateArtifacts,
    host_bytes: Option<&[u8]>,
) -> Result<renderpilot_domain::InstalledAddon, ServiceError> {
    let record = phase3.base.record();
    let managed = match (host_bytes, phase3.host_path.as_ref()) {
        (None, _) => record.managed_files().to_vec(),
        (Some(bytes), Some(host_path)) => {
            let index = record
                .managed_files()
                .iter()
                .position(|file| same_path(Path::new(file.path().as_str()), host_path))
                .ok_or_else(|| invalid("active RenoDX host claim disappeared during lowering"))?;
            if record.managed_files()[index].mode() != ManagedFileMode::Owned {
                return Err(invalid(
                    "active RenoDX cannot replace a Reused ReShade host",
                ));
            }
            let mut managed = record.managed_files().to_vec();
            managed[index] = ManagedAddonFile::owned(
                host_path.clone(),
                record.managed_files()[index].baseline().clone(),
                digest(bytes)?,
            );
            managed
        }
        (Some(_), None) => return Err(invalid("active RenoDX host replacement has no host path")),
    };
    tracking::rebuild_install_record(
        record,
        RebuildParts {
            addon_file: record.addon_file().clone(),
            addon_version: AddonVersionUpdate::Keep,
            managed_files: ManagedFilesUpdate::Replace(managed),
            created_files: record.created_files().to_vec(),
            backed_up_files: record.backed_up_files().to_vec(),
            tracked_sources: prepared.refreshed_sources.clone(),
            label: "RenoDX active update rebuild".to_owned(),
        },
        PreserveMetadata::renodx(),
    )
}

fn validate_source_digests(
    record: &renderpilot_domain::InstalledAddon,
    addon_bytes: &[u8],
    host_path: Option<&renderpilot_domain::PathRef>,
    host_bytes: Option<&[u8]>,
    phase3: &ActiveUpdatePhase1,
) -> Result<(), ServiceError> {
    let mut addon_source = false;
    let mut host_source = false;
    for source in record.tracked_sources() {
        let (seen, bytes) = match source.role() {
            TrackedSourceRole::AddonPayload => (&mut addon_source, Some(addon_bytes)),
            TrackedSourceRole::HostBinary => {
                let bytes = host_bytes.or_else(|| phase3.host_snapshot.as_ref()?.bytes());
                (&mut host_source, bytes)
            }
            TrackedSourceRole::DlssFix | TrackedSourceRole::DgVoodooWrapper => continue,
        };
        if *seen {
            return Err(invalid(format!(
                "active RenoDX update contains duplicate {:?} sources",
                source.role()
            )));
        }
        *seen = true;
        let bytes = bytes.ok_or_else(|| invalid("active RenoDX host source has no endpoint"))?;
        let expected = digest(bytes)?;
        if source.digest() != expected.as_str() {
            return Err(invalid(format!(
                "active RenoDX {:?} source digest does not match its postimage",
                source.role()
            )));
        }
    }
    if host_source && host_path.is_none() {
        return Err(invalid("active RenoDX host source has no proxy host path"));
    }
    Ok(())
}

fn retained_bytes<'a>(
    snapshot: &'a crate::peer_mutation_executor::PeerPathSnapshot,
    path: &renderpilot_domain::PathRef,
) -> Result<&'a [u8], ServiceError> {
    snapshot.bytes().ok_or_else(|| {
        invalid(format!(
            "active RenoDX endpoint has no retained preimage bytes: {path}"
        ))
    })
}

fn digest(bytes: &[u8]) -> Result<renderpilot_domain::Sha256Hash, ServiceError> {
    renderpilot_domain::Sha256Hash::new(hex::encode(Sha256::digest(bytes)))
        .map_err(|error| errors::failed(format!("active RenoDX digest failed: {error}")))
}

fn same_path(left: &Path, right: &renderpilot_domain::PathRef) -> bool {
    crate::paths::same_path(left, Path::new(right.as_str()))
}

fn invalid(message: impl Into<String>) -> ServiceError {
    errors::invalid(message.into())
}
