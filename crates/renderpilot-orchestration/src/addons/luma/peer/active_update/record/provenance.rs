use renderpilot_domain::{
    ManagedAddonFile, ManagedFileMode, Sha256Hash, TrackedSource, TrackedSourceRole,
};

use crate::addons::luma::peer::active_update::error::LumaActiveUpdateError;

pub(super) fn validate(
    managed: &[ManagedAddonFile],
    sources: &[TrackedSource],
) -> Result<(), LumaActiveUpdateError> {
    let host = managed
        .iter()
        .find(|binding| {
            binding.path().file_name().is_none_or(|name| {
                !name.eq_ignore_ascii_case(renderpilot_detection::NVNGX_DLSS_FILE_NAME)
            })
        })
        .ok_or_else(|| invalid("active Luma record has no projected host binding"))?;
    let mut roles = Vec::new();
    for source in sources {
        if roles.contains(&source.role()) {
            return Err(invalid_detail(format!(
                "tracked source role is duplicated: {:?}",
                source.role()
            )));
        }
        roles.push(source.role());
        if source.role() == TrackedSourceRole::DlssFix {
            return Err(invalid("Luma active update cannot retain a DlssFix source"));
        }
        if source.url().trim().is_empty() {
            return Err(invalid_detail(format!(
                "tracked source URL is empty for {:?}",
                source.role()
            )));
        }
        let digest = Sha256Hash::new(source.digest()).map_err(|_| {
            invalid_detail(format!(
                "tracked source digest is not valid SHA-256 for {:?}",
                source.role()
            ))
        })?;
        if source.role() == TrackedSourceRole::AddonPayload && source.is_advisory() {
            return Err(invalid(
                "active Luma payload source cannot remain advisory after update",
            ));
        }
        if source.role() == TrackedSourceRole::HostBinary
            && host.mode() == ManagedFileMode::Owned
            && digest != *host.installed_sha256()
        {
            return Err(invalid(
                "HostBinary source digest differs from installed host digest",
            ));
        }
    }

    let payload_sources = sources
        .iter()
        .filter(|source| source.role() == TrackedSourceRole::AddonPayload)
        .count();
    if payload_sources != 1 {
        return Err(invalid_detail(format!(
            "active Luma record requires exactly one AddonPayload source, found {payload_sources}"
        )));
    }

    let host_sources = sources
        .iter()
        .filter(|source| source.role() == TrackedSourceRole::HostBinary)
        .count();
    match host.mode() {
        ManagedFileMode::Reused if host_sources != 0 => Err(invalid(
            "reused active Luma host cannot retain HostBinary provenance",
        )),
        ManagedFileMode::Owned if host_sources != 1 => Err(invalid_detail(format!(
            "owned active Luma host requires exactly one HostBinary source, found {host_sources}",
        ))),
        _ => Ok(()),
    }
}

fn invalid(reason: &'static str) -> LumaActiveUpdateError {
    LumaActiveUpdateError::invalid_input(reason)
}

fn invalid_detail(reason: String) -> LumaActiveUpdateError {
    LumaActiveUpdateError::invalid_input_detail(reason)
}
