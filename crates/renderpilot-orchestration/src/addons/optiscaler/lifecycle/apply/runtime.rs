//! Pre-IO planning for OptiScaler runtime bindings.
//!
//! Native component copies and downloaded module artifacts have the same
//! custody contract.  This module owns only their small decision matrix; it
//! deliberately does not know about journals or persistence.

use std::path::{Path, PathBuf};

use renderpilot_domain::{
    FileOwnership, FileReceipt, OptiScalerFileBaseline, OptiScalerModuleRuntimeBinding, Sha256Hash,
};

use super::super::maybe_exact_receipt_from_live;
use crate::{ServiceError, failed};

/// The only runtime actions selected before the durable mutation starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum RuntimeApplyStep {
    /// Publish the requested bytes while preserving the absent baseline and
    /// retaining destructive ownership.
    WriteOwnedAbsent,
    /// Verify an unchanged operation-owned file without rewriting it.
    VerifyOwnedAbsent { receipt: FileReceipt },
    /// Verify an unchanged foreign file, retaining its exact identity and
    /// digest as the present reused baseline.
    VerifyReusedPresent { receipt: FileReceipt },
}

/// One fully classified runtime target.  All fields are decided before a
/// pending mutation row or any mutation IO exists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RuntimeApplyPlan {
    path: PathBuf,
    module: String,
    expected: Sha256Hash,
    step: RuntimeApplyStep,
}

impl RuntimeApplyPlan {
    pub(super) fn new(
        path: PathBuf,
        module: impl Into<String>,
        expected: Sha256Hash,
        step: RuntimeApplyStep,
    ) -> Self {
        Self {
            path,
            module: module.into(),
            expected,
            step,
        }
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    pub(super) fn module(&self) -> &str {
        &self.module
    }

    pub(super) fn expected(&self) -> &Sha256Hash {
        &self.expected
    }

    pub(super) fn step(&self) -> &RuntimeApplyStep {
        &self.step
    }

    pub(super) fn requires_write(&self) -> bool {
        matches!(self.step, RuntimeApplyStep::WriteOwnedAbsent)
    }

    pub(super) fn installed_ownership(&self) -> FileOwnership {
        match self.step {
            RuntimeApplyStep::WriteOwnedAbsent | RuntimeApplyStep::VerifyOwnedAbsent { .. } => {
                FileOwnership::Owned
            }
            RuntimeApplyStep::VerifyReusedPresent { .. } => FileOwnership::Reused,
        }
    }

    pub(super) fn baseline(&self) -> OptiScalerFileBaseline {
        match &self.step {
            RuntimeApplyStep::WriteOwnedAbsent | RuntimeApplyStep::VerifyOwnedAbsent { .. } => {
                OptiScalerFileBaseline::Absent
            }
            RuntimeApplyStep::VerifyReusedPresent { receipt } => OptiScalerFileBaseline::Present {
                receipt: receipt.clone(),
            },
        }
    }

    pub(super) fn exact_receipt(&self) -> Option<&FileReceipt> {
        match &self.step {
            RuntimeApplyStep::WriteOwnedAbsent => None,
            RuntimeApplyStep::VerifyOwnedAbsent { receipt }
            | RuntimeApplyStep::VerifyReusedPresent { receipt } => Some(receipt),
        }
    }
}

/// Classifies one target from its persisted binding and one plan-time live
/// observation.  This pure boundary makes the ownership matrix testable
/// without a fake journal or a fake filesystem.
pub(super) fn classify_runtime_target(
    prior: Option<&OptiScalerModuleRuntimeBinding>,
    live: Option<&FileReceipt>,
    module: &str,
    path: &Path,
    expected: Sha256Hash,
) -> Result<RuntimeApplyPlan, ServiceError> {
    let step = match prior {
        None => match live {
            None => RuntimeApplyStep::WriteOwnedAbsent,
            Some(receipt) if receipt.digest() == &expected => {
                RuntimeApplyStep::VerifyReusedPresent {
                    receipt: receipt.clone(),
                }
            }
            Some(receipt) => {
                return Err(conflict(
                    module,
                    path,
                    "an unrelated file already occupies the target",
                    receipt,
                ));
            }
        },
        Some(binding) => {
            if binding.module != module {
                return Err(failed(format!(
                    "OptiScaler runtime target {} is already bound to module {}, not {module}",
                    path.display(),
                    binding.module
                )));
            }
            match binding.installed.ownership() {
                FileOwnership::Owned => {
                    if !matches!(binding.baseline, OptiScalerFileBaseline::Absent) {
                        return Err(failed(format!(
                            "invalid persisted Owned runtime binding for module {module} at {}: baseline must be absent",
                            path.display()
                        )));
                    }
                    let Some(live) = live else {
                        return Ok(RuntimeApplyPlan::new(
                            path.to_path_buf(),
                            module,
                            expected,
                            RuntimeApplyStep::WriteOwnedAbsent,
                        ));
                    };
                    if !same_identity_and_digest(live, &binding.installed) {
                        return Err(repair_required(
                            module,
                            path,
                            "the operation-owned runtime file drifted",
                        ));
                    }
                    if live.digest() == &expected {
                        RuntimeApplyStep::VerifyOwnedAbsent {
                            receipt: binding.installed.clone(),
                        }
                    } else {
                        RuntimeApplyStep::WriteOwnedAbsent
                    }
                }
                FileOwnership::Reused => {
                    let OptiScalerFileBaseline::Present { receipt: baseline } = &binding.baseline
                    else {
                        return Err(failed(format!(
                            "invalid persisted Reused runtime binding for module {module} at {}: baseline must be present",
                            path.display()
                        )));
                    };
                    if baseline.ownership() != FileOwnership::Reused
                        || !same_identity_and_digest(baseline, &binding.installed)
                    {
                        return Err(failed(format!(
                            "invalid persisted Reused runtime binding for module {module} at {}: baseline is not the installed receipt",
                            path.display()
                        )));
                    }
                    let Some(live) = live else {
                        return Ok(RuntimeApplyPlan::new(
                            path.to_path_buf(),
                            module,
                            expected,
                            RuntimeApplyStep::WriteOwnedAbsent,
                        ));
                    };
                    if !same_identity_and_digest(live, &binding.installed) {
                        return Err(conflict(
                            module,
                            path,
                            "the reused runtime file drifted and cannot be adopted",
                            live,
                        ));
                    }
                    if live.digest() == &expected {
                        RuntimeApplyStep::VerifyReusedPresent {
                            receipt: live.clone(),
                        }
                    } else {
                        RuntimeApplyStep::WriteOwnedAbsent
                    }
                }
            }
        }
    };
    Ok(RuntimeApplyPlan::new(
        path.to_path_buf(),
        module,
        expected,
        step,
    ))
}

/// Performs the one plan-time observation used by both native and downloaded
/// runtime targets.  A missing parent is an absent target that the journal's
/// directory producer will create; an existing but unreadable parent remains
/// an authority error and is never downgraded to absence.
pub(super) fn plan_runtime_target(
    prior: Option<&OptiScalerModuleRuntimeBinding>,
    module: &str,
    path: &Path,
    expected: Sha256Hash,
) -> Result<RuntimeApplyPlan, ServiceError> {
    let live = observe_runtime_target(path)?;
    classify_runtime_target(prior, live.as_ref(), module, path, expected)
}

fn observe_runtime_target(path: &Path) -> Result<Option<FileReceipt>, ServiceError> {
    let parent = path
        .parent()
        .ok_or_else(|| failed(format!("runtime target has no parent: {}", path.display())))?;
    match std::fs::symlink_metadata(parent) {
        Ok(_) => maybe_exact_receipt_from_live(path, FileOwnership::Reused),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(failed(format!(
            "failed to inspect runtime target parent {}: {error}",
            parent.display()
        ))),
    }
}

fn same_identity_and_digest(left: &FileReceipt, right: &FileReceipt) -> bool {
    left.identity() == right.identity() && left.digest() == right.digest()
}

fn conflict(module: &str, path: &Path, reason: &str, live: &FileReceipt) -> ServiceError {
    failed(format!(
        "cannot apply OptiScaler runtime module {module} at {}: {reason} (identity {}, digest {})",
        path.display(),
        live.identity(),
        live.digest()
    ))
}

fn repair_required(module: &str, path: &Path, reason: &str) -> ServiceError {
    failed(format!(
        "OptiScaler runtime module {module} at {} requires repair before apply: {reason}",
        path.display()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn receipt(ownership: FileOwnership, identity: &str, fill: char) -> FileReceipt {
        let digest = Sha256Hash::new(fill.to_string().repeat(64)).expect("digest");
        match ownership {
            FileOwnership::Owned => FileReceipt::owned(identity, digest).expect("receipt"),
            FileOwnership::Reused => FileReceipt::reused(identity, digest).expect("receipt"),
        }
    }

    fn path() -> PathBuf {
        PathBuf::from("C:/Games/Test/OptiScaler/core.dll")
    }

    #[test]
    fn absent_target_is_owned_with_absent_baseline() {
        let expected = Sha256Hash::new("a".repeat(64)).expect("digest");
        let plan = classify_runtime_target(None, None, "core", &path(), expected).expect("plan");
        assert_eq!(plan.step(), &RuntimeApplyStep::WriteOwnedAbsent);
        assert_eq!(plan.installed_ownership(), FileOwnership::Owned);
        assert_eq!(plan.baseline(), OptiScalerFileBaseline::Absent);
    }

    #[test]
    fn exact_unmanaged_target_is_reused_with_exact_present_baseline() {
        let live = receipt(FileOwnership::Reused, "live", 'a');
        let plan =
            classify_runtime_target(None, Some(&live), "core", &path(), live.digest().clone())
                .expect("plan");
        assert_eq!(
            plan.step(),
            &RuntimeApplyStep::VerifyReusedPresent {
                receipt: live.clone()
            }
        );
        assert_eq!(
            plan.baseline(),
            OptiScalerFileBaseline::Present { receipt: live }
        );
    }

    #[test]
    fn occupied_different_target_is_rejected_before_io() {
        let live = receipt(FileOwnership::Reused, "live", 'b');
        let expected = Sha256Hash::new("a".repeat(64)).expect("digest");
        let error = classify_runtime_target(None, Some(&live), "core", &path(), expected)
            .expect_err("conflict");
        assert!(
            error
                .to_string()
                .contains("unrelated file already occupies")
        );
    }

    #[test]
    fn owned_exact_target_updates_or_verifies_without_changing_custody() {
        let old = receipt(FileOwnership::Owned, "owned", 'a');
        let binding = OptiScalerModuleRuntimeBinding {
            module: "core".to_owned(),
            path: renderpilot_domain::PathRef::new(path().to_string_lossy()).expect("path"),
            installed: old.clone(),
            baseline: OptiScalerFileBaseline::Absent,
        };
        let verify = classify_runtime_target(
            Some(&binding),
            Some(&old),
            "core",
            &path(),
            old.digest().clone(),
        )
        .expect("plan");
        assert!(matches!(
            verify.step(),
            RuntimeApplyStep::VerifyOwnedAbsent { .. }
        ));

        let changed = Sha256Hash::new("b".repeat(64)).expect("digest");
        let write = classify_runtime_target(Some(&binding), Some(&old), "core", &path(), changed)
            .expect("plan");
        assert_eq!(write.step(), &RuntimeApplyStep::WriteOwnedAbsent);
        assert_eq!(write.baseline(), OptiScalerFileBaseline::Absent);
    }

    #[test]
    fn exact_reused_target_verifies_or_is_acquired_for_an_update() {
        let old = receipt(FileOwnership::Reused, "reused", 'a');
        let binding = OptiScalerModuleRuntimeBinding {
            module: "core".to_owned(),
            path: renderpilot_domain::PathRef::new(path().to_string_lossy()).expect("path"),
            installed: old.clone(),
            baseline: OptiScalerFileBaseline::Present {
                receipt: old.clone(),
            },
        };
        let verify = classify_runtime_target(
            Some(&binding),
            Some(&old),
            "core",
            &path(),
            old.digest().clone(),
        )
        .expect("plan");
        assert!(matches!(
            verify.step(),
            RuntimeApplyStep::VerifyReusedPresent { .. }
        ));
        let changed = Sha256Hash::new("b".repeat(64)).expect("digest");
        let update = classify_runtime_target(Some(&binding), Some(&old), "core", &path(), changed)
            .expect("exact adopted target is managed");
        assert_eq!(update.step(), &RuntimeApplyStep::WriteOwnedAbsent);
        assert_eq!(update.installed_ownership(), FileOwnership::Owned);
    }

    #[test]
    fn a_path_bound_to_another_module_is_rejected() {
        let old = receipt(FileOwnership::Owned, "owned", 'a');
        let binding = OptiScalerModuleRuntimeBinding {
            module: "other".to_owned(),
            path: renderpilot_domain::PathRef::new(path().to_string_lossy()).expect("path"),
            installed: old.clone(),
            baseline: OptiScalerFileBaseline::Absent,
        };
        let error = classify_runtime_target(
            Some(&binding),
            Some(&old),
            "core",
            &path(),
            old.digest().clone(),
        )
        .expect_err("module identity collision");
        assert!(error.to_string().contains("bound to module other"));
    }

    #[test]
    fn missing_owned_target_is_recreated_but_present_drift_stays_fail_closed() {
        let old = receipt(FileOwnership::Owned, "owned", 'a');
        let binding = OptiScalerModuleRuntimeBinding {
            module: "core".to_owned(),
            path: renderpilot_domain::PathRef::new(path().to_string_lossy()).expect("path"),
            installed: old.clone(),
            baseline: OptiScalerFileBaseline::Absent,
        };
        let missing =
            classify_runtime_target(Some(&binding), None, "core", &path(), old.digest().clone())
                .expect("missing owned target is repairable");
        assert_eq!(missing.step(), &RuntimeApplyStep::WriteOwnedAbsent);
        let drifted = receipt(FileOwnership::Owned, "other", 'b');
        let drift = classify_runtime_target(
            Some(&binding),
            Some(&drifted),
            "core",
            &path(),
            old.digest().clone(),
        )
        .expect_err("drifted owned target");
        assert!(drift.to_string().contains("requires repair"));
    }
}
