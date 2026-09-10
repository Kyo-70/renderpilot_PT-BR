use renderpilot_domain::ManagedFileBaseline;

use crate::{
    addons::luma::peer::{
        effects::{LumaPeerEffectAccumulator, LumaPeerEffectGroup, ensure_bytes_match_image},
        host::{LumaHostDecision, lower_host_decision},
        snapshot_input::{require_absent, require_file, snapshot_bytes},
    },
    peer_mutation_executor::PeerPathSnapshot,
};

use super::model::{ActiveHostLoweringError, ActiveHostOwnedPlan, ActiveHostTransition};

/// Lowers a fresh `Owned/Absent` active host into one topology downstream
/// create. It never requests or observes a sidecar.
pub(super) fn lower_active_host_owned_create(
    plan: ActiveHostOwnedPlan,
    live_snapshot: &PeerPathSnapshot,
    accumulator: &mut LumaPeerEffectAccumulator,
) -> Result<(), ActiveHostLoweringError> {
    if !plan.is_create() {
        return Err(ActiveHostLoweringError::BindingMismatch(plan.target));
    }
    lower_host_decision(
        LumaHostDecision::Create {
            live_path: &plan.target,
            live_snapshot,
            prepared_bytes: plan.prepared_bytes,
        },
        accumulator,
    )?;
    Ok(())
}

/// Lowers the sole F4 `Owned/Present` active host acquisition. The supplied
/// sidecar snapshot must be absent; its payload is the exact retained live
/// image and is emitted immediately before the topology host replacement.
pub(super) fn lower_active_host_owned_replace(
    plan: ActiveHostOwnedPlan,
    live_snapshot: &PeerPathSnapshot,
    sidecar_snapshot: &PeerPathSnapshot,
    accumulator: &mut LumaPeerEffectAccumulator,
) -> Result<(), ActiveHostLoweringError> {
    let ActiveHostTransition::Replace { live_digest } = &plan.transition else {
        return Err(ActiveHostLoweringError::BindingMismatch(
            plan.target.clone(),
        ));
    };
    let sidecar_path = plan.sidecar_path()?;
    require_absent(&sidecar_path, sidecar_snapshot)?;
    let live_before = require_file(&plan.target, live_snapshot)?;
    let original_bytes = snapshot_bytes(&plan.target, live_snapshot)?;
    ensure_bytes_match_image(&plan.target, &original_bytes, live_before, false)
        .map_err(|_| ActiveHostLoweringError::SnapshotImageMismatch(plan.target.clone()))?;
    if live_before.digest() != live_digest
        || plan.binding.baseline()
            != &(ManagedFileBaseline::Present {
                sha256: live_digest.clone(),
            })
    {
        return Err(ActiveHostLoweringError::BindingMismatch(
            plan.target.clone(),
        ));
    }
    accumulator.acquire_foreign(
        LumaPeerEffectGroup::Host,
        plan.target.clone(),
        sidecar_path,
        live_before,
        original_bytes,
        plan.prepared_bytes,
    )?;
    Ok(())
}

/// Unified lowerer for callers that already split sidecar observation by
/// classification. A create passes `None`; a replacement passes its retained
/// absent sidecar snapshot.
pub(crate) fn lower_active_host_owned(
    plan: ActiveHostOwnedPlan,
    live_snapshot: &PeerPathSnapshot,
    sidecar_snapshot: Option<&PeerPathSnapshot>,
    accumulator: &mut LumaPeerEffectAccumulator,
) -> Result<(), ActiveHostLoweringError> {
    if plan.is_create() {
        if sidecar_snapshot.is_some() {
            return Err(ActiveHostLoweringError::SidecarNotApplicable);
        }
        lower_active_host_owned_create(plan, live_snapshot, accumulator)
    } else {
        lower_active_host_owned_replace(
            plan,
            live_snapshot,
            sidecar_snapshot.ok_or(ActiveHostLoweringError::SidecarNotApplicable)?,
            accumulator,
        )
    }
}
