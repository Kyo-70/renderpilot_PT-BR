use renderpilot_domain::{
    AddonKind, GameId, InstalledAddon, InstalledAddonHostKind, InstalledAddonParts,
    ManagedAddonFile, PathRef, TrackedSource, TrackedSourceRole,
};

use super::error::LumaActiveInstallCompositionError;
use crate::addons::luma::peer::active_dgvoodoo::ActiveDgVoodooRecordProjection;
use crate::addons::luma::peer::active_payload::ActivePayloadProjection;

pub(super) struct RecordInput {
    pub(super) game_id: GameId,
    pub(super) addon_source_url: String,
    pub(super) addon_source_etag: Option<String>,
    pub(super) addon_source_last_modified: Option<String>,
    pub(super) addon_zip_digest: String,
    pub(super) addon_version: Option<String>,
    pub(super) main_addon: PathRef,
    pub(super) reshade_source_url: String,
    pub(super) reshade_source_etag: Option<String>,
    pub(super) reshade_source_last_modified: Option<String>,
    pub(super) reshade_digest: String,
    pub(super) payload: ActivePayloadProjection,
    pub(super) dgvoodoo: ActiveDgVoodooRecordProjection,
    pub(super) host: ManagedAddonFile,
    pub(super) dlss: Option<ManagedAddonFile>,
    pub(super) host_owned: bool,
}

pub(super) fn build_record(
    input: RecordInput,
) -> Result<InstalledAddon, LumaActiveInstallCompositionError> {
    let RecordInput {
        game_id,
        addon_source_url,
        addon_source_etag,
        addon_source_last_modified,
        addon_zip_digest,
        addon_version,
        main_addon,
        reshade_source_url,
        reshade_source_etag,
        reshade_source_last_modified,
        reshade_digest,
        payload,
        dgvoodoo,
        host,
        dlss,
        host_owned,
    } = input;
    if addon_source_url.trim().is_empty() {
        return Err(LumaActiveInstallCompositionError::InvalidInput(
            "add-on source URL is empty",
        ));
    }
    let zip_digest = renderpilot_domain::Sha256Hash::new(addon_zip_digest).map_err(|_| {
        LumaActiveInstallCompositionError::InvalidInput("add-on ZIP digest is not valid SHA-256")
    })?;

    let mut created_files = payload.created_files().to_vec();
    created_files.extend(dgvoodoo.created_files().iter().cloned());
    let mut backed_up_files = payload.backed_up_files().to_vec();
    backed_up_files.extend(dgvoodoo.backed_up_files().iter().cloned());
    let mut managed_files = vec![host];
    if let Some(dlss) = dlss {
        managed_files.push(dlss);
    }

    let mut tracked_sources = vec![
        TrackedSource::new(
            TrackedSourceRole::AddonPayload,
            addon_source_url,
            addon_source_etag,
            zip_digest.as_str(),
        )
        .with_last_modified(addon_source_last_modified),
    ];
    if host_owned {
        if reshade_source_url.trim().is_empty() {
            return Err(LumaActiveInstallCompositionError::InvalidInput(
                "owned ReShade host source URL is empty",
            ));
        }
        tracked_sources.push(
            TrackedSource::new(
                TrackedSourceRole::HostBinary,
                reshade_source_url,
                reshade_source_etag,
                reshade_digest,
            )
            .with_last_modified(reshade_source_last_modified)
            .with_channel("nightly"),
        );
    }
    if let Some(source) = dgvoodoo.tracked_source().cloned() {
        if source.url().trim().is_empty() {
            return Err(LumaActiveInstallCompositionError::InvalidInput(
                "managed dgVoodoo source URL is empty",
            ));
        }
        renderpilot_domain::Sha256Hash::new(source.digest()).map_err(|_| {
            LumaActiveInstallCompositionError::InvalidInput(
                "managed dgVoodoo source digest is not valid SHA-256",
            )
        })?;
        tracked_sources.push(source);
    }

    let record = InstalledAddon::from_parts_with_managed(InstalledAddonParts {
        game_id,
        kind: AddonKind::Luma,
        addon_file: main_addon,
        addon_version,
        created_files,
        backed_up_files,
        managed_files,
        tracked_sources,
    })
    .map_err(|error| LumaActiveInstallCompositionError::Record(error.to_string()))?
    .ok_or_else(|| {
        LumaActiveInstallCompositionError::Record(
            "payload main add-on is not listed among created files".to_owned(),
        )
    })?
    .with_host_kind(InstalledAddonHostKind::Proxy);

    if host_owned {
        Ok(record.with_reshade_channel("nightly"))
    } else {
        Ok(record)
    }
}
