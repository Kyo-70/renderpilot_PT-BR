mod auxiliary;
mod claims;
mod context;
mod relocation;
mod retained_fsr;
mod transitions;

use super::super::*;
use auxiliary::apply_auxiliary_preservations;
use context::BindingContext;
use relocation::apply_peer_relocation;

pub(in crate::repositories::game_mutations) fn build_optiscaler_binding(
    before_state: Option<&OptiScalerInstallState>,
    after_state: Option<&OptiScalerInstallState>,
    before_topology: Option<&GameProxyTopology>,
    after_topology: Option<&GameProxyTopology>,
    peer: PeerAggregateBinding<'_>,
    auxiliary: &[OptiScalerAuxiliaryPreservation],
    retained_claims: &[OptiScalerRetainedClaim],
) -> AppResult<pending_file_mutations::OptiScalerAggregateBinding> {
    let mut context = BindingContext::new(
        before_state,
        after_state,
        before_topology,
        after_topology,
        peer,
    )?;
    apply_auxiliary_preservations(&mut context, auxiliary)?;
    apply_peer_relocation(&mut context)?;
    transitions::apply_transitions(&mut context, retained_claims)?;

    Ok(pending_file_mutations::OptiScalerAggregateBinding {
        paths: context.paths,
        after_claims: context.after_claims,
        known_paths: context.known_paths,
        retained_fsr_custody: context.retained_fsr_custody,
        auxiliary: auxiliary
            .iter()
            .map(
                |preservation| pending_file_mutations::OptiScalerAuxiliaryPreservation {
                    source: preservation.source.as_str().to_owned(),
                    destination: preservation.destination.as_str().to_owned(),
                    receipt: preservation.destination_receipt.clone(),
                },
            )
            .collect(),
        owned_preservations: context.owned_preservations,
    })
}
