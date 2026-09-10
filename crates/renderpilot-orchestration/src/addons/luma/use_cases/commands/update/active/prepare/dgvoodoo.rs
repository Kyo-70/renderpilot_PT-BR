use renderpilot_domain::TrackedSourceRole;

use crate::ServiceError;
use crate::addons::luma::dgvoodoo;

use super::super::model::{ActiveUpdatePhase1, DgVoodooLocalDecision};
use super::invalid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Plan {
    Preserve,
    Replace,
    Remove,
}

impl Plan {
    pub(super) const fn is_replace(self) -> bool {
        matches!(self, Self::Replace)
    }
}

pub(super) fn plan(phase1: &ActiveUpdatePhase1, payload_full: bool) -> Result<Plan, ServiceError> {
    let wrapper_count = phase1
        .record()
        .tracked_sources()
        .iter()
        .filter(|source| source.role() == TrackedSourceRole::DgVoodooWrapper)
        .count();
    if wrapper_count > 1 {
        return Err(invalid(
            "active Luma record has duplicate dgVoodoo wrapper provenance",
        ));
    }

    match phase1.dgvoodoo() {
        DgVoodooLocalDecision::Preserve { .. } => Ok(Plan::Preserve),
        DgVoodooLocalDecision::Remove => Ok(Plan::Remove),
        DgVoodooLocalDecision::Replace { .. } => {
            require_replacement_inputs(phase1, wrapper_count)?;
            Ok(Plan::Replace)
        }
        DgVoodooLocalDecision::ReplaceOnFull { .. } => {
            if payload_full {
                require_replacement_inputs(phase1, wrapper_count)?;
                Ok(Plan::Replace)
            } else {
                Ok(Plan::Preserve)
            }
        }
    }
}

fn require_replacement_inputs(
    phase1: &ActiveUpdatePhase1,
    wrapper_count: usize,
) -> Result<(), ServiceError> {
    if dgvoodoo::requirement(phase1.target().external_requirement.as_ref()).is_none() {
        return Err(invalid(
            "active Luma dgVoodoo replacement has no current requirement",
        ));
    }
    if wrapper_count != 1 {
        return Err(invalid(
            "active Luma dgVoodoo replacement requires exactly one wrapper provenance",
        ));
    }
    Ok(())
}
