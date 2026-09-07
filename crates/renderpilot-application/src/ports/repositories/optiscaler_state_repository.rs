use renderpilot_domain::{GameId, OptiScalerInstallState};

use crate::AppResult;

/// Persistence port for the typed OptiScaler lifecycle aggregate.
pub trait OptiScalerStateRepository: Send + Sync {
    /// Returns the typed OptiScaler state for a game, if installed.
    fn get_optiscaler_install_state(
        &self,
        game_id: &GameId,
    ) -> AppResult<Option<OptiScalerInstallState>>;

    /// Returns every typed OptiScaler state in stable game-id order.
    ///
    /// OptiScaler is not represented by an `InstalledAddon` row, so callers
    /// performing bulk checks or update enumeration must use this port.
    fn list_optiscaler_install_states(&self) -> AppResult<Vec<OptiScalerInstallState>>;
}
