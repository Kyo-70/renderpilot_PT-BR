//! Status and update queries with no filesystem mutation.

use super::*;

/// Checks release identity and managed-file drift without mutating game files.
pub async fn check_update(
    context: &Context,
    manifest: &OptiScalerManifest,
    catalog: &OptiScalerCompatibilityCatalog,
    game_id: &GameId,
) -> Result<OptiScalerUpdateCheck, ServiceError> {
    let state = context.storage().get_optiscaler_install_state(game_id)?;
    let Some(state) = state else {
        return Ok(OptiScalerUpdateCheck {
            overall: crate::addons::update::UpdateStatus::Current,
            installed_release: None,
            available_release: manifest.current_release().map(|release| release.id.clone()),
            update_available: false,
            repair_required: false,
            drifted_paths: Vec::new(),
        });
    };
    let release = current_release(manifest)?;
    let availability = matcher::evaluate(context, manifest, catalog, game_id)?;
    Ok(OptiScalerUpdateCheck {
        overall: if availability.update_available {
            crate::addons::update::UpdateStatus::Available
        } else {
            crate::addons::update::UpdateStatus::Current
        },
        installed_release: Some(state.release_id),
        available_release: Some(release.id.clone()),
        // Catalogue revisions may change compatibility wording without changing
        // immutable release bytes, so release identity alone drives this flag.
        update_available: availability.update_available,
        repair_required: availability.repair_required,
        drifted_paths: availability
            .drifted_paths
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect(),
    })
}
