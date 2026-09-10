use crate::peer_mutation_executor::PeerPathSnapshot;

use crate::addons::luma::peer::{
    active_update::{error::LumaActiveUpdateError, model::LumaActiveUpdateClaimDelta},
    snapshot_input::{LumaSnapshotInputError, require_absent, require_file},
};

use super::model::{
    Candidate, ClassifiedPayload, DecisionKind, ObservedCandidate, PlannedDecision,
};

pub(super) fn classify_candidates(
    candidates: &[Candidate],
    observed: &[ObservedCandidate],
    main_addon: &renderpilot_domain::PathRef,
) -> Result<ClassifiedPayload, LumaActiveUpdateError> {
    if candidates.len() != observed.len() {
        return Err(LumaActiveUpdateError::invalid_input(
            "payload observation count does not match candidate count",
        ));
    }

    let mut decisions = Vec::with_capacity(candidates.len());
    let mut add_created = Vec::new();
    let mut remove_created = Vec::new();
    let mut add_backed_up = Vec::new();
    let mut remove_backed_up = Vec::new();
    let mut main_changed_path = None;

    for (index, (candidate, observation)) in candidates.iter().zip(observed).enumerate() {
        let decision = classify_one(candidate, observation)?;
        match decision {
            DecisionKind::Create => {
                if candidate.is_new() {
                    add_created.push(candidate.live().clone());
                }
                if candidate.is_main(main_addon) {
                    main_changed_path = Some(candidate.live().clone());
                }
            }
            DecisionKind::AcquireForeign => {
                add_created.push(candidate.live().clone());
                add_backed_up.push(candidate.live().clone());
                if candidate.is_main(main_addon) {
                    main_changed_path = Some(candidate.live().clone());
                }
            }
            DecisionKind::ReplaceOwned => {
                if candidate.is_main(main_addon) {
                    main_changed_path = Some(candidate.live().clone());
                }
            }
            DecisionKind::RemoveCreated => remove_created.push(candidate.live().clone()),
            DecisionKind::ReleaseBacked => {
                remove_created.push(candidate.live().clone());
                if let Some(backed_path) = candidate.backed_path() {
                    remove_backed_up.push(backed_path.clone());
                }
            }
            DecisionKind::Unchanged => {}
        }
        decisions.push(PlannedDecision {
            candidate: index,
            kind: decision,
        });
    }

    Ok(ClassifiedPayload {
        decisions,
        delta: LumaActiveUpdateClaimDelta::new(
            add_created,
            remove_created,
            add_backed_up,
            remove_backed_up,
        ),
        main_changed_path,
    })
}

fn classify_one(
    candidate: &Candidate,
    observation: &ObservedCandidate,
) -> Result<DecisionKind, LumaActiveUpdateError> {
    match candidate {
        Candidate::Fresh {
            retained_live,
            backed,
            bytes,
            ..
        } => {
            let retained = retained_live.is_some();
            match &observation.live {
                PeerPathSnapshot::Absent => {
                    if *backed {
                        require_file(candidate.sidecar(), &observation.sidecar)
                            .map_err(snapshot_error)?;
                    } else {
                        require_absent(candidate.sidecar(), &observation.sidecar)
                            .map_err(snapshot_error)?;
                    }
                    Ok(DecisionKind::Create)
                }
                PeerPathSnapshot::File(_) => {
                    if *backed {
                        require_file(candidate.sidecar(), &observation.sidecar)
                            .map_err(snapshot_error)?;
                    } else {
                        require_absent(candidate.sidecar(), &observation.sidecar)
                            .map_err(snapshot_error)?;
                    }
                    if retained && observation.live.bytes() == Some(bytes.as_slice()) {
                        Ok(DecisionKind::Unchanged)
                    } else if retained {
                        Ok(DecisionKind::ReplaceOwned)
                    } else {
                        Ok(DecisionKind::AcquireForeign)
                    }
                }
            }
        }
        Candidate::Removed { backed, .. } => {
            require_file(candidate.live(), &observation.live).map_err(snapshot_error)?;
            if *backed {
                require_file(candidate.sidecar(), &observation.sidecar).map_err(snapshot_error)?;
                Ok(DecisionKind::ReleaseBacked)
            } else {
                require_absent(candidate.sidecar(), &observation.sidecar)
                    .map_err(snapshot_error)?;
                Ok(DecisionKind::RemoveCreated)
            }
        }
    }
}

fn snapshot_error(error: LumaSnapshotInputError) -> LumaActiveUpdateError {
    LumaActiveUpdateError::generic(
        crate::addons::luma::peer::generic::LumaGenericLoweringError::Snapshot(error),
    )
}
