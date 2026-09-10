use crate::addons::luma::peer::{
    effects::LumaPeerEffectAccumulator,
    generic::{LumaGenericDecision, lower_generic_decision},
};

use super::{
    super::error::LumaActiveUpdateError,
    model::{Candidate, DecisionKind, ObservedCandidate, PlannedDecision},
};

pub(super) fn lower_decisions(
    decisions: &[PlannedDecision],
    candidates: &[Candidate],
    observed: &[ObservedCandidate],
    accumulator: &mut LumaPeerEffectAccumulator,
) -> Result<(), LumaActiveUpdateError> {
    for decision in decisions {
        let candidate = &candidates[decision.candidate];
        let observation = &observed[decision.candidate];
        let generic = match decision.kind {
            DecisionKind::Unchanged => LumaGenericDecision::Unchanged,
            DecisionKind::Create => LumaGenericDecision::Create {
                live_path: candidate.live(),
                live_snapshot: &observation.live,
                prepared_bytes: candidate.bytes().unwrap_or_default().to_vec(),
            },
            DecisionKind::AcquireForeign => LumaGenericDecision::AcquireForeign {
                live_path: candidate.live(),
                sidecar_path: candidate.sidecar(),
                live_snapshot: &observation.live,
                sidecar_snapshot: &observation.sidecar,
                prepared_bytes: candidate.bytes().unwrap_or_default().to_vec(),
            },
            DecisionKind::ReplaceOwned => LumaGenericDecision::ReplaceOwned {
                live_path: candidate.live(),
                live_snapshot: &observation.live,
                prepared_bytes: candidate.bytes().unwrap_or_default().to_vec(),
            },
            DecisionKind::RemoveCreated => LumaGenericDecision::RemoveCreated {
                live_path: candidate.live(),
                live_snapshot: &observation.live,
            },
            DecisionKind::ReleaseBacked => LumaGenericDecision::ReleaseBacked {
                live_path: candidate.live(),
                sidecar_path: candidate.sidecar(),
                live_snapshot: &observation.live,
                sidecar_snapshot: &observation.sidecar,
            },
        };
        lower_generic_decision(generic, accumulator).map_err(LumaActiveUpdateError::generic)?;
    }
    Ok(())
}
