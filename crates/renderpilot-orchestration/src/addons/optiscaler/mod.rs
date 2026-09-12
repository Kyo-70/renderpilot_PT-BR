//! Manifest-driven OptiScaler integration.

mod archive;
mod artifact_descriptor;
pub(crate) mod compatibility_catalog;
pub(crate) mod config;
mod evaluation;
mod facade;
pub(crate) mod identity;
mod lifecycle;
pub(crate) mod manifest_store;
mod matcher;
mod module_artifact;
mod source;
pub(crate) mod tool;
pub mod types;

pub use facade::{
    InstallOptiScalerRequest, RelocateOptiScalerRequest, SetOptiScalerModulesRequest,
    UpdateOptiScalerRequest, availability, check_update, check_updates, install, relocate, repair,
    set_modules, status, uninstall, update,
};

/// Executes the already-planned OptiScaler inverse while the catalog removal
/// boundary owns the game lock. Generic managed cleanup uses this adapter so
/// OptiScaler remains on its typed state/topology transaction path.
pub(crate) fn uninstall_locked(
    context: &crate::Context,
    game_id: &renderpilot_domain::GameId,
    guard: &crate::game_mutation_lock::GameMutationGuard,
) -> Result<types::OptiScalerOperationResult, crate::ServiceError> {
    lifecycle::uninstall_locked(context, game_id, guard)
}

pub(crate) use lifecycle::OptiScalerManagedCleanupFootprint;

pub(crate) fn preflight_managed_cleanup_uninstall_locked(
    context: &crate::Context,
    game_id: &renderpilot_domain::GameId,
    guard: &crate::game_mutation_lock::GameMutationGuard,
) -> Result<OptiScalerManagedCleanupFootprint, crate::ServiceError> {
    lifecycle::preflight_managed_cleanup_uninstall_locked(context, game_id, guard)
}
/// Evaluates install eligibility without running filesystem/PE inspection on
/// an async runtime worker.
pub(crate) async fn availability_with_manifest(
    context: &crate::Context,
    manifest: &types::OptiScalerManifest,
    game_id: &renderpilot_domain::GameId,
    manual_override: bool,
) -> Result<types::OptiScalerAvailability, crate::ServiceError> {
    let availability =
        matcher::evaluation_off_runtime(context, manifest, game_id, manual_override).await?;
    // Like RenoDX/Luma, reconcile a recoverable local installation while
    // loading availability. This is a database-only adoption: no game file
    // is rewritten. A known ReShade downstream host is recorded
    // conservatively; an unknown release remains visible as unmanaged and is
    // never guessed.
    if availability.unmanaged {
        let adopted = lifecycle::adopt_exact(
            context,
            manifest,
            game_id,
            lifecycle::AdoptionPolicy::Reconcile,
            manual_override,
            &availability,
        )
        .await?;
        if adopted.is_some() || stored_status(context, game_id)?.is_some() {
            return matcher::evaluation_off_runtime(context, manifest, game_id, manual_override)
                .await
                .map(evaluation::EvaluatedAvailability::into_wire);
        }
    }
    Ok(availability.into_wire())
}

pub(crate) fn stored_status(
    context: &crate::Context,
    game_id: &renderpilot_domain::GameId,
) -> Result<Option<renderpilot_domain::OptiScalerInstallState>, crate::ServiceError> {
    use renderpilot_application::OptiScalerStateRepository;
    Ok(context.storage().get_optiscaler_install_state(game_id)?)
}

pub(crate) fn managed_state_endpoint_roots(
    state: &renderpilot_domain::OptiScalerInstallState,
) -> Vec<(std::path::PathBuf, std::path::PathBuf)> {
    lifecycle::managed_state_endpoint_roots(state)
}

/// Checks every tracked OptiScaler install against one resolved manifest.
pub(crate) async fn check_updates_with_manifest(
    context: &crate::Context,
    manifest: &types::OptiScalerManifest,
) -> Result<Vec<(renderpilot_domain::GameId, types::OptiScalerUpdateCheck)>, crate::ServiceError> {
    use renderpilot_application::OptiScalerStateRepository;
    let games = context.storage().list_games()?;
    let mut reports = Vec::new();
    for game in games {
        let game_id = game.id().clone();
        if context
            .storage()
            .get_optiscaler_install_state(&game_id)?
            .is_none()
        {
            continue;
        }
        reports.push((
            game_id.clone(),
            lifecycle::check_update(context, manifest, &game_id).await?,
        ));
    }
    Ok(reports)
}

pub(crate) const PHASE_VERIFYING: &str = "optiscaler.phase.verifying";
pub(crate) const PHASE_FINALIZING: &str = "optiscaler.phase.finalizing";
