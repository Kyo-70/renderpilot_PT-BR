use crate::addons::luma::peer::{
    active_update::error::LumaActiveUpdateError,
    dgvoodoo::{DgVoodooDecision, lower_dgvoodoo_decision},
    effects::LumaPeerEffectAccumulator,
};

use super::model::{Candidate, CandidateAction, ClassifiedCandidate, ObservedCandidate};

pub(super) fn lower_actions(
    actions: &[ClassifiedCandidate],
    candidates: &[Candidate],
    observed: &[ObservedCandidate],
    accumulator: &mut LumaPeerEffectAccumulator,
) -> Result<(), LumaActiveUpdateError> {
    for classified in actions {
        let candidate = &candidates[classified.index];
        let image = &observed[classified.index];
        let decision = match &classified.action {
            CandidateAction::Unchanged => DgVoodooDecision::UnchangedOrReused,
            CandidateAction::Create(bytes) => DgVoodooDecision::CreateManaged {
                live_path: &candidate.live,
                live_snapshot: &image.live,
                prepared_bytes: bytes.clone(),
            },
            CandidateAction::Replace(bytes) => DgVoodooDecision::ReplaceOwned {
                live_path: &candidate.live,
                live_snapshot: &image.live,
                prepared_bytes: bytes.clone(),
            },
            CandidateAction::RemoveCreated => DgVoodooDecision::ReleaseOwnedAbsent {
                live_path: &candidate.live,
                live_snapshot: &image.live,
            },
            CandidateAction::ReleaseBacked => DgVoodooDecision::ReleaseOwnedPresent {
                live_path: &candidate.live,
                sidecar_path: &candidate.sidecar,
                live_snapshot: &image.live,
                sidecar_snapshot: &image.sidecar,
            },
        };
        lower_dgvoodoo_decision(decision, accumulator)
            .map_err(LumaActiveUpdateError::dg_voodoo_lowering)?;
    }
    Ok(())
}
