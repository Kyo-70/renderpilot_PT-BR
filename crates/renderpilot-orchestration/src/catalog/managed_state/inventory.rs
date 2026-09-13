//! Stable inventory of managed state owned by one game.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use renderpilot_application::{
    ComponentRepository, InstalledAddonRepository, OptiScalerStateRepository,
};
use renderpilot_domain::{
    ComponentId, ComponentRollbackBaseline, GameId, InstalledAddon, LibraryComponent,
    OptiScalerInstallState,
};

use crate::ServiceError;

/// Complete read-only inventory used by root correction and removal.
#[derive(Debug, Clone)]
pub(in crate::catalog) struct ManagedGameStateInventory {
    /// Pending durable mutations that should normally be recovered on lock entry.
    pub pending_recovery_count: usize,
    /// Component rollback aggregates owned by the game.
    pub component_ids: Vec<ComponentId>,
    pub(super) component_baselines: BTreeMap<ComponentId, ComponentRollbackBaseline>,
    /// Baselines whose component row disappeared during a later scan.
    pub orphaned_component_ids: BTreeSet<ComponentId>,
    /// Installed add-on aggregate, when present.
    pub addon: Option<InstalledAddon>,
    /// Dedicated OptiScaler aggregate, when present.
    pub optiscaler_state: Option<OptiScalerInstallState>,
    /// A generic OptiScaler row is never a valid substitute for the dedicated
    /// aggregate. It is retained only so planning can reject the malformed
    /// inventory before any inverse action runs.
    pub malformed_optiscaler_addon: Option<InstalledAddon>,
    /// Number of driver-setting baselines owned by the game.
    pub nvapi_baseline_count: usize,
}

impl ManagedGameStateInventory {
    /// Whether removal needs any inverse action before deleting the card.
    pub(in crate::catalog) fn is_empty(&self) -> bool {
        self.pending_recovery_count == 0
            && self.component_ids.is_empty()
            && self.addon.is_none()
            && self.optiscaler_state.is_none()
            && self.malformed_optiscaler_addon.is_none()
            && self.nvapi_baseline_count == 0
    }
}

/// Captures all managed state in stable order.
pub(in crate::catalog) fn inventory(
    context: &crate::Context,
    game_id: &GameId,
) -> Result<ManagedGameStateInventory, ServiceError> {
    let storage = context.storage();
    let component_baselines = storage
        .component_backups_for_game(game_id)?
        .into_iter()
        .collect::<BTreeMap<_, _>>();
    let component_ids = component_baselines.keys().cloned().collect::<Vec<_>>();
    let current_components = storage.list_components_for_game(game_id)?;
    let current_component_ids: HashSet<&ComponentId> = current_components
        .iter()
        .map(LibraryComponent::id)
        .collect();
    let orphaned_component_ids = component_ids
        .iter()
        .filter(|component_id| !current_component_ids.contains(*component_id))
        .cloned()
        .collect();
    let installed_addon = storage.get_installed_addon(game_id)?;
    let (addon, malformed_optiscaler_addon) = match installed_addon {
        Some(addon) if addon.kind() == renderpilot_domain::AddonKind::OptiScaler => {
            (None, Some(addon))
        }
        other => (other, None),
    };
    Ok(ManagedGameStateInventory {
        pending_recovery_count: storage.pending_file_mutations_for_game(game_id)?.len(),
        component_ids,
        component_baselines,
        orphaned_component_ids,
        addon,
        optiscaler_state: storage.get_optiscaler_install_state(game_id)?,
        malformed_optiscaler_addon,
        nvapi_baseline_count: storage
            .list_nvapi_setting_baselines_for_game(game_id.as_str())?
            .len(),
    })
}
