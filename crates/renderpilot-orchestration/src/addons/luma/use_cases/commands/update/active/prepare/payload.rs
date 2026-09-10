use std::path::Path;

use renderpilot_domain::normalized_path_key;

use crate::ServiceError;
use crate::addons::luma::source;
use crate::addons::luma::use_cases::update_target::ResolvedUpdateTarget;
use crate::addons::update::{UpdateStatus, validator_fast_path};
use crate::net::head_validators;

use super::super::model::ActiveUpdatePhase1;
use super::sources;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Plan {
    Preserve,
    Full,
}

impl Plan {
    pub(super) const fn is_full(self) -> bool {
        matches!(self, Self::Full)
    }
}

pub(super) async fn plan(
    phase1: &ActiveUpdatePhase1,
    force_full: bool,
) -> Result<Plan, ServiceError> {
    let (_, payload_source) = sources::require_payload(phase1.record().tracked_sources())?;
    if locally_requires_full(phase1, payload_source, force_full) {
        return Ok(Plan::Full);
    }

    let validators = head_validators(payload_source.url(), "Luma active update check").await;
    let returned_validator = validators
        .ok()
        .and_then(|validators| validators.cache_validator());
    Ok(classify_head(
        payload_source
            .etag()
            .or_else(|| payload_source.last_modified()),
        returned_validator.as_deref(),
    ))
}

pub(super) fn classify_head(stored: Option<&str>, returned: Option<&str>) -> Plan {
    if validator_fast_path(stored, returned).is_some_and(|status| status == UpdateStatus::Current) {
        Plan::Preserve
    } else {
        Plan::Full
    }
}

pub(super) fn locally_requires_full(
    phase1: &ActiveUpdatePhase1,
    payload_source: &renderpilot_domain::TrackedSource,
    force_full: bool,
) -> bool {
    force_full
        || phase1.had_torn_marker()
        || !phase1.payload_disk_intact()
        || payload_source.is_advisory()
        || payload_source.url() != source::asset_url(&phase1.target().asset)
        || !same_addon_filename(phase1.record().addon_file().as_str(), phase1.target())
}

fn same_addon_filename(recorded: &str, target: &ResolvedUpdateTarget) -> bool {
    let Some(recorded) = Path::new(recorded)
        .file_name()
        .and_then(|name| name.to_str())
    else {
        return false;
    };
    let Some(expected) = Path::new(&target.addon_file)
        .file_name()
        .and_then(|name| name.to_str())
    else {
        return false;
    };
    normalized_path_key(recorded) == normalized_path_key(expected)
}
