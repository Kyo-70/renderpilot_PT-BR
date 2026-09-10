//! Network-only preparation for an active-topology Luma update.
//!
//! The phase-one snapshot is the sole authority for local state. This module
//! only resolves remote artifacts and builds the immutable peer preparation;
//! mutation, persistence, and all filesystem ownership remain in later phases.

mod dgvoodoo;
mod host;
mod payload;
mod sources;

#[cfg(test)]
mod tests;

use crate::ServiceError;
use crate::addons::luma::dgvoodoo as managed_dgvoodoo;
use crate::addons::luma::fetch::download::fetch_luma_payload;
use crate::addons::luma::peer::{
    LumaActiveUpdateDgVoodooInput, LumaActiveUpdateHostInput, LumaActiveUpdatePayloadInput,
    LumaActiveUpdatePrepared,
};
use crate::addons::luma::tracking::resolved_addon_version;
use crate::addons::progress::sequential_stage_observer;
use crate::addons::reshade::fetch::fetch_reshade_from_source;
use crate::addons::reshade::source::require_reshade_source;
use crate::addons::reshade::types::{ReshadeChannel, ReshadeSourceCatalog};
use crate::net::ProgressObserver;

use super::model::ActiveUpdatePhase1;

/// Prepares remote artifacts for an active Luma update without touching local
/// state. The supplied phase-one snapshot must remain immutable until the
/// later guarded materialization phase consumes the result.
pub(super) async fn prepare_active_update(
    phase1: &ActiveUpdatePhase1,
    reshade_sources: &ReshadeSourceCatalog,
    force_full: bool,
    progress: Option<&ProgressObserver<'_>>,
) -> Result<LumaActiveUpdatePrepared, ServiceError> {
    let dependency_paths = sources::convert_dependency_paths(phase1.dependency_paths())?;
    let payload_plan = payload::plan(phase1, force_full).await?;
    let host_plan = host::plan(phase1)?;
    let dgvoodoo_plan = dgvoodoo::plan(phase1, payload_plan.is_full())?;

    let payload_download = payload_plan.is_full();
    let host_download = host_plan.is_replace();
    let dgvoodoo_download = dgvoodoo_plan.is_replace();
    let stage_count =
        u64::from(payload_download) + u64::from(host_download) + u64::from(dgvoodoo_download);

    let mut stage_index = 0;
    let mut tracked_sources = phase1.record().tracked_sources().to_vec();

    let (payload, addon_version) = if payload_download {
        let stage_observer = sequential_stage_observer(progress, stage_index, stage_count);
        let stage_progress = stage_observer
            .as_ref()
            .map(|observer| observer as &ProgressObserver<'_>);
        stage_index += 1;
        let payload = fetch_luma_payload(
            &phase1.target().asset,
            &phase1.target().addon_file,
            phase1.target().arch,
            stage_progress,
        )
        .await?;
        sources::replace_payload(&mut tracked_sources, &phase1.target().asset, &payload)?;
        let addon_version = resolved_addon_version(phase1.record(), &payload);
        (LumaActiveUpdatePayloadInput::Full(payload), addon_version)
    } else {
        (
            LumaActiveUpdatePayloadInput::Preserve,
            phase1.record().addon_version().map(str::to_owned),
        )
    };

    let host = if host_download {
        let source = require_reshade_source(
            reshade_sources,
            ReshadeChannel::Nightly,
            phase1.target().arch,
        )?;
        let stage_observer = sequential_stage_observer(progress, stage_index, stage_count);
        let stage_progress = stage_observer
            .as_ref()
            .map(|observer| observer as &ProgressObserver<'_>);
        stage_index += 1;
        let downloaded =
            fetch_reshade_from_source(&source, phase1.target().arch, stage_progress).await?;
        let tracked = crate::addons::reshade::update::host_binary_source(
            source.url,
            downloaded.etag,
            downloaded.digest,
            downloaded.last_modified,
            Some(ReshadeChannel::Nightly),
        );
        sources::replace_host(&mut tracked_sources, tracked)?;
        LumaActiveUpdateHostInput::Replace {
            bytes: downloaded.bytes,
        }
    } else {
        LumaActiveUpdateHostInput::Preserve
    };

    let dgvoodoo = match dgvoodoo_plan {
        dgvoodoo::Plan::Preserve => LumaActiveUpdateDgVoodooInput::Preserve,
        dgvoodoo::Plan::Remove => {
            sources::remove_role(
                &mut tracked_sources,
                renderpilot_domain::TrackedSourceRole::DgVoodooWrapper,
            );
            LumaActiveUpdateDgVoodooInput::Remove
        }
        dgvoodoo::Plan::Replace => {
            let requirement =
                managed_dgvoodoo::requirement(phase1.target().external_requirement.as_ref())
                    .ok_or_else(|| {
                        invalid("active Luma dgVoodoo replacement has no requirement")
                    })?;
            let stage_observer = sequential_stage_observer(progress, stage_index, stage_count);
            let stage_progress = stage_observer
                .as_ref()
                .map(|observer| observer as &ProgressObserver<'_>);
            stage_index += 1;
            let prepared = managed_dgvoodoo::fetch(requirement, stage_progress).await?;
            sources::replace_dgvoodoo(&mut tracked_sources, prepared.tracked_source())?;
            LumaActiveUpdateDgVoodooInput::Replace(Box::new(prepared))
        }
    };
    debug_assert_eq!(stage_index, stage_count);

    Ok(LumaActiveUpdatePrepared::new(
        payload,
        host,
        dgvoodoo,
        dependency_paths,
        tracked_sources,
        addon_version,
    ))
}

fn invalid(message: impl Into<String>) -> ServiceError {
    ServiceError::invalid_input(message)
}
