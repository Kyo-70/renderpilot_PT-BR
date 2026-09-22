//! RenoDX's typed `ReShade.ini` configuration planner and removal transforms.
//!
//! The `[ADDON]`/`[INSTALL]` schema constants and the additive
//! `ini_merge_strategy` write transform are shared at
//! [`crate::addons::reshade::ini_schema`]. This module owns only the
//! byte-preserving RenoDX configuration planner, receipt-aware reconciliation
//! and removal, and the narrow legacy removal strategies used on uninstall.

use std::fmt;

#[cfg(test)]
use renderpilot_domain::{PathRef, RenoDxSetPathValue};
use renderpilot_domain::{RenoDxConfigReceipt, RenoDxManagedConfigKey};

mod config;
mod document;
mod removal;
mod strategies;

#[cfg(test)]
mod tests;

pub(crate) use config::{plan_config, plan_config_reconcile};
pub(crate) use removal::plan_config_removal;
pub(crate) use strategies::{ini_remove_dlss_fix_strategy, ini_remove_renodx_strategy};

pub(crate) const SET_PATH_KEY: &str = RenoDxManagedConfigKey::SetPath.as_str();

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RenoDxConfigError {
    NonUtf8,
    Nul,
    Ambiguous(&'static str),
    InvalidReceipt,
}

impl fmt::Display for RenoDxConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonUtf8 => f.write_str("ReShade.ini is not valid UTF-8"),
            Self::Nul => f.write_str("ReShade.ini contains NUL bytes"),
            Self::Ambiguous(reason) => {
                write!(
                    f,
                    "ambiguous ReShade.ini RenoDX configuration syntax: {reason}"
                )
            }
            Self::InvalidReceipt => f.write_str("invalid RenoDX configuration receipt"),
        }
    }
}

impl std::error::Error for RenoDxConfigError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RenoDxConfigMutation {
    pub(crate) after: Vec<u8>,
    pub(crate) receipt: RenoDxConfigReceipt,
    pub(crate) changed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RenoDxConfigReconcile {
    pub(crate) after: Option<Vec<u8>>,
    pub(crate) receipt: Option<RenoDxConfigReceipt>,
    pub(crate) changed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg(test)]
pub(crate) struct RenoDxSetPathMutation {
    pub(crate) after: Vec<u8>,
    pub(crate) receipt: RenoDxConfigReceipt,
    pub(crate) changed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg(test)]
pub(crate) struct RenoDxSetPathRemoval {
    pub(crate) after: Option<Vec<u8>>,
    pub(crate) changed: bool,
}

#[cfg(test)]
pub(crate) fn current_set_path_value(before: &[u8]) -> Result<Option<String>, RenoDxConfigError> {
    let doc = document::IniDocument::parse(before)?;
    doc.current_set_path_value()
}

/// Adapts the production multi-key planner to the historical Set_Path test shape.
#[cfg(test)]
pub(crate) fn plan_set_path_reconcile(
    ini_path: PathRef,
    before: Option<&[u8]>,
    desired: Option<RenoDxSetPathValue>,
    receipt: Option<&RenoDxConfigReceipt>,
) -> Result<RenoDxConfigReconcile, RenoDxConfigError> {
    plan_config_reconcile(ini_path, before, desired, None, receipt)
}

/// Adapts the production multi-key planner to the historical Set_Path test shape.
#[cfg(test)]
pub(crate) fn plan_set_path(
    ini_path: PathRef,
    before: &[u8],
    desired: RenoDxSetPathValue,
) -> Result<RenoDxSetPathMutation, RenoDxConfigError> {
    let planned = plan_config(ini_path, before, Some(desired), None)?;
    Ok(RenoDxSetPathMutation {
        after: planned.after,
        receipt: planned.receipt,
        changed: planned.changed,
    })
}

/// Adapts tolerant production removal to the historical Set_Path test shape.
#[cfg(test)]
pub(crate) fn plan_set_path_removal(
    before: &[u8],
    receipt: &RenoDxConfigReceipt,
) -> Result<RenoDxSetPathRemoval, RenoDxConfigError> {
    let planned = plan_config_removal(&receipt.ini_path, Some(before), receipt)?;
    Ok(RenoDxSetPathRemoval {
        after: planned.after,
        changed: planned.changed,
    })
}
