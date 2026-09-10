//! Phase-3 projection for the active Luma dgVoodoo dependency.
//!
//! This slice validates the complete ownership/source/candidate set before
//! observing any endpoint, classifies the retained images, and only then
//! lowers the owned transitions into the caller's effect accumulator.

mod classification;
mod guards;
mod lowering;
mod model;
mod observation;
mod validation;

use renderpilot_domain::{InstalledAddon, PathRef, TrackedSource};

use crate::addons::luma::{
    dgvoodoo::DgVoodooInstall,
    peer::{
        active_dgvoodoo::plan_active_dgvoodoo,
        active_update::{
            error::LumaActiveUpdateError,
            model::{
                DgVoodooProjection, LumaActiveUpdateClaimDelta, LumaActiveUpdateDgVoodooInput,
            },
        },
        effects::LumaPeerEffectAccumulator,
        root_authority::LumaPeerRootAuthority,
    },
};

/// Projects the active update's owned dgVoodoo claim delta.
pub(super) fn project_dgvoodoo(
    before: &InstalledAddon,
    authority: &LumaPeerRootAuthority,
    input: LumaActiveUpdateDgVoodooInput,
    dependency_paths: &[PathRef],
    refreshed_sources: &[TrackedSource],
    accumulator: &mut LumaPeerEffectAccumulator,
) -> Result<DgVoodooProjection, LumaActiveUpdateError> {
    // Preserve is intentionally inert: irrelevant dependency/source data must
    // not become observable merely because this phase was selected.
    if matches!(input, LumaActiveUpdateDgVoodooInput::Preserve) {
        return Ok(DgVoodooProjection::new(
            LumaActiveUpdateClaimDelta::default(),
        ));
    }

    let (created, backed, dependencies) =
        validation::validate_before_and_dependencies(before, authority, dependency_paths)?;
    let plan = match input {
        LumaActiveUpdateDgVoodooInput::Replace(prepared) => {
            validation::validate_replace_sources(&prepared, refreshed_sources)?;
            plan_active_dgvoodoo(authority, Some(DgVoodooInstall::Managed(*prepared)))
                .map_err(LumaActiveUpdateError::dg_voodoo_plan)?
        }
        LumaActiveUpdateDgVoodooInput::Remove => {
            validation::validate_remove_sources(refreshed_sources)?;
            None
        }
        LumaActiveUpdateDgVoodooInput::Preserve => {
            return Ok(DgVoodooProjection::new(
                LumaActiveUpdateClaimDelta::default(),
            ));
        }
    };

    let candidates =
        validation::build_candidates(before, authority, plan, &created, &backed, &dependencies)?;
    let observed = observation::observe_candidates(authority, &candidates)?;
    let (actions, delta) = classification::classify_candidates(&candidates, &observed)?;
    lowering::lower_actions(&actions, &candidates, &observed, accumulator)?;

    Ok(DgVoodooProjection::new(delta))
}

#[cfg(test)]
mod tests;
