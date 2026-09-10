use std::collections::BTreeSet;
use std::path::Path;

use renderpilot_domain::{InstalledAddon, ManagedFileMode, PathRef};

use super::model::PersistedDlss;
use crate::addons::luma::peer::active_update::{
    error::LumaActiveUpdateError, model::LumaActiveUpdateDlssInput,
};
use crate::addons::luma::peer::root_authority::LumaPeerRootAuthority;
use crate::catalog::cascade::CascadeResult;

pub(super) fn target(authority: &LumaPeerRootAuthority) -> Result<PathRef, LumaActiveUpdateError> {
    authority
        .effective_dlss_target()
        .map_err(LumaActiveUpdateError::authority)
}

pub(super) fn persisted_binding(
    before: &InstalledAddon,
    target: &PathRef,
) -> Result<PersistedDlss, LumaActiveUpdateError> {
    let target_key = renderpilot_domain::normalized_path_key(target.as_str());
    let mut dlss = Vec::new();
    for binding in before.managed_files() {
        let is_dlss = binding.path().file_name().is_some_and(|name| {
            name.eq_ignore_ascii_case(renderpilot_detection::NVNGX_DLSS_FILE_NAME)
        });
        if is_dlss {
            if renderpilot_domain::normalized_path_key(binding.path().as_str()) != target_key {
                return Err(invalid_detail(
                    "persisted DLSS binding is outside the sealed effective payload root",
                ));
            }
            dlss.push(binding);
        }
    }
    if dlss.len() > 1 {
        return Err(invalid_detail("persisted DLSS binding is not unique"));
    }

    for path in before
        .created_files()
        .iter()
        .chain(before.backed_up_files())
    {
        if renderpilot_domain::normalized_path_key(path.as_str()) == target_key {
            return Err(invalid_detail(
                "DLSS target aliases a generic managed payload claim",
            ));
        }
    }

    Ok(match dlss.into_iter().next() {
        None => PersistedDlss::None,
        Some(binding) if binding.mode() == ManagedFileMode::Reused => {
            PersistedDlss::Reused(binding.clone())
        }
        Some(binding) => PersistedDlss::Owned(binding.clone()),
    })
}

pub(super) fn validate_cascade(
    before: &InstalledAddon,
    target: &PathRef,
    cascade: &CascadeResult,
    input: &LumaActiveUpdateDlssInput,
    persisted_owned: bool,
) -> Result<(), LumaActiveUpdateError> {
    let target_path = Path::new(target.as_str());
    let containing = cascade
        .rollback_specs
        .iter()
        .filter(|spec| spec.contains_path(target_path))
        .count();
    let has_specs = !cascade.rollback_specs.is_empty();
    let has_claim = cascade.catalog_claim().is_some();
    let consumed = match containing {
        0 if has_specs || has_claim => {
            return Err(invalid_detail(
                "catalog cascade contains unrelated rollback state for the DLSS target",
            ));
        }
        0 => false,
        1 if cascade.rollback_specs.len() == 1 => {
            let claim = cascade.catalog_claim().ok_or_else(|| {
                invalid_detail("catalog cascade consumes DLSS without a catalog claim")
            })?;
            if claim.game_id() != before.game_id() {
                return Err(invalid_detail("catalog cascade belongs to another game"));
            }
            let spec_ids = cascade
                .rollback_specs
                .iter()
                .map(|spec| spec.component_id().clone())
                .collect::<BTreeSet<_>>();
            let claim_ids = claim
                .deleted_baselines()
                .iter()
                .map(|entry| entry.component_id().clone())
                .collect::<BTreeSet<_>>();
            if spec_ids != claim_ids || cascade.next_components != claim.after_components() {
                return Err(invalid_detail(
                    "catalog cascade rollback specs do not match its durable claim",
                ));
            }
            true
        }
        _ => {
            return Err(invalid_detail(
                "multiple catalog rollback plans consume DLSS",
            ));
        }
    };

    if consumed
        && (!persisted_owned
            || !matches!(
                input,
                LumaActiveUpdateDlssInput::Full {
                    bundled_bytes: None
                }
            ))
    {
        return Err(invalid_detail(
            "catalog cascade requires an owned DLSS release",
        ));
    }
    Ok(())
}

fn invalid_detail(reason: &'static str) -> LumaActiveUpdateError {
    LumaActiveUpdateError::invalid_input(reason)
}
