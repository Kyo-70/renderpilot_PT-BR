use super::super::super::*;
use super::claims::collect_retained_claims;
use super::context::{BindingContext, ReusedAggregateRole};
use super::retained_fsr::apply_retained_fsr_transitions;

pub(super) fn apply_transitions(
    context: &mut BindingContext<'_>,
    retained_claims: &[OptiScalerRetainedClaim],
) -> AppResult<()> {
    let mut all_keys = context.known_paths.clone();
    all_keys.extend(context.before_receipts.keys().cloned());
    all_keys.extend(context.after_receipts.keys().cloned());
    all_keys.extend(context.before_directories.keys().cloned());
    all_keys.extend(context.after_directories.keys().cloned());
    all_keys.extend(
        optiscaler_transition_paths(
            context.before_state,
            context.after_state,
            context.before_topology,
            context.after_topology,
            context.peer.mutation,
        )
        .into_iter()
        .map(|path| normalized_path_key(&path)),
    );

    let retained_by_key = collect_retained_claims(context, retained_claims)?;
    apply_retained_fsr_transitions(context)?;

    for key in all_keys {
        if context.relocation_keys.contains(&key) || context.auxiliary_destinations.contains(&key) {
            continue;
        }
        let before_directory = context.before_directories.get(&key);
        let after_directory = context.after_directories.get(&key);
        let has_before_file = context.before_receipts.contains_key(&key);
        let has_after_file = context.after_receipts.contains_key(&key);
        if has_before_file && (before_directory.is_some() || after_directory.is_some())
            || has_after_file && before_directory.is_some()
        {
            return Err(renderpilot_application::AppError::invalid_input(
                "OptiScaler aggregate reuses one path as both file and directory",
            ));
        }
        if before_directory.is_some() || after_directory.is_some() {
            match (before_directory, after_directory) {
                (Some((_before_path, before_identity)), Some((after_path, after_identity))) => {
                    if before_identity != after_identity {
                        // A persisted managed directory that is absent at
                        // execution time is recreated through an explicit
                        // Absent -> Directory journal action. Its current
                        // identity replaces the stale receipt; the fold
                        // below still requires that exact recreation proof.
                        let transition =
                            pending_file_mutations::OptiScalerBoundTransition::ClaimedDirectory {
                                identity: after_identity.clone(),
                            };
                        insert_path(&mut context.paths, after_path, transition.clone())?;
                        insert_path(&mut context.after_claims, after_path, transition)?;
                    }
                }
                (None, Some((after_path, identity))) => {
                    let transition =
                        pending_file_mutations::OptiScalerBoundTransition::ClaimedDirectory {
                            identity: identity.clone(),
                        };
                    insert_path(&mut context.paths, after_path, transition.clone())?;
                    insert_path(&mut context.after_claims, after_path, transition)?;
                }
                (Some((before_path, identity)), None) => {
                    insert_path(
                        &mut context.paths,
                        before_path,
                        pending_file_mutations::OptiScalerBoundTransition::PostCommitRemoveDirectory {
                            identity: identity.clone(),
                        },
                    )?;
                }
                (None, None) => {}
            }
            continue;
        }

        let retained = retained_by_key.get(&key).copied();
        let Some(transition) = transition_for_file(context, &key, retained)? else {
            continue;
        };
        let Some(path) = context
            .before_receipts
            .get(&key)
            .or_else(|| context.after_receipts.get(&key))
            .map(|(path, _)| path.as_str())
            .or_else(|| retained.map(|claim| claim.path.as_str()))
        else {
            return Err(renderpilot_application::AppError::invalid_input(
                "OptiScaler transition has no path",
            ));
        };
        insert_path(&mut context.paths, path, transition)?;
    }
    Ok(())
}

fn transition_for_file(
    context: &mut BindingContext<'_>,
    key: &str,
    retained: Option<&OptiScalerRetainedClaim>,
) -> AppResult<Option<pending_file_mutations::OptiScalerBoundTransition>> {
    let before_file = context.before_receipts.get(key);
    let after_file = context.after_receipts.get(key);
    let transition = if let Some(claim) = retained {
        if before_file.or(after_file).is_none() {
            return Err(renderpilot_application::AppError::invalid_input(
                "OptiScaler retained claim has no receipt transition",
            ));
        }
        Some(
            pending_file_mutations::OptiScalerBoundTransition::RelinquishReusedFile {
                installed: claim.receipt.clone(),
            },
        )
    } else if let Some(transition) =
        configuration_restore_transition(context, key, before_file, after_file)?
    {
        Some(transition)
    } else {
        match (before_file, after_file) {
            (None, None) => None,
            (None, Some((path, receipt))) => {
                let transition =
                    if let Some(prior) = new_configuration_acquisition(context, key, receipt) {
                        pending_file_mutations::OptiScalerBoundTransition::AcquireReusedFile {
                            prior,
                            installed: receipt.clone(),
                            mode: pending_file_mutations::ReusedAcquisitionMode::Configuration,
                            configuration_baseline: context
                                .after_state
                                .and_then(|state| state.configuration_baseline().receipt())
                                .cloned(),
                        }
                    } else if is_owned(receipt) {
                        pending_file_mutations::OptiScalerBoundTransition::ClaimedFile {
                            installed: receipt.clone(),
                        }
                    } else {
                        pending_file_mutations::OptiScalerBoundTransition::RelinquishReusedFile {
                            installed: receipt.clone(),
                        }
                    };
                insert_path(&mut context.after_claims, path, transition.clone())?;
                Some(transition)
            }
            (Some((_path, before)), None) => {
                if is_owned(before) {
                    Some(
                        pending_file_mutations::OptiScalerBoundTransition::RemoveOwnedFile {
                            installed: before.clone(),
                            restoration: None,
                            allow_absent: true,
                        },
                    )
                } else {
                    match context.reused_role(key) {
                        Some(
                            ReusedAggregateRole::ReleaseArtifact
                            | ReusedAggregateRole::RuntimeBinding
                            | ReusedAggregateRole::TopologyOuter,
                        ) => Some(
                            pending_file_mutations::OptiScalerBoundTransition::RemoveReusedFile {
                                installed: before.clone(),
                                restoration: None,
                                allow_absent: true,
                            },
                        ),
                        Some(ReusedAggregateRole::Configuration) => Some(
                            pending_file_mutations::OptiScalerBoundTransition::ObserveReusedConfiguration {
                                persisted: before.clone(),
                            },
                        ),
                        None => Some(
                            pending_file_mutations::OptiScalerBoundTransition::RelinquishReusedFile {
                                installed: before.clone(),
                            },
                        ),
                    }
                }
            }
            (Some((path, before)), Some((_, after))) => {
                if before.ownership() != after.ownership() {
                    if before.ownership() == FileOwnership::Reused
                        && after.ownership() == FileOwnership::Owned
                    {
                        let mode = match context.reused_role(key) {
                            Some(ReusedAggregateRole::Configuration) => {
                                pending_file_mutations::ReusedAcquisitionMode::Configuration
                            }
                            Some(
                                ReusedAggregateRole::ReleaseArtifact
                                | ReusedAggregateRole::RuntimeBinding
                                | ReusedAggregateRole::TopologyOuter,
                            ) => pending_file_mutations::ReusedAcquisitionMode::ExactOrAbsent,
                            None => {
                                return Err(renderpilot_application::AppError::invalid_input(
                                    format!(
                                        "OptiScaler receipt ownership changed outside a mutable role at {path}"
                                    ),
                                ));
                            }
                        };
                        return Ok(Some(
                            pending_file_mutations::OptiScalerBoundTransition::AcquireReusedFile {
                                prior: before.clone(),
                                installed: after.clone(),
                                mode,
                                configuration_baseline: if mode
                                    == pending_file_mutations::ReusedAcquisitionMode::Configuration
                                {
                                    context
                                        .after_state
                                        .and_then(|state| state.configuration_baseline().receipt())
                                        .cloned()
                                } else {
                                    None
                                },
                            },
                        ));
                    }
                    return Err(renderpilot_application::AppError::invalid_input(format!(
                        "OptiScaler receipt ownership changed at {path}"
                    )));
                }
                if is_reused(before) {
                    if context.reused_role(key) == Some(ReusedAggregateRole::Configuration) {
                        if !same_receipt_identity_digest_ownership(before, after) {
                            return Err(renderpilot_application::AppError::invalid_input(format!(
                                "reused OptiScaler configuration receipt changed at {path}"
                            )));
                        }
                        return Ok(Some(
                            pending_file_mutations::OptiScalerBoundTransition::ObserveReusedConfiguration {
                                persisted: before.clone(),
                            },
                        ));
                    }
                    if !same_receipt_identity_digest_ownership(before, after) {
                        return Err(renderpilot_application::AppError::invalid_input(format!(
                            "reused OptiScaler receipt changed at {path}"
                        )));
                    }
                    Some(
                        pending_file_mutations::OptiScalerBoundTransition::RelinquishReusedFile {
                            installed: before.clone(),
                        },
                    )
                } else if !same_receipt_identity(before, after) {
                    Some(
                        pending_file_mutations::OptiScalerBoundTransition::RecreateOwnedFile {
                            prior: before.clone(),
                            installed: after.clone(),
                        },
                    )
                } else if before.digest() == after.digest() {
                    Some(
                        pending_file_mutations::OptiScalerBoundTransition::VerifyOwnedFile {
                            prior: before.clone(),
                            current: after.clone(),
                        },
                    )
                } else {
                    Some(
                        pending_file_mutations::OptiScalerBoundTransition::ReplaceClaimedFile {
                            before: before.clone(),
                            after: after.clone(),
                        },
                    )
                }
            }
        }
    };
    Ok(transition)
}

fn new_configuration_acquisition(
    context: &BindingContext<'_>,
    key: &str,
    installed: &FileReceipt,
) -> Option<FileReceipt> {
    if context.before_state.is_some() {
        return None;
    }
    let state = context.after_state?;
    let configuration = state.release_files.iter().find(|file| {
        file.role == OptiScalerFileRole::Configuration
            && normalized_path_key(file.path.as_str()) == key
            && &file.installed == installed
            && file.cleanup == OptiScalerFileCleanup::PreserveCurrentThenRestoreBaseline
    })?;
    let baseline = state.configuration_baseline().receipt()?;
    (configuration.installed.ownership() == FileOwnership::Owned).then(|| baseline.clone())
}

/// Lowers the one legal OptiScaler configuration-uninstall restoration into a
/// dedicated custody transition.  A state removal must not be interpreted as
/// a generic owned-file removal when the persisted configuration baseline is a
/// present user file: the live object is rewritten in place and then released
/// as Reused custody.
fn configuration_restore_transition(
    context: &BindingContext<'_>,
    key: &str,
    before_file: Option<&(String, FileReceipt)>,
    after_file: Option<&(String, FileReceipt)>,
) -> AppResult<Option<pending_file_mutations::OptiScalerBoundTransition>> {
    let (Some(before_state), None) = (context.before_state, context.after_state) else {
        return Ok(None);
    };
    let renderpilot_domain::OptiScalerConfigurationBaseline::Present {
        receipt: restoration,
        ..
    } = before_state.configuration_baseline()
    else {
        return Ok(None);
    };
    let configuration = before_state
        .release_files
        .iter()
        .find(|file| file.role == OptiScalerFileRole::Configuration)
        .ok_or_else(|| {
            renderpilot_application::AppError::invalid_input(
                "OptiScaler present configuration baseline has no configuration receipt",
            )
        })?;
    if normalized_path_key(configuration.path.as_str()) != key {
        return Ok(None);
    }
    let Some((installed_path, installed)) = before_file else {
        return Err(renderpilot_application::AppError::invalid_input(
            "OptiScaler configuration restoration has no installed receipt",
        ));
    };
    if normalized_path_key(installed_path) != key || installed != &configuration.installed {
        return Err(renderpilot_application::AppError::invalid_input(
            "OptiScaler configuration restoration does not bind the canonical installed receipt",
        ));
    }
    if installed.ownership() != FileOwnership::Owned {
        return Ok(None);
    }
    if after_file.is_some()
        || restoration.ownership() != FileOwnership::Reused
        || restoration.validate().is_err()
    {
        return Err(renderpilot_application::AppError::invalid_input(
            "OptiScaler configuration restoration has invalid exact baseline custody",
        ));
    }
    let restoration = FileReceipt::reused(
        installed.identity().to_owned(),
        restoration.digest().clone(),
    )
    .map_err(|error| {
        renderpilot_application::AppError::invalid_input(format!(
            "OptiScaler configuration restoration receipt is invalid: {error}"
        ))
    })?;
    Ok(Some(
        pending_file_mutations::OptiScalerBoundTransition::RestoreOwnedFile {
            installed: installed.clone(),
            restoration,
        },
    ))
}
