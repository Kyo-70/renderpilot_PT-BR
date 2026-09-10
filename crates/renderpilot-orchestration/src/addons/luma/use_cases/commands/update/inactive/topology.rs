use renderpilot_application::ProxyTopologyRepository;
use renderpilot_domain::GameId;

use crate::addons::luma::errors;
use crate::{Context, ServiceError};

/// Keeps the inactive route fail-closed if OptiScaler becomes active while
/// network preparation is running outside the game guard.
pub(super) fn ensure_absent(context: &Context, game_id: &GameId) -> Result<(), ServiceError> {
    if context.storage().get_proxy_topology(game_id)?.is_some() {
        return Err(errors::state_changed_retry_update());
    }
    Ok(())
}
