//! Dedicated DLSS-Fix lifecycle commands.
//!
//! This deliberately does not share the RenoDX add-on/host update path. The
//! companion has its own ownership projection and can be repaired after partial
//! record loss without touching ReShade host or shared Vulkan policy.

use renderpilot_domain::{AddonKind, GameId, RenoDxInstallState};

use crate::addons::progress::emit_tool_finalizing;
use crate::addons::renodx::dlss_fix_binding::DlssFixBindingState;
use crate::addons::renodx::{errors, fetch};
use crate::file_mutation::{RetryableFileOperation, V2DiskObservation};
use crate::game_mutation_lock;
use crate::net::ProgressObserver;
use crate::{Context, ServiceError};

mod support;

use support::{
    DlssProjectionCommit, DlssProjectionIntent, commit_projection, ensure_no_active_topology,
    ensure_snapshot_matches, install_ini_operation, invalid_binding, reconcile_regular_partial,
    remove_ini_operation, resolve_snapshot, source_from_download,
};

/// Explicitly installs/claims DLSS-Fix. An active row with no evidence never
/// auto-claims a physical file; this command is the affirmative user action
/// allowed to replace the exact regular target or create it when absent.
pub async fn install_dlss_fix(
    context: &Context,
    game_id: &GameId,
    safety: crate::GameSafetyPermit,
    progress: Option<&ProgressObserver<'_>>,
) -> Result<RenoDxInstallState, ServiceError> {
    let snapshot = {
        let _guard =
            crate::mutation_boundary::enter_game_mutation_boundary_async(context, game_id).await?;
        ensure_no_active_topology(context, game_id)?;
        resolve_snapshot(context, game_id, true)?
    };
    match snapshot.binding.state {
        DlssFixBindingState::Invalid => return Err(invalid_binding()),
        DlssFixBindingState::SourceOnly | DlssFixBindingState::OwnedOnly
            if matches!(
                snapshot.binding.observation,
                V2DiskObservation::Regular { .. }
            ) =>
        {
            let _guard =
                crate::mutation_boundary::enter_game_mutation_boundary_async(context, game_id)
                    .await?;
            ensure_no_active_topology(context, game_id)?;
            let current = resolve_snapshot(context, game_id, true)?;
            ensure_snapshot_matches(&snapshot, &current)?;
            return crate::FileSafetyAuthority::new().authorize_game_commit(
                context,
                renderpilot_domain::mutation_features::RENODX_DLSS_FIX_INSTALL,
                &_guard,
                &safety,
                || reconcile_regular_partial(context, game_id, &current, None),
            );
        }
        DlssFixBindingState::Bound => {
            return Err(errors::invalid(
                "DLSS-Fix is already installed; use its update action".to_owned(),
            ));
        }
        DlssFixBindingState::SourceOnly | DlssFixBindingState::OwnedOnly => {
            return Err(errors::invalid(
                "DLSS-Fix is missing; use its repair action".to_owned(),
            ));
        }
        DlssFixBindingState::None => {}
    }

    let arch = snapshot.binding.arch.ok_or_else(invalid_binding)?;
    let download = fetch::fetch_dlss_fix(arch, progress).await?;
    let guard =
        crate::mutation_boundary::enter_game_mutation_boundary_async(context, game_id).await?;
    ensure_no_active_topology(context, game_id)?;
    let current = resolve_snapshot(context, game_id, true)?;
    ensure_snapshot_matches(&snapshot, &current)?;
    if current.binding.state != DlssFixBindingState::None {
        return Err(errors::state_changed_retry_update());
    }
    let request = current.request.as_ref().ok_or_else(|| {
        errors::invalid("DLSS-Fix is no longer available for this game".to_owned())
    })?;
    let source = source_from_download(arch, &download);
    let mut operations = vec![RetryableFileOperation::Write {
        path: current.binding.target.clone(),
        bytes: download.bytes,
        expected: current.binding.observation.clone(),
    }];
    operations.extend(install_ini_operation(&current, request)?);
    emit_tool_finalizing(progress, AddonKind::RenoDx);
    crate::FileSafetyAuthority::new().authorize_game_commit(
        context,
        renderpilot_domain::mutation_features::RENODX_DLSS_FIX_INSTALL,
        &guard,
        &safety,
        || {
            commit_projection(
                context,
                &guard,
                game_id,
                &current,
                DlssProjectionCommit {
                    intent: DlssProjectionIntent::Bind(source),
                    operations,
                    feature: renderpilot_domain::mutation_features::RENODX_DLSS_FIX_INSTALL,
                    label: "DLSS-Fix install projection",
                },
            )
        },
    )
}

/// Updates the companion or repairs a source/ownership projection. Repair is
/// payload-only: it never rewrites the active ReShade.ini or touches host policy.
pub async fn update_dlss_fix(
    context: &Context,
    game_id: &GameId,
    safety: crate::GameSafetyPermit,
    progress: Option<&ProgressObserver<'_>>,
) -> Result<RenoDxInstallState, ServiceError> {
    let snapshot = {
        let _guard =
            crate::mutation_boundary::enter_game_mutation_boundary_async(context, game_id).await?;
        ensure_no_active_topology(context, game_id)?;
        resolve_snapshot(context, game_id, false)?
    };
    match snapshot.binding.state {
        DlssFixBindingState::Invalid => return Err(invalid_binding()),
        DlssFixBindingState::None => {
            return Err(errors::invalid(
                "DLSS-Fix is not installed; use its install action".to_owned(),
            ));
        }
        DlssFixBindingState::SourceOnly | DlssFixBindingState::OwnedOnly
            if matches!(
                snapshot.binding.observation,
                V2DiskObservation::Regular { .. }
            ) =>
        {
            let _guard =
                crate::mutation_boundary::enter_game_mutation_boundary_async(context, game_id)
                    .await?;
            ensure_no_active_topology(context, game_id)?;
            let current = resolve_snapshot(context, game_id, false)?;
            ensure_snapshot_matches(&snapshot, &current)?;
            return crate::FileSafetyAuthority::new().authorize_game_commit(
                context,
                renderpilot_domain::mutation_features::RENODX_DLSS_FIX_UPDATE,
                &_guard,
                &safety,
                || reconcile_regular_partial(context, game_id, &current, None),
            );
        }
        DlssFixBindingState::SourceOnly
        | DlssFixBindingState::OwnedOnly
        | DlssFixBindingState::Bound => {}
    }

    let arch = snapshot.binding.arch.ok_or_else(invalid_binding)?;
    let download = fetch::fetch_dlss_fix(arch, progress).await?;
    let guard =
        crate::mutation_boundary::enter_game_mutation_boundary_async(context, game_id).await?;
    ensure_no_active_topology(context, game_id)?;
    let current = resolve_snapshot(context, game_id, false)?;
    ensure_snapshot_matches(&snapshot, &current)?;
    if current.binding.state == DlssFixBindingState::Invalid {
        return Err(invalid_binding());
    }
    let source = source_from_download(arch, &download);
    // Bound regular updates are normal content refreshes. Missing partial/bound
    // rows recreate the exact target, still without touching the INI.
    let operations = vec![RetryableFileOperation::Write {
        path: current.binding.target.clone(),
        bytes: download.bytes,
        expected: current.binding.observation.clone(),
    }];
    emit_tool_finalizing(progress, AddonKind::RenoDx);
    crate::FileSafetyAuthority::new().authorize_game_commit(
        context,
        renderpilot_domain::mutation_features::RENODX_DLSS_FIX_UPDATE,
        &guard,
        &safety,
        || {
            commit_projection(
                context,
                &guard,
                game_id,
                &current,
                DlssProjectionCommit {
                    intent: DlssProjectionIntent::Bind(source),
                    operations,
                    feature: renderpilot_domain::mutation_features::RENODX_DLSS_FIX_UPDATE,
                    label: "DLSS-Fix update projection",
                },
            )
        },
    )
}

/// Retries only pending durable-file recovery for this game, then reports the
/// current RenoDX state. This is deliberately a narrow mutation boundary:
/// it neither fetches a payload nor runs ReShade/host reconciliation policy.
pub fn retry_dlss_fix_recovery(
    context: &Context,
    game_id: &GameId,
) -> Result<RenoDxInstallState, ServiceError> {
    let guard = game_mutation_lock::blocking_lock(game_id);
    let recovered = crate::file_mutation::recover_pending_matching(context, &guard, |row| {
        renderpilot_domain::mutation_features::is_renodx_dlss_fix_feature(&row.feature)
    })?;
    if recovered == 0 {
        return Err(errors::invalid(
            "no pending DLSS-Fix recovery exists for this game".to_owned(),
        ));
    }
    crate::addons::renodx::use_cases::queries::status::status(context, game_id)
}

/// Removes only the companion's exact recorded target and its active game-root
/// INI entries. A source-only row never grants deletion authority over a merely
/// physical exact file, so it clears metadata/INI only.
pub fn uninstall_dlss_fix(
    context: &Context,
    game_id: &GameId,
) -> Result<RenoDxInstallState, ServiceError> {
    let guard = crate::mutation_boundary::enter_game_mutation_boundary(context, game_id)?;
    ensure_no_active_topology(context, game_id)?;
    let snapshot = resolve_snapshot(context, game_id, false)?;
    if snapshot.binding.state == DlssFixBindingState::Invalid {
        return Err(invalid_binding());
    }
    if snapshot.binding.state == DlssFixBindingState::None {
        return Err(errors::invalid(
            "DLSS-Fix is not installed for this game".to_owned(),
        ));
    }
    let mut operations = remove_ini_operation(&snapshot)?;
    if matches!(
        snapshot.binding.state,
        DlssFixBindingState::OwnedOnly | DlssFixBindingState::Bound
    ) && matches!(
        snapshot.binding.observation,
        V2DiskObservation::Regular { .. }
    ) {
        operations.insert(
            0,
            RetryableFileOperation::Delete {
                path: snapshot.binding.target.clone(),
                expected: snapshot.binding.observation.clone(),
            },
        );
    }
    commit_projection(
        context,
        &guard,
        game_id,
        &snapshot,
        DlssProjectionCommit {
            intent: DlssProjectionIntent::Clear,
            operations,
            feature: renderpilot_domain::mutation_features::RENODX_DLSS_FIX_UNINSTALL,
            label: "DLSS-Fix removal projection",
        },
    )
}

#[cfg(test)]
mod tests;
