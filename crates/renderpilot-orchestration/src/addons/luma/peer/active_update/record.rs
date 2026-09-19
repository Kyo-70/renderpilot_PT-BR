//! Pure reconstruction of the persisted Luma record after an active update.
//!
//! The composer supplies already validated projections.  This boundary only
//! merges their exact claim deltas and rebuilds one canonical
//! [`InstalledAddon`]; it does not choose a route or inspect the filesystem.

mod claims;
mod managed;
mod provenance;

#[cfg(test)]
mod tests;

use renderpilot_domain::{
    AddonKind, InstalledAddon, InstalledAddonHostKind, InstalledAddonParts, ManagedAddonFile,
    ManagedFileMode, TrackedSource,
};

use super::{
    error::LumaActiveUpdateError,
    model::{DgVoodooProjection, DlssProjection, PayloadRecordProjection},
};

/// Rebuilds the durable Luma record from immutable active-update projections.
///
/// Every filesystem-facing decision has already been made by the projections.
/// This function deliberately consumes them, so no later phase can silently
/// re-derive ownership from mutable state.
pub(super) fn rebuild_record(
    before: &InstalledAddon,
    payload: PayloadRecordProjection,
    dgvoodoo: DgVoodooProjection,
    host_binding: &ManagedAddonFile,
    dlss: DlssProjection,
    tracked_sources: Vec<TrackedSource>,
    addon_version: Option<String>,
) -> Result<InstalledAddon, LumaActiveUpdateError> {
    validate_before(before)?;

    let (addon_file, payload_delta) = payload.into_parts();
    let dgvoodoo_delta = dgvoodoo.into_claims();
    let (created_files, backed_up_files) =
        claims::merge(before, payload_delta, dgvoodoo_delta, &addon_file)?;

    let host_owned = host_binding.mode() == ManagedFileMode::Owned;
    let dlss_binding = dlss.into_binding();
    let managed_files = managed::rebuild(before, host_binding, dlss_binding)?;
    provenance::validate(&managed_files, &tracked_sources)?;
    claims::validate_managed_disjoint(&created_files, &backed_up_files, &managed_files)?;

    let record = InstalledAddon::from_parts_with_managed(InstalledAddonParts {
        game_id: before.game_id().clone(),
        kind: AddonKind::Luma,
        addon_file,
        addon_version,
        created_files,
        backed_up_files,
        managed_files,
        tracked_sources,
        renodx_config_receipt: None,
        engine_config_journal: before.engine_config_journal().cloned(),
    })
    .map_err(|error| LumaActiveUpdateError::invalid_input_detail(error.to_string()))?
    .ok_or_else(|| {
        LumaActiveUpdateError::invalid_input(
            "active Luma record main add-on is not a generic created claim",
        )
    })?
    .with_timestamps(before.installed_at(), before.updated_at())
    .with_host_kind(InstalledAddonHostKind::Proxy);

    Ok(if host_owned {
        record.with_reshade_channel("nightly")
    } else {
        record
    })
}

fn validate_before(before: &InstalledAddon) -> Result<(), LumaActiveUpdateError> {
    if before.kind() != AddonKind::Luma {
        return Err(LumaActiveUpdateError::invalid_input(
            "active Luma update requires a Luma record",
        ));
    }
    if !before.created_files().iter().any(|path| {
        renderpilot_domain::normalized_path_key(path.as_str())
            == renderpilot_domain::normalized_path_key(before.addon_file().as_str())
    }) {
        return Err(LumaActiveUpdateError::invalid_input(
            "active Luma record does not claim its main add-on in generic created files",
        ));
    }
    Ok(())
}
