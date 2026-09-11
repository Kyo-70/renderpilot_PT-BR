//! Installs RenoDX from upstream or from a user-selected add-on file.

mod active;
mod inactive;

use renderpilot_application::ProxyTopologyRepository;
use renderpilot_domain::{GameId, InstalledAddon};

use crate::Context;
use crate::addons::renodx::types::RenoDxManifest;
use crate::addons::reshade::types::{ReshadeChannel, ReshadeSourceCatalog};
use crate::net::ProgressObserver;

/// Shared parameters for a RenoDX install operation.
pub struct InstallRequest<'a> {
    /// Backend context (game repository, addon repository, settings).
    pub context: &'a Context,
    /// The resolved RenoDX tool catalogue.
    pub manifest: &'a RenoDxManifest,
    /// Independently resolved ReShade source catalogue.
    pub reshade_sources: &'a ReshadeSourceCatalog,
    /// The game to install RenoDX for.
    pub game_id: &'a GameId,
    /// The ReShade host channel to install (stable or nightly).
    pub requested_channel: ReshadeChannel,
    /// Typed game authority and optional shared-Vulkan authority.
    pub safety: crate::GameMutationSafetyPermits,
    /// Whether this caller permits installing the shared Vulkan layer when needed.
    pub allow_shared_vulkan_layer_install: bool,
    /// Optional download progress observer.
    pub progress: Option<&'a ProgressObserver<'a>>,
}

/// Installs RenoDX into `game`, fetching the add-on + ReShade from upstream and
/// persisting the record needed to reverse it.
///
/// The game permit must be fresh for the target game. A shared-Vulkan permit is
/// required when a Vulkan install will mutate
/// the shared layer. `allow_shared_vulkan_layer_install` must be `true` for a Vulkan game when
/// no ReShade Vulkan layer is present yet. The ReShade host (when one must be
/// installed) uses the requested channel. An unavailable explicit channel is
/// rejected rather than silently remapped.
///
/// Returns the `managed_app_record` (the per-game `InstalledAddon`).
///
/// Network prepare runs **outside** the per-game `game_mutation_lock` (same 3-phase
/// contract as Luma install) so a slow download does not block peer availability.
pub async fn install(request: InstallRequest<'_>) -> Result<InstalledAddon, crate::ServiceError> {
    match select_route(&request)? {
        InstallRoute::Active(topology) => active::install(request, *topology).await,
        InstallRoute::Inactive => inactive::install(request).await,
    }
}

/// Installs RenoDX from a user-downloaded add-on file — the manual path for any
/// DirectX game, whether or not the catalogue knows it.
///
/// Same engine and reversibility as [`install`]; the add-on bytes come from
/// `file_path` (validated as a PE) instead of an upstream download, and the record
/// tracks no upstream source. A curated *External* title is no longer a special
/// case: it just yields a richer plan, while any DirectX game falls back to a
/// generic "ReShade host + your add-on" plan. The renderer must be able to load a
/// proxy DLL (a confirmed Vulkan/OpenGL game is refused), and the add-on's
/// architecture must match the game's.
pub async fn install_from_file(
    request: InstallRequest<'_>,
    file_path: &str,
) -> Result<InstalledAddon, crate::ServiceError> {
    match select_route(&request)? {
        InstallRoute::Active(topology) => {
            active::install_from_file(request, file_path, *topology).await
        }
        InstallRoute::Inactive => inactive::install_from_file(request, file_path).await,
    }
}

enum InstallRoute {
    Active(Box<renderpilot_domain::GameProxyTopology>),
    Inactive,
}

fn select_route(request: &InstallRequest<'_>) -> Result<InstallRoute, crate::ServiceError> {
    let topology = request
        .context
        .storage()
        .get_proxy_topology(request.game_id)?;
    Ok(select_route_from_topology(topology))
}

fn select_route_from_topology(
    topology: Option<renderpilot_domain::GameProxyTopology>,
) -> InstallRoute {
    match topology {
        Some(topology) => InstallRoute::Active(Box::new(topology)),
        None => InstallRoute::Inactive,
    }
}

#[cfg(test)]
mod tests {
    use renderpilot_domain::{
        FileReceipt, GameId, GameProxyTopology, PathRef, ProxyImplementation, ProxyLink,
        ProxyRootPrestate, Sha256Hash,
    };

    use super::{InstallRoute, select_route_from_topology};

    fn topology() -> GameProxyTopology {
        let root_slot = PathRef::new("C:/Games/Test/ReShade64.dll").expect("root slot");
        GameProxyTopology {
            id: "optiscaler:test".to_owned(),
            game_id: GameId::new("manual:test").expect("game id"),
            root_slot: root_slot.clone(),
            outer: ProxyLink {
                implementation: ProxyImplementation::OptiScaler,
                path: root_slot,
                receipt: FileReceipt::owned(
                    "outer",
                    Sha256Hash::new("a".repeat(64)).expect("digest"),
                )
                .expect("receipt"),
            },
            downstream: None,
            downstream_origin: None,
            root_prestate: ProxyRootPrestate::Absent,
        }
    }

    #[test]
    fn topology_presence_selects_the_active_route() {
        assert!(matches!(
            select_route_from_topology(Some(topology())),
            InstallRoute::Active(_)
        ));
        assert!(matches!(
            select_route_from_topology(None),
            InstallRoute::Inactive
        ));
    }
}
