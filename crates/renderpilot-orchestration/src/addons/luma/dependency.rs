//! Durable OptiScaler-to-Luma dependency boundary.
//!
//! This deliberately models only the one accepted OptiScaler prerequisite;
//! it is not a generic add-on dependency graph.

use renderpilot_application::OptiScalerStateRepository;
use renderpilot_domain::{AddonKind, GameId, OptiScalerPrerequisiteBinding};

use crate::{Context, ServiceError};

/// Returns the installed add-on that currently prevents a Luma uninstall.
/// Invalid persisted state propagates as a storage error and therefore fails
/// closed rather than permitting a destructive lifecycle operation.
pub(crate) fn uninstall_blocker(
    context: &Context,
    game_id: &GameId,
) -> Result<Option<AddonKind>, ServiceError> {
    Ok(context
        .storage()
        .get_optiscaler_install_state(game_id)?
        .filter(|state| state.prerequisite_binding == OptiScalerPrerequisiteBinding::Luma)
        .map(|_| AddonKind::OptiScaler))
}

/// Enforces the persisted prerequisite before any Luma uninstall planning or
/// filesystem work begins.
pub(crate) fn ensure_uninstall_allowed(
    context: &Context,
    game_id: &GameId,
) -> Result<(), ServiceError> {
    if uninstall_blocker(context, game_id)?.is_some() {
        return Err(ServiceError::luma_required_by_optiscaler());
    }
    Ok(())
}
