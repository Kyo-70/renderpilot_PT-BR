use renderpilot_domain::{GameId, GameProxyTopology};

use crate::AppResult;

/// Persistence port for the proxy-chain topology of a game.
pub trait ProxyTopologyRepository: Send + Sync {
    /// Returns the proxy chain recorded for a game, if one exists.
    fn get_proxy_topology(&self, game_id: &GameId) -> AppResult<Option<GameProxyTopology>>;
}
