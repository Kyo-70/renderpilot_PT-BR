//! Aggregate projection for the one game-owned FSR baseline OptiScaler may
//! retain beside an active replacement DLL.

use super::super::super::*;
use super::context::BindingContext;

/// Lowers the explicit AMD FSR original backup together with its paired
/// active-target transition. The backup is external provenance, not an
/// OptiScaler installation artifact.
pub(super) fn apply_retained_fsr_transitions(context: &mut BindingContext<'_>) -> AppResult<()> {
    let before = retained_fsr_by_target(context.before_state)?;
    let after = retained_fsr_by_target(context.after_state)?;
    let keys = before
        .keys()
        .chain(after.keys())
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    for key in keys {
        match (before.get(&key), after.get(&key)) {
            (None, Some(after)) => {
                let installed = context
                    .after_receipts
                    .get(&key)
                    .map(|(_, receipt)| receipt)
                    .ok_or_else(|| {
                        renderpilot_application::AppError::invalid_input(
                            "retained FSR target has no post-install receipt",
                        )
                    })?;
                if installed != &after.installed {
                    return Err(renderpilot_application::AppError::invalid_input(
                        "retained FSR target receipt does not match its state binding",
                    ));
                }
                let claim = pending_file_mutations::OptiScalerBoundTransition::RelocateThenClaim {
                    source: after.original.clone(),
                    destination: after.original.clone(),
                    installed: after.installed.clone(),
                };
                // `paths` binds both relocation legs. The composite target
                // claim must not be duplicated in `after_claims`.
                insert_path(&mut context.paths, &after.target, claim)?;
                insert_path(
                    &mut context.paths,
                    &after.custody,
                    pending_file_mutations::OptiScalerBoundTransition::Relocate {
                        source: after.original.clone(),
                        destination: after.original.clone(),
                    },
                )?;
                context.relocation_keys.insert(key);
                context
                    .relocation_keys
                    .insert(normalized_path_key(&after.custody));
            }
            (Some(before), None) => {
                let installed = context
                    .before_receipts
                    .get(&key)
                    .map(|(_, receipt)| receipt)
                    .ok_or_else(|| {
                        renderpilot_application::AppError::invalid_input(
                            "retained FSR target has no pre-uninstall receipt",
                        )
                    })?;
                if installed != &before.installed {
                    return Err(renderpilot_application::AppError::invalid_input(
                        "retained FSR target receipt does not match its state binding",
                    ));
                }
                insert_path(
                    &mut context.paths,
                    &before.target,
                    pending_file_mutations::OptiScalerBoundTransition::RemoveOwnedFile {
                        installed: before.installed.clone(),
                        restoration: Some(before.original.clone()),
                        allow_absent: true,
                    },
                )?;
                insert_path(
                    &mut context.paths,
                    &before.custody,
                    pending_file_mutations::OptiScalerBoundTransition::Relocate {
                        source: before.original.clone(),
                        destination: before.original.clone(),
                    },
                )?;
                context.relocation_keys.insert(key);
                context
                    .relocation_keys
                    .insert(normalized_path_key(&before.custody));
            }
            (Some(before), Some(after)) => {
                if !same_retained_fsr_baseline(before, after) {
                    return Err(renderpilot_application::AppError::invalid_input(
                        "retained FSR baseline changed during an OptiScaler update",
                    ));
                }
                insert_path(
                    &mut context.paths,
                    &before.custody,
                    pending_file_mutations::OptiScalerBoundTransition::RelinquishReusedFile {
                        installed: before.original.clone(),
                    },
                )?;
                let custody_key = normalized_path_key(&before.custody);
                if context
                    .retained_fsr_custody
                    .insert(custody_key.clone(), before.original.clone())
                    .is_some()
                {
                    return Err(renderpilot_application::AppError::invalid_input(format!(
                        "OptiScaler state has duplicate retained FSR custody at {}",
                        before.custody
                    )));
                }
                context.relocation_keys.insert(custody_key);
            }
            (None, None) => unreachable!("retained FSR key set contains one side"),
        }
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RetainedFsrBinding {
    target: String,
    component_id: ComponentId,
    custody: String,
    installed: FileReceipt,
    original: FileReceipt,
}

/// The active OptiScaler payload may change during an update. The retained
/// baseline is the exact target, component identity, backup path, and
/// immutable original receipt.
fn same_retained_fsr_baseline(left: &RetainedFsrBinding, right: &RetainedFsrBinding) -> bool {
    left.target == right.target
        && left.component_id == right.component_id
        && left.custody == right.custody
        && left.original == right.original
}

fn retained_fsr_by_target(
    state: Option<&OptiScalerInstallState>,
) -> AppResult<std::collections::BTreeMap<String, RetainedFsrBinding>> {
    let mut bindings = std::collections::BTreeMap::new();
    for file in state
        .into_iter()
        .flat_map(|state| state.release_files.iter())
    {
        let OptiScalerReleaseFileBaseline::RetainedFsrEntryPoint {
            component_id,
            custody_path,
            original,
        } = &file.baseline
        else {
            continue;
        };
        let binding = RetainedFsrBinding {
            target: file.path.as_str().to_owned(),
            component_id: component_id.clone(),
            custody: custody_path.as_str().to_owned(),
            installed: file.installed.clone(),
            original: original.clone(),
        };
        if bindings
            .insert(normalized_path_key(file.path.as_str()), binding)
            .is_some()
        {
            return Err(renderpilot_application::AppError::invalid_input(
                "OptiScaler state has duplicate retained FSR targets",
            ));
        }
    }
    Ok(bindings)
}
