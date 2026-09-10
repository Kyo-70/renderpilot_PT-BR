//! Generic payload projection for one active Luma update.
//!
//! This facade enforces the phase boundary: validate every fallible path and
//! candidate fact, observe each selected endpoint once, classify the complete
//! image, and only then lower effects.

mod classification;
mod lowering;
mod model;
mod observation;
mod validation;

use renderpilot_domain::{InstalledAddon, PathRef, TrackedSource};

use crate::addons::luma::peer::active_payload;

use super::{
    error::LumaActiveUpdateError,
    model::{
        LumaActiveUpdateClaimDelta, LumaActiveUpdateDlssInput, LumaActiveUpdateMtime,
        LumaActiveUpdatePayloadInput, PayloadProjection, PayloadRecordProjection,
    },
};
use crate::addons::luma::peer::{
    effects::LumaPeerEffectAccumulator, root_authority::LumaPeerRootAuthority,
};

/// Projects the generic payload portion of an active Luma update.
///
pub(super) fn project_payload(
    before: &InstalledAddon,
    authority: &LumaPeerRootAuthority,
    input: LumaActiveUpdatePayloadInput,
    dependency_paths: &[PathRef],
    refreshed_sources: &[TrackedSource],
    accumulator: &mut LumaPeerEffectAccumulator,
) -> Result<PayloadProjection, LumaActiveUpdateError> {
    let LumaActiveUpdatePayloadInput::Full(payload) = input else {
        return Ok(preserve_projection(before));
    };

    validation::validate_before_record(before)?;
    validation::validate_payload_source(&payload, refreshed_sources)?;

    let validated =
        active_payload::validate_active_payload(authority, payload.files, &payload.main_addon_rel)
            .map_err(LumaActiveUpdateError::payload)?;
    let (validated_main, targets, dlss_bytes) = validated.into_parts();
    let targets = targets
        .into_iter()
        .map(model::FreshTarget::from_validated)
        .collect();

    let plan = validation::build_candidate_plan(before, authority, dependency_paths, targets)?;
    let observed = observation::observe_candidates(authority, &plan.candidates)?;
    let classified =
        classification::classify_candidates(&plan.candidates, &observed, &validated_main)?;
    lowering::lower_decisions(
        &classified.decisions,
        &plan.candidates,
        &observed,
        accumulator,
    )?;

    let main_addon = validation::projected_main_addon(before, &validated_main);
    let mtime = classified
        .main_changed_path
        .map(|path| LumaActiveUpdateMtime::new(path, payload.last_modified.clone()));
    Ok(PayloadProjection::new(
        PayloadRecordProjection::new(main_addon, classified.delta),
        LumaActiveUpdateDlssInput::Full {
            bundled_bytes: dlss_bytes,
        },
        mtime,
    ))
}

fn preserve_projection(before: &InstalledAddon) -> PayloadProjection {
    PayloadProjection::new(
        PayloadRecordProjection::new(
            before.addon_file().clone(),
            LumaActiveUpdateClaimDelta::default(),
        ),
        LumaActiveUpdateDlssInput::Preserve,
        None,
    )
}

#[cfg(test)]
mod tests;
