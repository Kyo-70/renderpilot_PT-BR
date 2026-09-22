//! Pure installation-record projection for the active RenoDX route.
//!
//! The record is built from the immutable install snapshot and the bytes that
//! were prepared outside the mutation boundary.  In particular, this module
//! does not infer ownership from topology: the host binding supplied by the
//! host classifier is the only source of managed-host ownership.

use renderpilot_domain::{
    AddonKind, InstalledAddon, InstalledAddonHostKind, InstalledAddonParts, ManagedAddonFile,
    PathRef, RenoDxConfigReceipt, Sha256Hash, TrackedSource, TrackedSourceRole,
};

use crate::addons::renodx::install::PreparedInstall;
use crate::addons::renodx::peer::InstallActiveSnapshot;
use crate::addons::reshade::proxy::HostKind;
use crate::addons::reshade::update::host_binary_source;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RenoDxRecordError {
    InvalidInput(&'static str),
    Record(String),
}

impl std::fmt::Display for RenoDxRecordError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidInput(reason) => {
                write!(formatter, "invalid RenoDX record input: {reason}")
            }
            Self::Record(reason) => write!(formatter, "invalid RenoDX install record: {reason}"),
        }
    }
}

impl std::error::Error for RenoDxRecordError {}

/// Builds the exact reversible record for one active install.
///
/// `host` is present only for proxy installs.  A reused host is retained as a
/// `Reused` managed binding; an owned create/repair is retained as `Owned`.
/// The config projection supplies the only generic created-file addition for
/// `ReShade.ini`; replacing an existing config is intentionally not recorded
/// as ownership and never creates a backup entry.
pub(crate) fn build_record(
    snapshot: &InstallActiveSnapshot,
    prepared: &PreparedInstall,
    host: Option<&ManagedAddonFile>,
    config_created: bool,
    config_receipt: Option<&RenoDxConfigReceipt>,
) -> Result<InstalledAddon, RenoDxRecordError> {
    let addon_file = snapshot.payload_path().clone();
    let mut created_files = vec![addon_file.clone()];
    if config_created {
        let ini_path = snapshot
            .root_seal()
            .config_source()
            .exact_ini_path()
            .to_str()
            .ok_or(RenoDxRecordError::InvalidInput(
                "invalid non-UTF-8 ReShade.ini path",
            ))?;
        created_files.push(
            PathRef::new(ini_path.to_owned())
                .map_err(|_| RenoDxRecordError::InvalidInput("invalid ReShade.ini path"))?,
        );
    }

    let mut managed_files = Vec::new();
    if matches!(prepared.host_kind, HostKind::Proxy) {
        managed_files.push(host.cloned().ok_or(RenoDxRecordError::InvalidInput(
            "proxy record is missing host binding",
        ))?);
    } else if host.is_some() {
        return Err(RenoDxRecordError::InvalidInput(
            "Vulkan record cannot carry a proxy host binding",
        ));
    }

    let mut tracked_sources = Vec::new();
    if !prepared.addon_source_url.is_empty() || prepared.source_last_modified.is_some() {
        tracked_sources.push(
            TrackedSource::new(
                TrackedSourceRole::AddonPayload,
                prepared.addon_source_url.clone(),
                prepared.source_etag.clone(),
                prepared.source_digest.clone(),
            )
            .with_last_modified(prepared.source_last_modified.clone()),
        );
    }

    let host_owned = managed_files
        .first()
        .is_some_and(|file| file.mode() == renderpilot_domain::ManagedFileMode::Owned);
    if host_owned {
        if prepared.reshade_source_url.trim().is_empty() {
            return Err(RenoDxRecordError::InvalidInput(
                "owned ReShade host source URL is empty",
            ));
        }
        let source_digest = Sha256Hash::new(prepared.reshade_digest.clone()).map_err(|_| {
            RenoDxRecordError::InvalidInput("owned ReShade host source digest is empty or invalid")
        })?;
        if &source_digest != managed_files[0].installed_sha256() {
            return Err(RenoDxRecordError::InvalidInput(
                "owned ReShade host source digest differs from prepared bytes",
            ));
        }
        tracked_sources.push(host_binary_source(
            prepared.reshade_source_url.clone(),
            prepared.reshade_source_etag.clone(),
            prepared.reshade_digest.clone(),
            prepared.reshade_last_modified.clone(),
            prepared.reshade_channel,
        ));
    }

    let mut record = InstalledAddon::from_parts_with_managed(InstalledAddonParts {
        game_id: prepared.game_id.clone(),
        kind: AddonKind::RenoDx,
        addon_file,
        addon_version: None,
        created_files,
        backed_up_files: Vec::new(),
        managed_files,
        tracked_sources,
        renodx_config_receipt: config_receipt.cloned(),
        engine_config_journal: None,
    })
    .map_err(|error| RenoDxRecordError::Record(error.to_string()))?
    .ok_or(RenoDxRecordError::InvalidInput(
        "payload add-on is not listed among created files",
    ))?;

    record = record
        .with_host_kind(match prepared.host_kind {
            HostKind::Proxy => InstalledAddonHostKind::Proxy,
            HostKind::Vulkan => InstalledAddonHostKind::SharedVulkanLayer,
        })
        .with_reshade_channel(snapshot.requested_channel().as_str());

    if matches!(prepared.host_kind, HostKind::Vulkan) {
        let executable =
            snapshot
                .registered_executable()
                .cloned()
                .ok_or(RenoDxRecordError::InvalidInput(
                    "Vulkan record is missing its registered executable",
                ))?;
        record = record.with_registered_exe_path(executable);
    }

    Ok(record)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::addons::peer_lifecycle::PeerRoots;
    use crate::addons::renodx::peer::{
        InstallCommandVariant, RenoDxConfigSourceSeal, RenoDxRootSeal,
    };
    use crate::addons::reshade::proxy::HostKind;
    use crate::addons::reshade::types::{ReshadeChannel, ReshadeIniTweaks};
    use crate::peer_mutation_executor::PeerPathSnapshot;
    use renderpilot_domain::{GameId, RenoDxReshadeIniFeature, Sha256Hash};

    fn hash(value: char) -> Sha256Hash {
        Sha256Hash::new(value.to_string().repeat(Sha256Hash::HEX_LENGTH)).expect("hash")
    }

    fn snapshot(root: &std::path::Path) -> InstallActiveSnapshot {
        let root = crate::paths::canonicalize_existing(root).expect("canonical root");
        let root_ref = renderpilot_domain::PathRef::new(root.to_string_lossy().into_owned())
            .expect("root ref");
        let ini_path = root.join("ReShade.ini");
        InstallActiveSnapshot {
            variant: InstallCommandVariant::Catalog,
            feature: RenoDxReshadeIniFeature::Install,
            request_fingerprint: hash('a'),
            plan_fingerprint: hash('b'),
            requested_channel: ReshadeChannel::Stable,
            canonical_target_dir: root.clone(),
            canonical_target_dir_ref: root_ref.clone(),
            topology: None,
            root_seal: RenoDxRootSeal {
                canonical_game_root: root.clone(),
                canonical_game_root_ref: root_ref.clone(),
                config_source: RenoDxConfigSourceSeal::Absent {
                    exact_ini_path: ini_path.clone(),
                },
                effective_addon_root: root.clone(),
                payload_root: None,
                payload_root_ref: None,
                exact_ini_path: ini_path,
                exact_proxy_host: None,
                canonical_registered_exe: None,
                roots: PeerRoots::new(root, None).expect("roots"),
            },
            payload_path: renderpilot_domain::PathRef::new(format!(
                "{}/renodx.addon64",
                root_ref.as_str()
            ))
            .expect("addon path"),
            payload_preimage: PeerPathSnapshot::Absent,
            host_path: None,
            host_preimage: None,
            host_assessment: None,
            acquisition_sidecar_preimage: None,
            registered_executable: Some(
                renderpilot_domain::PathRef::new(format!("{}/game.exe", root_ref.as_str()))
                    .expect("exe path"),
            ),
            writes_host: false,
            content: crate::addons::reshade::scan::ReshadeContent::Empty,
        }
    }

    fn prepared(game_id: GameId) -> PreparedInstall {
        PreparedInstall {
            game_id,
            host_kind: HostKind::Vulkan,
            processing_path: crate::addons::renodx::types::RenoDxProcessingPath::Unmanaged,
            renodx_config: None,
            proxy_dll_name: String::new(),
            addon_file_name: "renodx.addon64".to_owned(),
            addon_source_url: String::new(),
            source_digest: String::new(),
            source_etag: None,
            source_last_modified: None,
            addon_bytes: b"addon".to_vec(),
            reshade_dll_bytes: Vec::new(),
            reshade_source_url: String::new(),
            reshade_source_etag: None,
            reshade_last_modified: None,
            reshade_digest: String::new(),
            reshade_channel: None,
            ini_tweaks: ReshadeIniTweaks::default(),
        }
    }

    #[test]
    fn vulkan_record_preserves_shared_host_kind_channel_and_exe() {
        let root = tempfile::tempdir().expect("root");
        let snapshot = snapshot(root.path());
        let game_id = GameId::new("manual:test").expect("game id");
        let record = build_record(&snapshot, &prepared(game_id), None, true, None).expect("record");
        let canonical_root = crate::paths::canonicalize_existing(root.path()).expect("canonical");
        let expected_exe = PathRef::new(format!("{}/game.exe", canonical_root.to_string_lossy()))
            .expect("exe path");

        assert_eq!(
            record.host_kind(),
            Some(InstalledAddonHostKind::SharedVulkanLayer)
        );
        assert_eq!(record.reshade_channel(), Some("stable"));
        assert_eq!(
            record.registered_exe_path().map(PathRef::as_str),
            Some(expected_exe.as_str())
        );
        assert_eq!(record.created_files().len(), 2);
        assert!(record.tracked_sources().is_empty());
        assert!(record.backed_up_files().is_empty());
    }
}
