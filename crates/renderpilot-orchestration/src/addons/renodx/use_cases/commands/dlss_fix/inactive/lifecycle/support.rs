//! Inactive DLSS-Fix support: snapshot, projection and file commit.

use std::path::{Path, PathBuf};

use renderpilot_domain::{
    AddonKind, GameId, InstalledAddon, RenoDxInstallState, TrackedSource, TrackedSourceRole,
};

use crate::addons::game_context::require_game;
use crate::addons::records;
use crate::addons::renodx::dlss_fix::{DlssFixRequest, resolve_dlss_fix};
use crate::addons::renodx::dlss_fix_binding::{self, DlssFixBinding, DlssFixBindingState};
use crate::addons::renodx::{errors, reshade_ini, source, tracking};
use crate::addons::reshade::ini_schema::ini_merge_strategy;
use crate::addons::reshade::scan as reshade;
use crate::addons::reshade::types::{DlssFixIniTweaks, ReshadeIniTweaks};
use crate::file_mutation::{
    MutationScope, RetryableFileMutationV2, RetryableFileOperation, RetryableFilePlan,
    V2DiskObservation, observe,
};
use crate::{Context, ServiceError};

#[cfg_attr(test, derive(Clone))]
pub(super) struct DlssSnapshot {
    pub(super) record: InstalledAddon,
    pub(super) binding: DlssFixBinding,
    pub(super) request: Option<DlssFixRequest>,
    pub(super) ini_path: PathBuf,
    pub(super) ini_observation: V2DiskObservation,
}

/// Complete projection change to commit alongside a prepared DLSS-Fix durable
/// file plan. The canonical path always comes from the snapshot binding, so a
/// caller cannot separately select an unrelated record path.
pub(super) struct DlssProjectionCommit {
    pub(super) intent: DlssProjectionIntent,
    pub(super) operations: Vec<RetryableFileOperation>,
    pub(super) feature: &'static str,
    pub(super) label: &'static str,
}

pub(super) enum DlssProjectionIntent {
    Bind(TrackedSource),
    Clear,
}

pub(super) fn resolve_snapshot(
    context: &Context,
    game_id: &GameId,
    need_request: bool,
) -> Result<DlssSnapshot, ServiceError> {
    let record = records::record_of_kind(context, game_id, AddonKind::RenoDx)?
        .ok_or_else(errors::not_installed)?;
    let binding = dlss_fix_binding::resolve(&record);
    let game = require_game(context, game_id)?;
    let game_root = PathBuf::from(game.install_path().as_str());
    let host_path = crate::addons::tracking::host_proxy_path(&record);
    let paths = reshade::resolve_paths(&game_root, host_path.as_deref());
    let ini_path = paths
        .ini_path
        .unwrap_or_else(|| game_root.join(reshade::RESHADE_INI_FILE_NAME));
    let request = need_request
        .then(|| resolve_dlss_fix(context.storage(), game_id))
        .transpose()?
        .flatten();
    Ok(DlssSnapshot {
        record,
        binding,
        request,
        ini_observation: observe(&ini_path),
        ini_path,
    })
}

pub(super) fn ensure_snapshot_matches(
    before: &DlssSnapshot,
    current: &DlssSnapshot,
) -> Result<(), ServiceError> {
    if before.record != current.record
        || before.binding.state != current.binding.state
        || before.binding.target != current.binding.target
        || before.binding.observation != current.binding.observation
        || before.ini_path != current.ini_path
        || before.ini_observation != current.ini_observation
        || before.request != current.request
    {
        return Err(errors::state_changed_retry_update());
    }
    Ok(())
}

pub(super) fn reconcile_regular_partial(
    context: &Context,
    game_id: &GameId,
    snapshot: &DlssSnapshot,
    mutation_id: Option<&str>,
) -> Result<RenoDxInstallState, ServiceError> {
    let source = match snapshot.binding.state {
        DlssFixBindingState::SourceOnly => snapshot.binding.source.clone(),
        DlssFixBindingState::OwnedOnly => Some(advisory_source_from_live(&snapshot.binding)?),
        _ => {
            return Err(errors::invalid(
                "DLSS-Fix projection is not partial".to_owned(),
            ));
        }
    };
    let updated = tracking::rebuild_with_dlss_projection(
        &snapshot.record,
        Some(&snapshot.binding.target),
        source,
        "DLSS-Fix partial reconciliation",
    )?;
    persist_projection(context, game_id, &updated, mutation_id)?;
    Ok(tracking::install_state_from_record(&updated))
}

fn advisory_source_from_live(binding: &DlssFixBinding) -> Result<TrackedSource, ServiceError> {
    let arch = binding.arch.ok_or_else(invalid_binding)?;
    let V2DiskObservation::Regular { digest } = &binding.observation else {
        return Err(errors::invalid(
            "DLSS-Fix companion is not a readable regular file".to_owned(),
        ));
    };
    Ok(TrackedSource::new(
        TrackedSourceRole::DlssFix,
        source::dlss_fix_url(arch),
        None,
        digest.clone(),
    )
    .with_advisory())
}

pub(super) fn source_from_download(
    arch: renderpilot_domain::Architecture,
    download: &crate::addons::reshade::fetch::Download,
) -> TrackedSource {
    TrackedSource::new(
        TrackedSourceRole::DlssFix,
        source::dlss_fix_url(arch),
        download.etag.clone(),
        download.digest.clone(),
    )
    .with_last_modified(download.last_modified.clone())
}

pub(super) fn install_ini_operation(
    snapshot: &DlssSnapshot,
    request: &DlssFixRequest,
) -> Result<Vec<RetryableFileOperation>, ServiceError> {
    let strategy = ini_merge_strategy(&ReshadeIniTweaks {
        disabled_addons: Vec::new(),
        addon_path: None,
        dlss_fix: Some(DlssFixIniTweaks {
            addon_file_name: snapshot
                .binding
                .target
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(invalid_binding)?
                .to_owned(),
            dlss_path: request.dlss_path.clone(),
            streamline_path: request.streamline_path.clone(),
        }),
    });
    ini_write_operation(&snapshot.ini_path, &snapshot.ini_observation, &strategy)
}

pub(super) fn remove_ini_operation(
    snapshot: &DlssSnapshot,
) -> Result<Vec<RetryableFileOperation>, ServiceError> {
    ini_write_operation(
        &snapshot.ini_path,
        &snapshot.ini_observation,
        &reshade_ini::ini_remove_dlss_fix_strategy(),
    )
}

fn ini_write_operation(
    path: &Path,
    expected: &V2DiskObservation,
    strategy: &crate::addons::engine::MergeStrategy,
) -> Result<Vec<RetryableFileOperation>, ServiceError> {
    let base = match expected {
        V2DiskObservation::Absent => String::new(),
        V2DiskObservation::Regular { .. } => {
            String::from_utf8(std::fs::read(path).map_err(|error| {
                crate::failed(format!(
                    "failed to read active ReShade.ini {}: {error}",
                    path.display()
                ))
            })?)
            .map_err(|_| {
                errors::invalid(format!(
                    "active ReShade.ini is not UTF-8: {}",
                    path.display()
                ))
            })?
        }
        V2DiskObservation::NonRegular | V2DiskObservation::Unreadable => {
            return Err(errors::invalid(format!(
                "active ReShade.ini is unsafe to modify: {}",
                path.display()
            )));
        }
    };
    let next = strategy.apply(&base);
    if next == base {
        return Ok(Vec::new());
    }
    Ok(vec![RetryableFileOperation::Write {
        path: path.to_path_buf(),
        bytes: next.into_bytes(),
        expected: expected.clone(),
    }])
}

pub(super) fn commit_projection(
    context: &Context,
    guard: &crate::game_mutation_lock::GameMutationGuard,
    game_id: &GameId,
    snapshot: &DlssSnapshot,
    commit: DlssProjectionCommit,
) -> Result<RenoDxInstallState, ServiceError> {
    let updated = match commit.intent {
        DlssProjectionIntent::Bind(source) => tracking::rebuild_with_dlss_projection(
            &snapshot.record,
            Some(&snapshot.binding.target),
            Some(source),
            commit.label,
        )?,
        DlssProjectionIntent::Clear => {
            tracking::rebuild_with_dlss_projection(&snapshot.record, None, None, commit.label)?
        }
    };
    if commit.operations.is_empty() {
        persist_projection(context, game_id, &updated, None)?;
        return Ok(tracking::install_state_from_record(&updated));
    }
    let roots = commit
        .operations
        .iter()
        .filter_map(|operation| operation.path().parent().map(Path::to_path_buf));
    let scope = MutationScope::new(roots)?;
    let mutation = RetryableFileMutationV2::prepare(
        context,
        guard,
        &scope,
        commit.feature,
        Some(game_id.as_str()),
        &RetryableFilePlan {
            operations: commit.operations,
        },
    )?;
    mutation.commit_or_rollback(context, |mutation_id| {
        persist_projection(context, game_id, &updated, Some(mutation_id))?;
        Ok(tracking::install_state_from_record(&updated))
    })
}

fn persist_projection(
    context: &Context,
    game_id: &GameId,
    record: &InstalledAddon,
    mutation_id: Option<&str>,
) -> Result<(), ServiceError> {
    context
        .storage()
        .commit_game_mutation(renderpilot_storage_sqlite::GameMutationCommit {
            game_id,
            component_set: None,
            baseline_mutations: &[],
            addon: renderpilot_storage_sqlite::InstalledAddonMutation::Upsert(record),
            mutation_id,
        })?;
    Ok(())
}

pub(super) fn invalid_binding() -> ServiceError {
    errors::invalid(
        "DLSS-Fix record or disk binding requires validation before targeted mutation".to_owned(),
    )
}

pub(super) fn ensure_no_active_topology(
    context: &Context,
    game_id: &GameId,
) -> Result<(), ServiceError> {
    if renderpilot_application::ProxyTopologyRepository::get_proxy_topology(
        context.storage(),
        game_id,
    )?
    .is_some()
    {
        return Err(errors::state_changed_retry_update());
    }
    Ok(())
}
