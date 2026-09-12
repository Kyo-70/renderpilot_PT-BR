//! Closed uninstall plan: one typed vector lowers to one journal vector.

use super::super::*;

mod build;
mod directories;
mod model;
mod receipts;
mod registry;
mod scope;

pub(super) use build::build_uninstall_plan;
pub(super) use directories::{canonical_postcommit_directories, validate_directory_emptiness};
pub(super) use model::{UninstallFsPlan, UninstallStep, VerifyPreimage};
pub(super) use receipts::{
    ReceiptObservation, missing_recovery_directories, observe_managed_receipt,
    observe_managed_reused_configuration, observe_receipt, require_receipt,
};
pub(super) use registry::validate_producer_registry;
pub(super) use scope::uninstall_scope;

impl UninstallFsPlan {
    pub(super) fn operations(
        &self,
    ) -> Vec<crate::file_mutation::optiscaler::OptiScalerPlannedOperation> {
        self.precommit
            .iter()
            .map(UninstallStep::operation)
            .chain(self.postcommit_directories.iter().cloned().map(
                crate::file_mutation::optiscaler::OptiScalerPlannedOperation::PostCommitRemoveDirectory,
            ))
            .collect()
    }
}
