//! Persisted host projection for one active-topology Luma update.
//!
//! The host claim and the topology receipt are one authority boundary. This
//! facade validates both before selecting the ownership-specific projection.

mod classification;
mod validation;

use renderpilot_domain::{GameProxyTopology, InstalledAddon, Version};

use crate::addons::luma::peer::{
    active_host::validate_active_host_evidence, effects::LumaPeerEffectAccumulator,
    root_authority::LumaPeerRootAuthority,
};

use super::{
    error::LumaActiveUpdateError,
    model::{LumaActiveUpdateHostInput, LumaActiveUpdateHostObservation},
};

/// Projects the persisted ReShade downstream of an active Luma topology.
///
/// All supplied evidence is validated before the accumulator is changed. The
/// only additional observation is the exact managed sidecar required by a
/// policy-selected Reused-to-Owned repair.
pub(super) fn project_host(
    before: &InstalledAddon,
    topology: &GameProxyTopology,
    authority: &LumaPeerRootAuthority,
    input: LumaActiveUpdateHostInput,
    observation: LumaActiveUpdateHostObservation<'_>,
    minimum_version: &Version,
    accumulator: &mut LumaPeerEffectAccumulator,
) -> Result<super::model::HostProjection, LumaActiveUpdateError> {
    let (assessment, live_path, live_snapshot) = observation.into_parts();
    validate_active_host_evidence(authority, topology, assessment, live_path, live_snapshot)?;
    let persisted = validation::validate_persisted_host_claim(before, topology, live_path)?;
    validation::validate_live_receipt(persisted, topology, live_path, live_snapshot)?;

    match persisted.mode() {
        renderpilot_domain::ManagedFileMode::Reused => classification::project_reused(
            topology,
            authority,
            input,
            observation,
            minimum_version,
            accumulator,
        ),
        renderpilot_domain::ManagedFileMode::Owned => classification::project_owned(
            topology,
            persisted,
            input,
            observation,
            minimum_version,
            accumulator,
        ),
    }
}

#[cfg(test)]
mod tests;
