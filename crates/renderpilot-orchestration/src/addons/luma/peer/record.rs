use renderpilot_application::ProxyTopologyRepository;
use renderpilot_domain::{AddonKind, InstalledAddon};

use crate::game_mutation_lock::GameMutationGuard;
use crate::{Context, ServiceError};

/// Persists a Luma metadata refresh through the topology-aware route selected
/// from the exact topology image observed under `guard`.
pub(crate) fn commit_metadata(
    context: &Context,
    guard: &GameMutationGuard,
    before: &InstalledAddon,
    after: &InstalledAddon,
) -> Result<(), ServiceError> {
    validate_luma_images(guard, before, after)?;
    let topology = context.storage().get_proxy_topology(guard.game_id())?;
    renderpilot_domain::validate_peer_metadata_only(
        Some(before),
        Some(after),
        topology.as_ref(),
        topology.as_ref(),
    )
    .map_err(|error| {
        ServiceError::invalid_input(format!("Luma metadata transition rejected: {error}"))
    })?;

    context.peer_mutation_executor().commit_metadata_aggregate(
        guard,
        before,
        after,
        topology.as_ref(),
    )
}

fn validate_luma_images(
    guard: &GameMutationGuard,
    before: &InstalledAddon,
    after: &InstalledAddon,
) -> Result<(), ServiceError> {
    if guard.game_id() != before.game_id() || guard.game_id() != after.game_id() {
        return Err(ServiceError::invalid_input(
            "Luma metadata images must belong to the guarded game",
        ));
    }
    if before.kind() != AddonKind::Luma || after.kind() != AddonKind::Luma {
        return Err(ServiceError::invalid_input(
            "Luma metadata facade accepts only Luma peer records",
        ));
    }
    Ok(())
}
