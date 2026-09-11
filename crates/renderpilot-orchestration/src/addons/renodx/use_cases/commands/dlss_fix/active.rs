//! Active-topology RenoDX DLSS-Fix lifecycle.
//!
//! A persisted topology is handled only through the typed durable peer route.
//! All aggregate, root, and endpoint observations are sealed before download;
//! the commit consumes that sealed snapshot and never falls back to a generic
//! metadata shortcut.

mod commit;
mod snapshot;

use std::path::Path;

use renderpilot_domain::{AddonKind, GameId, RenoDxInstallState};

use crate::addons::progress::emit_tool_finalizing;
use crate::addons::renodx::dlss_fix_binding::DlssFixBindingState;
use crate::addons::renodx::peer::ActiveDlssEffect;
use crate::addons::renodx::{errors, fetch, tracking};
use crate::net::ProgressObserver;
use crate::{Context, ServiceError};

use self::commit::{
    ActiveDlssCommit, commit_claim_or_physical, install_ini_effect, reconcile_effect,
    reconcile_partial, remove_ini_effect, source_from_download,
};
use self::snapshot::{ensure_same, resolve_snapshot};

pub(crate) async fn install(
    context: &Context,
    game_id: &GameId,
    safety: crate::GameSafetyPermit,
    progress: Option<&ProgressObserver<'_>>,
) -> Result<RenoDxInstallState, ServiceError> {
    let initial = {
        let _guard =
            crate::mutation_boundary::enter_game_mutation_boundary_async(context, game_id).await?;
        resolve_snapshot(context, game_id, true)?
    };
    match initial.binding.state {
        DlssFixBindingState::Invalid => return Err(commit::invalid_binding()),
        DlssFixBindingState::Bound => {
            return Err(errors::invalid(
                "DLSS-Fix is already installed; use its update action".to_owned(),
            ));
        }
        DlssFixBindingState::SourceOnly | DlssFixBindingState::OwnedOnly
            if initial.companion.file().is_some() =>
        {
            let guard =
                crate::mutation_boundary::enter_game_mutation_boundary_async(context, game_id)
                    .await?;
            let current = resolve_snapshot(context, game_id, true)?;
            ensure_same(&initial, &current)?;
            let after = reconcile_partial(&current)?;
            let effect = reconcile_effect(&current)?;
            emit_tool_finalizing(progress, AddonKind::RenoDx);
            return crate::FileSafetyAuthority::new().authorize_game_commit(
                context,
                renderpilot_domain::mutation_features::RENODX_DLSS_FIX_INSTALL,
                &guard,
                &safety,
                || {
                    commit_claim_or_physical(ActiveDlssCommit {
                        context,
                        guard: &guard,
                        game_id,
                        snapshot: &current,
                        after,
                        companion_effect: effect,
                        ini: None,
                        feature: renderpilot_domain::mutation_features::RENODX_DLSS_FIX_INSTALL,
                    })
                },
            );
        }
        DlssFixBindingState::SourceOnly
        | DlssFixBindingState::OwnedOnly
        | DlssFixBindingState::None => {}
    }

    let arch = initial.binding.arch.ok_or_else(commit::invalid_binding)?;
    let download = fetch::fetch_dlss_fix(arch, progress).await?;
    let guard =
        crate::mutation_boundary::enter_game_mutation_boundary_async(context, game_id).await?;
    let current = resolve_snapshot(context, game_id, true)?;
    ensure_same(&initial, &current)?;
    if !matches!(
        current.binding.state,
        DlssFixBindingState::None
            | DlssFixBindingState::SourceOnly
            | DlssFixBindingState::OwnedOnly
    ) {
        return Err(errors::state_changed_retry_update());
    }
    let request = current.request.as_ref().ok_or_else(|| {
        errors::invalid("DLSS-Fix is no longer available for this game".to_owned())
    })?;
    let after = tracking::rebuild_with_dlss_projection(
        &current.record,
        Some(Path::new(current.companion_path.as_str())),
        Some(source_from_download(arch, &download)),
        "DLSS-Fix active install",
    )?;
    let ini_effect = install_ini_effect(&current, request)?;
    emit_tool_finalizing(progress, AddonKind::RenoDx);
    crate::FileSafetyAuthority::new().authorize_game_commit(
        context,
        renderpilot_domain::mutation_features::RENODX_DLSS_FIX_INSTALL,
        &guard,
        &safety,
        || {
            commit_claim_or_physical(ActiveDlssCommit {
                context,
                guard: &guard,
                game_id,
                snapshot: &current,
                after,
                companion_effect: ActiveDlssEffect::Write(download.bytes),
                ini: ini_effect,
                feature: renderpilot_domain::mutation_features::RENODX_DLSS_FIX_INSTALL,
            })
        },
    )
}

pub(crate) async fn update(
    context: &Context,
    game_id: &GameId,
    safety: crate::GameSafetyPermit,
    progress: Option<&ProgressObserver<'_>>,
) -> Result<RenoDxInstallState, ServiceError> {
    let initial = {
        let _guard =
            crate::mutation_boundary::enter_game_mutation_boundary_async(context, game_id).await?;
        resolve_snapshot(context, game_id, false)?
    };
    match initial.binding.state {
        DlssFixBindingState::Invalid => return Err(commit::invalid_binding()),
        DlssFixBindingState::None => {
            return Err(errors::invalid(
                "DLSS-Fix is not installed; use its install action".to_owned(),
            ));
        }
        DlssFixBindingState::SourceOnly | DlssFixBindingState::OwnedOnly
            if initial.companion.file().is_some() =>
        {
            let guard =
                crate::mutation_boundary::enter_game_mutation_boundary_async(context, game_id)
                    .await?;
            let current = resolve_snapshot(context, game_id, false)?;
            ensure_same(&initial, &current)?;
            let after = reconcile_partial(&current)?;
            let effect = reconcile_effect(&current)?;
            emit_tool_finalizing(progress, AddonKind::RenoDx);
            return crate::FileSafetyAuthority::new().authorize_game_commit(
                context,
                renderpilot_domain::mutation_features::RENODX_DLSS_FIX_UPDATE,
                &guard,
                &safety,
                || {
                    commit_claim_or_physical(ActiveDlssCommit {
                        context,
                        guard: &guard,
                        game_id,
                        snapshot: &current,
                        after,
                        companion_effect: effect,
                        ini: None,
                        feature: renderpilot_domain::mutation_features::RENODX_DLSS_FIX_UPDATE,
                    })
                },
            );
        }
        DlssFixBindingState::SourceOnly
        | DlssFixBindingState::OwnedOnly
        | DlssFixBindingState::Bound => {}
    }

    let arch = initial.binding.arch.ok_or_else(commit::invalid_binding)?;
    let download = fetch::fetch_dlss_fix(arch, progress).await?;
    let guard =
        crate::mutation_boundary::enter_game_mutation_boundary_async(context, game_id).await?;
    let current = resolve_snapshot(context, game_id, false)?;
    ensure_same(&initial, &current)?;
    if current.binding.state == DlssFixBindingState::Invalid {
        return Err(commit::invalid_binding());
    }
    let after = tracking::rebuild_with_dlss_projection(
        &current.record,
        Some(Path::new(current.companion_path.as_str())),
        Some(source_from_download(arch, &download)),
        "DLSS-Fix active update",
    )?;
    emit_tool_finalizing(progress, AddonKind::RenoDx);
    crate::FileSafetyAuthority::new().authorize_game_commit(
        context,
        renderpilot_domain::mutation_features::RENODX_DLSS_FIX_UPDATE,
        &guard,
        &safety,
        || {
            commit_claim_or_physical(ActiveDlssCommit {
                context,
                guard: &guard,
                game_id,
                snapshot: &current,
                after,
                companion_effect: ActiveDlssEffect::Write(download.bytes),
                ini: None,
                feature: renderpilot_domain::mutation_features::RENODX_DLSS_FIX_UPDATE,
            })
        },
    )
}

pub(crate) fn uninstall(
    context: &Context,
    game_id: &GameId,
) -> Result<RenoDxInstallState, ServiceError> {
    let guard = crate::mutation_boundary::enter_game_mutation_boundary(context, game_id)?;
    let current = resolve_snapshot(context, game_id, false)?;
    match current.binding.state {
        DlssFixBindingState::Invalid => return Err(commit::invalid_binding()),
        DlssFixBindingState::None => {
            return Err(errors::invalid(
                "DLSS-Fix is not installed for this game".to_owned(),
            ));
        }
        DlssFixBindingState::SourceOnly
        | DlssFixBindingState::OwnedOnly
        | DlssFixBindingState::Bound => {}
    }
    let after = tracking::rebuild_with_dlss_projection(
        &current.record,
        None,
        None,
        "DLSS-Fix active uninstall",
    )?;
    let companion_effect = if matches!(
        current.binding.state,
        DlssFixBindingState::OwnedOnly | DlssFixBindingState::Bound
    ) {
        ActiveDlssEffect::Remove
    } else {
        ActiveDlssEffect::Unchanged
    };
    let ini_effect = remove_ini_effect(&current)?;
    commit_claim_or_physical(ActiveDlssCommit {
        context,
        guard: &guard,
        game_id,
        snapshot: &current,
        after,
        companion_effect,
        ini: ini_effect,
        feature: renderpilot_domain::mutation_features::RENODX_DLSS_FIX_UNINSTALL,
    })
}
