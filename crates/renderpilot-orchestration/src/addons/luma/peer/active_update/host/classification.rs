use renderpilot_domain::{
    GameProxyTopology, ManagedAddonFile, ManagedFileBaseline, PlannedGameProxyTopology, Version,
};

use crate::{
    addons::luma::peer::{
        active_host::{
            ActiveHostClassification, classify_active_host_from_validated_evidence,
            lower_active_host_owned,
        },
        effects::LumaPeerEffectAccumulator,
        root_authority::LumaPeerRootAuthority,
    },
    peer_mutation_executor::observe_peer_path_snapshot,
};

use super::{
    super::{
        error::LumaActiveUpdateError,
        model::{HostProjection, LumaActiveUpdateHostObservation},
    },
    validation::validate_owned_policy,
};
use crate::addons::luma::peer::active_update::model::LumaActiveUpdateHostInput;

pub(super) fn project_reused(
    topology: &GameProxyTopology,
    authority: &LumaPeerRootAuthority,
    input: LumaActiveUpdateHostInput,
    observation: LumaActiveUpdateHostObservation<'_>,
    minimum_version: &Version,
    accumulator: &mut LumaPeerEffectAccumulator,
) -> Result<HostProjection, LumaActiveUpdateError> {
    let (assessment, live_path, live_snapshot) = observation.into_parts();
    let prepared = match input {
        LumaActiveUpdateHostInput::Preserve => None,
        LumaActiveUpdateHostInput::Replace { bytes } => Some(bytes),
    };
    let classification = classify_active_host_from_validated_evidence(
        topology,
        assessment,
        live_path,
        live_snapshot,
        prepared,
        minimum_version,
    )?;
    match classification {
        ActiveHostClassification::Reused {
            binding,
            planned_topology,
        } => Ok(HostProjection::new(binding, planned_topology)),
        ActiveHostClassification::Owned(plan) => {
            let sidecar_path = plan.sidecar_path()?;
            let sidecar_snapshot =
                observe_peer_path_snapshot(&sidecar_path, authority.canonical_game_root_ref())
                    .map_err(|error| LumaActiveUpdateError::observation(sidecar_path, error))?;
            let binding = plan.binding().clone();
            let planned_topology = plan.planned_topology().clone();
            lower_active_host_owned(plan, live_snapshot, Some(&sidecar_snapshot), accumulator)?;
            Ok(HostProjection::new(binding, planned_topology))
        }
    }
}

pub(super) fn project_owned(
    topology: &GameProxyTopology,
    persisted: &ManagedAddonFile,
    input: LumaActiveUpdateHostInput,
    observation: LumaActiveUpdateHostObservation<'_>,
    minimum_version: &Version,
    accumulator: &mut LumaPeerEffectAccumulator,
) -> Result<HostProjection, LumaActiveUpdateError> {
    let (assessment, live_path, live_snapshot) = observation.into_parts();
    validate_owned_policy(assessment, &input)?;
    match input {
        LumaActiveUpdateHostInput::Preserve => Ok(HostProjection::new(
            persisted.clone(),
            PlannedGameProxyTopology::Exact(topology.clone()),
        )),
        LumaActiveUpdateHostInput::Replace { bytes } => {
            let prepared = crate::addons::luma::peer::active_host::validate_prepared_host(
                Some(&bytes),
                minimum_version,
            )?;
            let planned_digest =
                crate::addons::luma::peer::active_host::active_host_digest(prepared)?;
            if &planned_digest == persisted.installed_sha256() {
                return Ok(HostProjection::new(
                    persisted.clone(),
                    PlannedGameProxyTopology::Exact(topology.clone()),
                ));
            }
            if matches!(
                persisted.baseline(),
                ManagedFileBaseline::Present { sha256 } if sha256 == &planned_digest
            ) {
                return Err(LumaActiveUpdateError::invalid_input(
                    "owned ReShade replacement equals its persisted baseline",
                ));
            }

            let length = u64::try_from(prepared.len()).map_err(|_| {
                LumaActiveUpdateError::invalid_input("prepared ReShade host is too large")
            })?;
            let binding = ManagedAddonFile::owned(
                live_path.clone(),
                persisted.baseline().clone(),
                planned_digest.clone(),
            );
            let planned_topology = crate::addons::luma::peer::active_host::observed_topology(
                topology,
                live_path,
                planned_digest,
                length,
            );
            crate::addons::luma::peer::host::lower_host_decision(
                crate::addons::luma::peer::host::LumaHostDecision::Replace {
                    live_path,
                    live_snapshot,
                    prepared_bytes: bytes,
                },
                accumulator,
            )?;
            Ok(HostProjection::new(binding, planned_topology))
        }
    }
}
