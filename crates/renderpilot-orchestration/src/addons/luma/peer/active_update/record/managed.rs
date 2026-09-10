use std::collections::HashSet;

use renderpilot_domain::{InstalledAddon, ManagedAddonFile, normalized_path_key};

use crate::addons::luma::peer::active_update::error::LumaActiveUpdateError;

pub(super) fn rebuild(
    before: &InstalledAddon,
    host: &ManagedAddonFile,
    dlss: Option<ManagedAddonFile>,
) -> Result<Vec<ManagedAddonFile>, LumaActiveUpdateError> {
    let host_key = normalized_path_key(host.path().as_str());
    let dlss_key = dlss
        .as_ref()
        .map(|binding| normalized_path_key(binding.path().as_str()));
    if host_key == dlss_key.as_deref().unwrap_or("") {
        return Err(invalid("host and DLSS bindings overlap"));
    }
    if dlss.as_ref().is_some_and(|binding| !is_dlss_path(binding)) {
        return Err(invalid("projected DLSS binding has an unexpected path"));
    }

    let mut host_index = None;
    let mut dlss_index = None;
    let mut seen = HashSet::new();
    for (index, binding) in before.managed_files().iter().enumerate() {
        let key = normalized_path_key(binding.path().as_str());
        if !seen.insert(key) {
            return Err(invalid("persisted managed bindings contain a duplicate"));
        }
        if is_dlss_path(binding) {
            if dlss_index.replace(index).is_some() {
                return Err(invalid("persisted managed DLSS binding is not unique"));
            }
        } else if host_index.replace(index).is_some() {
            return Err(invalid("persisted active Luma host binding is not unique"));
        }
    }
    let host_index = host_index
        .ok_or_else(|| invalid("persisted active Luma record has no managed host binding"))?;

    let persisted_host = &before.managed_files()[host_index];
    if normalized_path_key(persisted_host.path().as_str()) != host_key {
        return Err(invalid(
            "projected active Luma host path differs from persisted host claim",
        ));
    }
    if let (Some(index), Some(binding)) = (dlss_index, dlss.as_ref())
        && normalized_path_key(before.managed_files()[index].path().as_str())
            != normalized_path_key(binding.path().as_str())
    {
        return Err(invalid(
            "projected DLSS path differs from persisted managed claim",
        ));
    }

    let mut result = Vec::with_capacity(
        before.managed_files().len() + usize::from(dlss_index.is_none() && dlss.is_some()),
    );
    for (index, _binding) in before.managed_files().iter().enumerate() {
        if index == host_index {
            result.push(host.clone());
        } else if Some(index) == dlss_index {
            if let Some(binding) = dlss.as_ref() {
                result.push(binding.clone());
            }
        } else {
            return Err(invalid("persisted managed binding is not host or DLSS"));
        }
    }
    if dlss_index.is_none()
        && let Some(binding) = dlss
    {
        result.push(binding);
    }
    Ok(result)
}

fn is_dlss_path(binding: &ManagedAddonFile) -> bool {
    binding
        .path()
        .file_name()
        .is_some_and(|name| name.eq_ignore_ascii_case(renderpilot_detection::NVNGX_DLSS_FILE_NAME))
}

fn invalid(reason: &'static str) -> LumaActiveUpdateError {
    LumaActiveUpdateError::invalid_input(reason)
}
