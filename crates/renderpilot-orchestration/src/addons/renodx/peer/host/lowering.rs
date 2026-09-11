use super::super::effects::RenoDxPeerEffectAccumulator;
use super::error::RenoDxHostError;
use super::model::RenoDxOwnedHostPlan;
use crate::peer_mutation_executor::PeerPathSnapshot;

/// Emits one selected owned host transition. The sidecar snapshot is supplied
/// by phase one for replacement and is never observed here.
pub(crate) fn lower_owned_host(
    plan: RenoDxOwnedHostPlan,
    live_snapshot: &PeerPathSnapshot,
    sidecar_snapshot: Option<&PeerPathSnapshot>,
    accumulator: &mut RenoDxPeerEffectAccumulator,
) -> Result<(), RenoDxHostError> {
    if plan.is_create() {
        if sidecar_snapshot.is_some() {
            return Err(RenoDxHostError::Assessment(
                "fresh RenoDX host create cannot carry a sidecar snapshot",
            ));
        }
        super::validation::require_absent(plan.target(), live_snapshot)?;
        accumulator.create(
            super::super::effects::RenoDxPeerEffectGroup::Host,
            plan.target().clone(),
            plan.prepared_bytes(),
        )?;
        return Ok(());
    }

    let sidecar_snapshot = sidecar_snapshot.ok_or(RenoDxHostError::Assessment(
        "RenoDX host replacement requires its retained sidecar snapshot",
    ))?;
    let sidecar_path = plan
        .sidecar_path()
        .map_err(|error| RenoDxHostError::Sidecar(crate::failed(error.to_string())))?;
    super::validation::require_absent(&sidecar_path, sidecar_snapshot)?;
    let live_before = super::validation::require_snapshot_file(plan.target(), live_snapshot)?;
    let original_bytes = live_snapshot.bytes().ok_or_else(|| {
        RenoDxHostError::Evidence(plan.target().clone(), "present host snapshot has no bytes")
    })?;
    accumulator.acquire_host(
        plan.target().clone(),
        sidecar_path,
        live_before,
        original_bytes.to_vec(),
        plan.prepared_bytes(),
    )?;
    Ok(())
}
