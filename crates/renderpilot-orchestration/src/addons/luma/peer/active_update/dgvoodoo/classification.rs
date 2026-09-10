use std::str;

use renderpilot_domain::PathRef;

use crate::{
    addons::engine::MergeStrategy,
    addons::luma::peer::active_update::{
        error::LumaActiveUpdateError, model::LumaActiveUpdateClaimDelta,
    },
    peer_mutation_executor::PeerPathSnapshot,
};

use super::model::{
    Candidate, CandidateAction, CandidateKind, CandidatePayload, ClassifiedCandidate,
    ObservedCandidate,
};

pub(super) fn classify_candidates(
    candidates: &[Candidate],
    observed: &[ObservedCandidate],
) -> Result<(Vec<ClassifiedCandidate>, LumaActiveUpdateClaimDelta), LumaActiveUpdateError> {
    if candidates.len() != observed.len() {
        return Err(invalid(
            "dgVoodoo observation count does not match candidates",
        ));
    }
    let mut actions = Vec::with_capacity(candidates.len());
    let mut add_created = Vec::new();
    let mut remove_created = Vec::new();
    let mut remove_backed_up = Vec::new();

    for (index, (candidate, image)) in candidates.iter().zip(observed).enumerate() {
        let action = classify_one(candidate, image)?;
        match &action {
            CandidateAction::Create(_) if !candidate.created => {
                add_created.push(candidate.live.clone());
            }
            CandidateAction::RemoveCreated => remove_created.push(candidate.live.clone()),
            CandidateAction::ReleaseBacked => {
                remove_created.push(candidate.live.clone());
                remove_backed_up.push(candidate.live.clone());
            }
            CandidateAction::Unchanged
            | CandidateAction::Create(_)
            | CandidateAction::Replace(_) => {}
        }
        actions.push(ClassifiedCandidate { index, action });
    }

    let delta =
        LumaActiveUpdateClaimDelta::new(add_created, remove_created, Vec::new(), remove_backed_up);
    Ok((actions, delta))
}

fn classify_one(
    candidate: &Candidate,
    observed: &ObservedCandidate,
) -> Result<CandidateAction, LumaActiveUpdateError> {
    if candidate.is_removed() {
        require_file(&candidate.live, &observed.live)?;
        if candidate.backed {
            require_file(&candidate.sidecar, &observed.sidecar)?;
            return Ok(CandidateAction::ReleaseBacked);
        }
        require_absent(&candidate.sidecar, &observed.sidecar)?;
        return Ok(CandidateAction::RemoveCreated);
    }

    match &candidate.kind {
        CandidateKind::Desired(CandidatePayload::Runtime { bytes }) => {
            validate_sidecar(candidate, &observed.sidecar)?;
            match &observed.live {
                PeerPathSnapshot::Absent => Ok(CandidateAction::Create(bytes.clone())),
                PeerPathSnapshot::File(_) if !candidate.created => Err(invalid_detail(format!(
                    "new dgVoodoo runtime would take over an existing file: {}",
                    candidate.live
                ))),
                PeerPathSnapshot::File(_) if observed.live.bytes() == Some(bytes.as_slice()) => {
                    Ok(CandidateAction::Unchanged)
                }
                PeerPathSnapshot::File(_) => Ok(CandidateAction::Replace(bytes.clone())),
            }
        }
        CandidateKind::Desired(CandidatePayload::Config { default, sections }) => {
            classify_config(candidate, observed, default, sections)
        }
        CandidateKind::Removed => Err(invalid(
            "dgVoodoo removed candidate reached desired classification",
        )),
    }
}

fn classify_config(
    candidate: &Candidate,
    observed: &ObservedCandidate,
    default: &[u8],
    sections: &[crate::addons::engine::IniSection],
) -> Result<CandidateAction, LumaActiveUpdateError> {
    validate_sidecar(candidate, &observed.sidecar)?;
    let strategy = MergeStrategy::IniSetKeys {
        sections: sections.to_vec(),
    };
    match &observed.live {
        PeerPathSnapshot::Absent => {
            let base = if candidate.backed {
                sidecar_text(&candidate.sidecar, &observed.sidecar)?
            } else {
                text(&candidate.live, default)?
            };
            Ok(CandidateAction::Create(strategy.apply(base).into_bytes()))
        }
        PeerPathSnapshot::File(_) => {
            let current = observed.live.bytes().ok_or_else(|| {
                invalid_detail(format!("missing dgVoodoo config image: {}", candidate.live))
            })?;
            let current_text = str::from_utf8(current).map_err(|_| {
                invalid_detail(format!("dgVoodoo config is not UTF-8: {}", candidate.live))
            })?;
            let desired = strategy.apply(current_text).into_bytes();
            if desired == current {
                Ok(CandidateAction::Unchanged)
            } else {
                Ok(CandidateAction::Replace(desired))
            }
        }
    }
}

fn validate_sidecar(
    candidate: &Candidate,
    sidecar: &PeerPathSnapshot,
) -> Result<(), LumaActiveUpdateError> {
    if candidate.backed {
        require_file(&candidate.sidecar, sidecar).map(|_| ())
    } else {
        require_absent(&candidate.sidecar, sidecar)
    }
}

fn require_absent(
    path: &PathRef,
    snapshot: &PeerPathSnapshot,
) -> Result<(), LumaActiveUpdateError> {
    if matches!(snapshot, PeerPathSnapshot::Absent) {
        Ok(())
    } else {
        Err(invalid_detail(format!(
            "expected absent dgVoodoo endpoint: {path}"
        )))
    }
}

fn require_file<'a>(
    path: &PathRef,
    snapshot: &'a PeerPathSnapshot,
) -> Result<&'a crate::peer_mutation_executor::VerifiedPeerFile, LumaActiveUpdateError> {
    snapshot
        .file()
        .ok_or_else(|| invalid_detail(format!("expected present dgVoodoo endpoint: {path}")))
}

fn sidecar_text<'a>(
    path: &PathRef,
    snapshot: &'a PeerPathSnapshot,
) -> Result<&'a str, LumaActiveUpdateError> {
    let bytes = snapshot
        .bytes()
        .ok_or_else(|| invalid_detail(format!("missing dgVoodoo sidecar image: {path}")))?;
    str::from_utf8(bytes)
        .map_err(|_| invalid_detail(format!("dgVoodoo sidecar is not UTF-8: {path}")))
}

fn text<'a>(path: &PathRef, bytes: &'a [u8]) -> Result<&'a str, LumaActiveUpdateError> {
    str::from_utf8(bytes)
        .map_err(|_| invalid_detail(format!("dgVoodoo config default is not UTF-8: {path}")))
}

fn invalid(reason: &'static str) -> LumaActiveUpdateError {
    LumaActiveUpdateError::invalid_input(reason)
}

fn invalid_detail(reason: impl Into<String>) -> LumaActiveUpdateError {
    LumaActiveUpdateError::invalid_input_detail(reason.into())
}
