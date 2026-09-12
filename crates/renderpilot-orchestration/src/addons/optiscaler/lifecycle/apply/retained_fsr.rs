//! Exact custody of an official AMD FSR entry point while OptiScaler owns the
//! active filename.

use std::collections::HashMap;
use std::path::PathBuf;

use super::super::*;
use super::plan::FilesystemApplyPlan;

/// One selected AMD FSR entry point and its immutable original backup.
#[derive(Debug, Clone)]
pub(in crate::addons::optiscaler::lifecycle::apply) struct RetainedFsrEntryPointPlan {
    pub(in crate::addons::optiscaler::lifecycle::apply) target: PathBuf,
    pub(in crate::addons::optiscaler::lifecycle::apply) original_backup: PathBuf,
    pub(in crate::addons::optiscaler::lifecycle::apply) component_id: ComponentId,
    pub(in crate::addons::optiscaler::lifecycle::apply) original: FileReceipt,
    pub(in crate::addons::optiscaler::lifecycle::apply) action: RetainedFsrOriginalAction,
}

/// The only lifecycle operations for the game-owned FSR original backup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::addons::optiscaler::lifecycle::apply) enum RetainedFsrOriginalAction {
    /// Move the exact official AMD DLL away before publishing OptiScaler.
    Acquire,
    /// Leave the already-retained original untouched during update or repair.
    Preserve,
    /// Remove (or verify absent) this exact OptiScaler DLL, then return the
    /// retained original.
    Restore { active: FileReceipt },
}

/// Consumes the exact journal operations that preserve the game-owned AMD FSR
/// entry point. The original backup is external provenance, while the active
/// target is separately written and recorded as an Owned OptiScaler runtime.
pub(super) fn prepare_retained_fsr_originals(
    plan: &FilesystemApplyPlan<'_>,
    mutation: &mut PreparedFileMutation<'_>,
    changed: &mut Vec<String>,
) -> Result<HashMap<String, OptiScalerReleaseFileBaseline>, ServiceError> {
    let mut baselines = HashMap::new();
    for retained in &plan.retained_fsr {
        match &retained.action {
            RetainedFsrOriginalAction::Acquire => {
                let moved = mutation.relocate_file_exact(
                    &retained.target,
                    &retained.original_backup,
                    &retained.original,
                    FileOwnership::Reused,
                )?;
                if moved != retained.original {
                    return Err(failed(format!(
                        "OptiScaler retained AMD FSR original changed during relocation: {}",
                        retained.target.display()
                    )));
                }
                changed.push(retained.target.to_string_lossy().into_owned());
                changed.push(retained.original_backup.to_string_lossy().into_owned());
            }
            RetainedFsrOriginalAction::Preserve => {
                mutation.verify_unchanged(&retained.original_backup)?;
            }
            RetainedFsrOriginalAction::Restore { active } => {
                mutation.delete_or_verify_exact(&retained.target, active)?;
                let restored = mutation.relocate_file_exact(
                    &retained.original_backup,
                    &retained.target,
                    &retained.original,
                    FileOwnership::Reused,
                )?;
                if restored != retained.original {
                    return Err(failed(format!(
                        "OptiScaler retained AMD FSR original changed during restoration: {}",
                        retained.target.display()
                    )));
                }
                changed.push(retained.original_backup.to_string_lossy().into_owned());
                changed.push(retained.target.to_string_lossy().into_owned());
                continue;
            }
        }
        let baseline = OptiScalerReleaseFileBaseline::RetainedFsrEntryPoint {
            component_id: retained.component_id.clone(),
            custody_path: path_ref(&retained.original_backup)?,
            original: retained.original.clone(),
        };
        if baselines
            .insert(crate::paths::normalized_key(&retained.target), baseline)
            .is_some()
        {
            return Err(failed(format!(
                "OptiScaler has duplicate retained AMD FSR target: {}",
                retained.target.display()
            )));
        }
    }
    Ok(baselines)
}
