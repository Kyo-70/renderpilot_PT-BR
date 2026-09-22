//! Unlocked network preparation for RenoDX updates.

use std::path::{Path, PathBuf};

use renderpilot_domain::{
    InstalledAddon, PathRef, RenoDxConfigReceipt, TrackedSource, TrackedSourceRole,
};

use crate::ServiceError;
use crate::addons::file_update::Replacement;
use crate::addons::progress::sequential_stage_observer;
use crate::addons::records::addon_label;
use crate::addons::renodx::use_cases::reshade_update::HostUpdateTarget;
use crate::addons::renodx::{fetch, tracking};
use crate::addons::reshade::fetch::{Download, fetch_reshade_from_source};
use crate::addons::reshade::update::host_binary_source;
use crate::net::ProgressObserver;

use super::snapshot::UpdateSnapshot;

/// All network results needed by the locked commit phase.
pub(super) struct PreparedUpdateArtifacts {
    pub(super) refreshed_sources: Vec<TrackedSource>,
    pub(super) replacements: Vec<Replacement>,
    pub(super) host_install: Option<HostInstall>,
    pub(super) config: Option<PreparedRenoDxConfig>,
}

/// Exact RenoDX configuration transition prepared under the phase-three game lock. The
/// update commit consumes this projection together with binary replacements;
/// no later path or value derivation is permitted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PreparedRenoDxConfig {
    pub(super) path: PathBuf,
    pub(super) before: Option<Vec<u8>>,
    pub(super) after: Option<Vec<u8>>,
    pub(super) receipt: Option<RenoDxConfigReceipt>,
    pub(super) physical_changed: bool,
}

impl PreparedRenoDxConfig {
    pub(super) fn replacement(&self) -> Option<Replacement> {
        if !self.physical_changed {
            return None;
        }
        self.after.as_ref().map(|bytes| Replacement {
            path: self.path.clone(),
            bytes: bytes.clone(),
            mtime: None,
        })
    }
}

impl PreparedUpdateArtifacts {
    pub(super) fn replacement_paths(&self) -> Vec<PathBuf> {
        self.replacements
            .iter()
            .map(|replacement| replacement.path.clone())
            .collect()
    }

    pub(super) fn host_install_path(&self) -> Option<PathBuf> {
        self.host_install
            .as_ref()
            .map(|install| install.game_dir.join(&install.name))
    }

    pub(super) fn with_config(mut self, config: Option<PreparedRenoDxConfig>) -> Self {
        self.config = config;
        self
    }
}

pub(super) async fn prepare_update_artifacts(
    snapshot: &UpdateSnapshot,
    progress: Option<&ProgressObserver<'_>>,
) -> Result<PreparedUpdateArtifacts, ServiceError> {
    let record = &snapshot.record;
    let addon_tracked = snapshot
        .addon
        .as_ref()
        .is_some_and(|source| !source.url().is_empty());
    // The generic update has no authority to fetch, write, or refresh DLSS-Fix.
    // Preserve its exact phase-3 projection, including advisory/partial evidence.
    let mut refreshed_sources: Vec<TrackedSource> = record
        .tracked_sources()
        .iter()
        .filter(|source| source.role() == TrackedSourceRole::DlssFix)
        .cloned()
        .collect();
    let mut replacements = Vec::new();
    let mut host_install = None;
    // Shared Vulkan is applied in phase 3; do not count it in unlocked stages.
    let stage_count = u64::from(addon_tracked)
        + u64::from(snapshot.host.is_some() && snapshot.host_target.is_some());
    let mut stage_index = 0;

    if let Some(addon) = snapshot.addon.as_ref() {
        if addon.url().is_empty() {
            refreshed_sources.push(addon.clone());
        } else {
            let stage_progress_fn = sequential_stage_observer(progress, stage_index, stage_count);
            let stage_progress = stage_progress_fn
                .as_ref()
                .map(|observer| observer as &ProgressObserver<'_>);
            let prepared = prepare_addon_update(record, addon, stage_progress).await?;
            refreshed_sources.push(prepared.source);
            replacements.extend(prepared.replacement);
            stage_index += 1;
        }
    }

    if let (Some(host), Some(target)) = (snapshot.host.as_ref(), snapshot.host_target.as_ref()) {
        let stage_progress_fn = sequential_stage_observer(progress, stage_index, stage_count);
        let stage_progress = stage_progress_fn
            .as_ref()
            .map(|observer| observer as &ProgressObserver<'_>);
        let prepared = prepare_policy_host_update(record, target, host, stage_progress).await?;
        refreshed_sources.push(prepared.source);
        if let Some(replacement) = prepared.replacement {
            match replacement {
                HostReplacement::InPlace(replacement) => replacements.push(replacement),
                HostReplacement::Install(install) => host_install = Some(install),
            }
        }
        stage_index += 1;
    } else if let Some(host) = snapshot.host.as_ref() {
        refreshed_sources.push(host.clone());
    }

    debug_assert_eq!(stage_index, stage_count);

    Ok(PreparedUpdateArtifacts {
        refreshed_sources,
        replacements,
        host_install,
        config: None,
    })
}

/// Captures and plans only RenoDX's typed configuration reconciliation. This is
/// intentionally called after phase-three route validation while the game
/// lock is held, so the supplied before image is the one the durable commit
/// will replace or restore.
pub(super) fn prepare_config_update(
    snapshot: &UpdateSnapshot,
) -> Result<Option<PreparedRenoDxConfig>, ServiceError> {
    let desired = snapshot.processing_path.desired_set_path();
    let config = snapshot.renodx_config.as_ref();
    let receipt = snapshot.record.renodx_config_receipt();
    if desired.is_none()
        && !config.is_some_and(|config| !config.settings.is_empty())
        && receipt.is_none()
    {
        return Ok(None);
    }

    let path = receipt
        .map(|receipt| PathBuf::from(receipt.ini_path.as_str()))
        .or_else(|| crate::addons::reshade::scan::reshade_ini_path(&snapshot.game_dir))
        .unwrap_or_else(|| {
            snapshot
                .game_dir
                .join(crate::addons::reshade::scan::RESHADE_INI_FILE_NAME)
        });
    if !crate::paths::is_within(&path, &snapshot.game_dir)
        || path
            .parent()
            .is_none_or(|parent| !crate::paths::same_path(parent, &snapshot.game_dir))
        || !path.file_name().is_some_and(|name| {
            name.eq_ignore_ascii_case(crate::addons::reshade::scan::RESHADE_INI_FILE_NAME)
        })
    {
        return Err(crate::failed(format!(
            "RenoDX ReShade.ini path is outside the game root or has an invalid leaf: {}",
            path.display()
        )));
    }
    let path_str = path
        .to_str()
        .ok_or_else(|| crate::failed("RenoDX ReShade.ini path is not valid UTF-8"))?;
    let path_ref = PathRef::new(path_str)
        .map_err(|error| crate::failed(format!("invalid RenoDX ReShade.ini path: {error}")))?;
    let before = read_config_before(&path)?;
    let planned = crate::addons::renodx::reshade_ini::plan_config_reconcile(
        path_ref,
        before.as_deref(),
        desired,
        config,
        receipt,
    )
    .map_err(|error| crate::failed(format!("cannot reconcile RenoDX configuration: {error}")))?;
    let physical_changed = before != planned.after;
    let metadata_changed = receipt != planned.receipt.as_ref();
    if !physical_changed && !metadata_changed {
        return Ok(None);
    }
    Ok(Some(PreparedRenoDxConfig {
        path,
        before,
        after: planned.after,
        receipt: planned.receipt,
        physical_changed,
    }))
}

fn read_config_before(path: &Path) -> Result<Option<Vec<u8>>, ServiceError> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(crate::failed(error.to_string())),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(crate::failed(format!(
            "RenoDX ReShade.ini is not a regular file: {}",
            path.display()
        )));
    }
    std::fs::read(path)
        .map(Some)
        .map_err(|error| crate::failed(error.to_string()))
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;
    use crate::addons::renodx::types::RenoDxProcessingPath;
    use renderpilot_domain::{
        AddonKind, GameId, InstalledAddon, PathRef, RenoDxConfigReceipt, RenoDxSetPathBaseline,
        RenoDxSetPathValue,
    };
    use tempfile::tempdir;

    fn snapshot(
        game_dir: &std::path::Path,
        processing_path: RenoDxProcessingPath,
        receipt: Option<RenoDxConfigReceipt>,
    ) -> UpdateSnapshot {
        let addon =
            PathRef::new(game_dir.join("renodx.addon64").to_string_lossy()).expect("addon path");
        let record = InstalledAddon::new(
            GameId::new("manual:renodx-config-prepare").expect("game id"),
            AddonKind::RenoDx,
            addon,
        )
        .with_renodx_config_receipt(receipt)
        .expect("receipt");
        UpdateSnapshot {
            record,
            game_dir: game_dir.to_path_buf(),
            processing_path,
            renodx_config: None,
            shared_vulkan_channel: None,
            addon: None,
            host: None,
            host_target: None,
        }
    }

    fn receipt(
        path: &std::path::Path,
        baseline: RenoDxSetPathBaseline,
        last: RenoDxSetPathValue,
    ) -> RenoDxConfigReceipt {
        let section_preexisted = matches!(baseline, RenoDxSetPathBaseline::Present { .. });
        RenoDxConfigReceipt::new(
            PathRef::new(path.to_string_lossy()).expect("ini path"),
            baseline,
            section_preexisted,
            last,
        )
    }

    #[test]
    fn config_prepare_captures_baseline_and_reconciles_policy_flips() {
        let root = tempdir().expect("root");
        let ini = root.path().join("ReShade.ini");
        std::fs::write(&ini, b"[renodx]\nSet_Path=arbitrary\n").expect("ini");
        let prepared =
            prepare_config_update(&snapshot(root.path(), RenoDxProcessingPath::Native, None))
                .expect("capture")
                .expect("config projection");
        assert_eq!(
            prepared.after.as_deref(),
            Some(b"[renodx]\nSet_Path=0\n".as_slice())
        );
        assert_eq!(
            prepared.receipt.as_ref().expect("receipt").baseline,
            RenoDxSetPathBaseline::Present {
                value: "arbitrary".to_owned()
            }
        );

        let current = b"[renodx]\nSet_Path=1\n";
        std::fs::write(&ini, current).expect("reset ini");
        let record_snapshot = snapshot(
            root.path(),
            RenoDxProcessingPath::Native,
            Some(receipt(
                &ini,
                RenoDxSetPathBaseline::Present {
                    value: "arbitrary".to_owned(),
                },
                RenoDxSetPathValue::One,
            )),
        );
        let prepared = prepare_config_update(&record_snapshot)
            .expect("flip")
            .expect("config projection");
        assert_eq!(
            prepared.after.as_deref(),
            Some(b"[renodx]\nSet_Path=0\n".as_slice())
        );
        assert_eq!(
            prepared.receipt.as_ref().expect("receipt").baseline,
            RenoDxSetPathBaseline::Present {
                value: "arbitrary".to_owned()
            }
        );

        std::fs::write(&ini, b"[renodx]\nSet_Path=user-edit\n").expect("edit ini");
        assert!(prepare_config_update(&record_snapshot).is_err());
    }

    #[test]
    fn config_prepare_fails_closed_on_an_external_edit_when_relinquishing_path() {
        let root = tempdir().expect("root");
        let ini = root.path().join("ReShade.ini");
        let receipt = receipt(&ini, RenoDxSetPathBaseline::Absent, RenoDxSetPathValue::One);
        std::fs::write(&ini, b"[renodx]\nSet_Path=1\n").expect("ini");
        let prepared = prepare_config_update(&snapshot(
            root.path(),
            RenoDxProcessingPath::Unmanaged,
            Some(receipt.clone()),
        ))
        .expect("relinquish")
        .expect("config projection");
        assert_eq!(prepared.receipt, None);
        assert_eq!(prepared.after.as_deref(), Some(b"".as_slice()));

        std::fs::write(&ini, b"[renodx]\nSet_Path=user-edit\n").expect("edit ini");
        let error = prepare_config_update(&snapshot(
            root.path(),
            RenoDxProcessingPath::Unmanaged,
            Some(receipt),
        ))
        .expect_err("external edit must fail closed");
        assert!(error.to_string().contains("edited outside RenderPilot"));
    }
}

struct PreparedSourceUpdate {
    source: TrackedSource,
    replacement: Option<Replacement>,
}

pub(super) struct HostInstall {
    pub(super) game_dir: PathBuf,
    pub(super) name: String,
    pub(super) bytes: Vec<u8>,
}

enum HostReplacement {
    InPlace(Replacement),
    Install(HostInstall),
}

struct PreparedHostPolicyUpdate {
    source: TrackedSource,
    replacement: Option<HostReplacement>,
}

async fn prepare_addon_update(
    record: &InstalledAddon,
    source: &TrackedSource,
    progress: Option<&ProgressObserver<'_>>,
) -> Result<PreparedSourceUpdate, ServiceError> {
    let download = fetch::fetch_addon(source.url(), addon_label(record), progress).await?;
    let changed = download.digest != source.digest();
    let refreshed = refreshed_source(source, &download);
    Ok(PreparedSourceUpdate {
        source: refreshed,
        replacement: changed.then(|| Replacement {
            path: addon_path(record),
            bytes: download.bytes,
            mtime: download.last_modified.clone(),
        }),
    })
}

async fn prepare_policy_host_update(
    record: &InstalledAddon,
    target: &HostUpdateTarget,
    existing_source: &TrackedSource,
    progress: Option<&ProgressObserver<'_>>,
) -> Result<PreparedHostPolicyUpdate, ServiceError> {
    let download = fetch_reshade_from_source(&target.source, target.arch, progress).await?;
    let changed = download.digest != existing_source.digest() || target.action.writes_host();
    let source = host_binary_source(
        target.source.url.clone(),
        download.etag,
        download.digest,
        download.last_modified,
        Some(target.channel),
    );

    let replacement = if changed {
        match tracking::required_rollback_host_path(record) {
            Ok(path) if crate::paths::same_path(&path, &target.target_path) => {
                Some(HostReplacement::InPlace(Replacement {
                    path,
                    bytes: download.bytes,
                    mtime: None,
                }))
            }
            Ok(_) | Err(_) => Some(HostReplacement::Install(HostInstall {
                game_dir: target.game_dir.clone(),
                name: target.slot.clone(),
                bytes: download.bytes,
            })),
        }
    } else {
        None
    };

    Ok(PreparedHostPolicyUpdate {
        source,
        replacement,
    })
}

fn refreshed_source(source: &TrackedSource, download: &Download) -> TrackedSource {
    TrackedSource::new(
        source.role(),
        source.url().to_owned(),
        download.etag.clone(),
        download.digest.clone(),
    )
    .with_last_modified(download.last_modified.clone())
}

fn addon_path(record: &InstalledAddon) -> PathBuf {
    PathBuf::from(record.addon_file().as_str())
}
