use renderpilot_domain::{FileOwnership, TrackedSourceRole};

use super::super::model::ActiveUpdatePhase1;
use super::invalid;
use crate::ServiceError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Plan {
    Preserve,
    Replace,
}

impl Plan {
    pub(super) const fn is_replace(self) -> bool {
        matches!(self, Self::Replace)
    }
}

pub(super) fn plan(phase1: &ActiveUpdatePhase1) -> Result<Plan, ServiceError> {
    let host_sources = phase1
        .record()
        .tracked_sources()
        .iter()
        .filter(|source| source.role() == TrackedSourceRole::HostBinary)
        .count();
    if host_sources > 1 {
        return Err(invalid(
            "active Luma record has duplicate HostBinary provenance",
        ));
    }
    let downstream = phase1
        .topology()
        .downstream
        .as_ref()
        .ok_or_else(|| invalid("active Luma topology has no ReShade downstream"))?;
    if phase1.host_replacement_required() {
        return Ok(Plan::Replace);
    }

    let valid = match downstream.receipt.ownership() {
        FileOwnership::Reused => host_sources == 0,
        FileOwnership::Owned => host_sources == 1,
    };
    if !valid {
        return Err(invalid(
            "active Luma retained ReShade host provenance is inconsistent with topology custody",
        ));
    }
    Ok(Plan::Preserve)
}
