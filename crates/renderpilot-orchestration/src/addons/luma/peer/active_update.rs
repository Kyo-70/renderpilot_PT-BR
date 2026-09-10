//! Decision-complete composition for an active-topology Luma update.

use std::path::Path;

use renderpilot_domain::{InstalledAddon, PathRef};

use crate::ServiceError;

mod compose;
mod dgvoodoo;
mod dlss;
mod error;
mod host;
mod model;
mod payload;
mod record;

pub(crate) use model::{
    LumaActiveUpdateAggregateMembership, LumaActiveUpdateComposition,
    LumaActiveUpdateDgVoodooInput, LumaActiveUpdateEvidence, LumaActiveUpdateHostInput,
    LumaActiveUpdateHostObservation, LumaActiveUpdateInput, LumaActiveUpdateMetadata,
    LumaActiveUpdatePayloadInput, LumaActiveUpdatePhysical, LumaActiveUpdatePrepared,
};

pub(crate) use compose::compose_active_update;

impl model::LumaActiveUpdatePayloadInput {
    /// Reports whether a full payload contains the exact top-level DLSS file.
    /// The path rule remains owned by the generic payload validator.
    pub(crate) fn contains_exact_dlss(&self) -> bool {
        let model::LumaActiveUpdatePayloadInput::Full(payload) = self else {
            return false;
        };
        payload.files.iter().any(|file| {
            super::active_payload::is_exact_dlss_relative(Path::new(&file.relative_path))
        })
    }
}

impl model::LumaActiveUpdatePrepared {
    /// Reuses the update DLSS validator for the phase-three cascade selector.
    /// The facade keeps the typed peer error inside the peer boundary.
    pub(crate) fn persisted_effective_binding_is_owned(
        before: &InstalledAddon,
        target: &PathRef,
    ) -> Result<bool, ServiceError> {
        dlss::persisted_effective_binding_is_owned(before, target).map_err(|error| {
            ServiceError::invalid_input(format!("active Luma DLSS binding rejected: {error}"))
        })
    }
}
