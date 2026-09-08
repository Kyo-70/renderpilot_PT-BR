use super::prelude::*;
use super::*;

pub(crate) fn mark_file_mutation_committed_within_transaction(
    transaction: &Transaction<'_>,
    id: &str,
) -> AppResult<()> {
    let row: Option<(String, String, String)> = transaction
        .query_row(
            "SELECT feature, state, manifest_json FROM pending_file_mutations WHERE id = ?1",
            [id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(storage_error)?;
    let Some((feature, state, journal_json)) = row else {
        return Err(AppError::storage_failed(format!(
            "pending file mutation `{id}` is missing"
        )));
    };
    if is_optiscaler_feature(&feature) {
        if state != PendingFileMutationState::Prepared.as_str() {
            return Err(AppError::storage_failed(format!(
                "OptiScaler mutation `{id}` is not prepared"
            )));
        }
        let journal = parse_journal(&journal_json, "prepared OptiScaler journal")?;
        validate_prepared_journal(&journal)?;
    }
    let now_ms = sqlite_clock::now_ms(transaction)?;
    let updated = transaction
        .execute(
            "UPDATE pending_file_mutations
                    SET state = 'committed',
                        updated_at = MAX(updated_at, :now_ms)
             WHERE id = :id AND state = 'prepared'",
            named_params! { ":id": id, ":now_ms": now_ms },
        )
        .map_err(storage_error)?;
    if updated != 1 {
        return Err(AppError::storage_failed(format!(
            "pending file mutation `{id}` is missing or is not prepared"
        )));
    }
    Ok(())
}

pub(in crate::repositories) fn is_optiscaler_feature(feature: &str) -> bool {
    matches!(
        feature,
        renderpilot_domain::mutation_features::OPTISCALER_INSTALL
            | renderpilot_domain::mutation_features::OPTISCALER_UPDATE
            | renderpilot_domain::mutation_features::OPTISCALER_RELOCATE
            | renderpilot_domain::mutation_features::OPTISCALER_UNINSTALL
    )
}

/// Validates the catalog fence shared by all feature commit boundaries.
pub(crate) fn validate_prepared_mutation_commit_within_transaction(
    transaction: &Transaction<'_>,
    game_id: &GameId,
    mutation_id: &str,
    component_count: Option<usize>,
    has_baseline_mutations: bool,
) -> AppResult<PreparedMutationCommitBinding> {
    let state: Option<String> = transaction
        .query_row(
            "SELECT state FROM pending_file_mutations WHERE id = ?1 AND game_id = ?2",
            [mutation_id, game_id.as_str()],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage_error)?;
    if state.as_deref() != Some(PendingFileMutationState::Prepared.as_str()) {
        return Err(AppError::storage_failed(format!(
            "file mutation '{mutation_id}' is not prepared for game {}",
            game_id.as_str()
        )));
    }

    match classify_catalog_binding_within_transaction(transaction, game_id)? {
        CatalogBinding::CatalogAbsent => {
            if component_count.is_some_and(|count| count != 0) || has_baseline_mutations {
                return Err(AppError::storage_failed(format!(
                    "pre-catalog file mutation '{mutation_id}' cannot write component or baseline state"
                )));
            }
            Ok(PreparedMutationCommitBinding::CatalogAbsent)
        }
        CatalogBinding::CatalogPresent(CatalogReadiness::Invalidated {
            mutation_token: Some(token),
            ..
        }) if token == mutation_id => Ok(PreparedMutationCommitBinding::CatalogInvalidated),
        CatalogBinding::CatalogPresent(CatalogReadiness::NeverCompleted { .. }) => {
            let repaired = observations::invalidate_game_authority_within_transaction(
                transaction,
                game_id,
                "prepared_file_mutation",
                Some(mutation_id),
            )?;
            if matches!(
                repaired,
                CatalogReadiness::Invalidated {
                    mutation_token: Some(ref token),
                    ..
                } if token == mutation_id
            ) {
                Ok(PreparedMutationCommitBinding::CatalogInvalidated)
            } else {
                Err(AppError::storage_failed(
                    "late catalog binding did not produce matching invalidated authority",
                ))
            }
        }
        CatalogBinding::CatalogPresent(_) => Err(AppError::storage_failed(format!(
            "file mutation '{mutation_id}' has no matching invalidated scan authority"
        ))),
    }
}

/// Validates the typed OptiScaler journal and its aggregate proof at the
/// database commit boundary.  This function only inspects durable values; the
/// filesystem authority owns all native observations.
pub(in crate::repositories) fn validate_optiscaler_binding_within_transaction(
    transaction: &Transaction<'_>,
    game_id: &GameId,
    mutation_id: &str,
    expected_feature: &str,
    expected_subject: &str,
    binding: &OptiScalerAggregateBinding,
) -> AppResult<()> {
    let row: Option<(Option<String>, String, String)> = transaction
        .query_row(
            "SELECT subject_id, state, manifest_json
             FROM pending_file_mutations
             WHERE feature = :feature AND id = :id AND game_id = :game_id",
            named_params! {
                ":feature": expected_feature,
                ":id": mutation_id,
                ":game_id": game_id.as_str(),
            },
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(storage_error)?;
    let Some((subject, state, journal_json)) = row else {
        return Err(AppError::storage_failed(format!(
            "OptiScaler mutation '{mutation_id}' is missing for game {} or has the wrong feature",
            game_id.as_str()
        )));
    };
    if state != PendingFileMutationState::Prepared.as_str() {
        return Err(AppError::storage_failed(format!(
            "OptiScaler mutation '{mutation_id}' is not prepared"
        )));
    }
    if subject.as_deref() != Some(expected_subject) {
        return Err(AppError::storage_failed(format!(
            "OptiScaler mutation '{mutation_id}' has a mismatched topology subject"
        )));
    }
    let journal = parse_journal(&journal_json, "prepared OptiScaler journal")?;
    validate_prepared_journal(&journal)?;
    validate_namespace_custody(&journal)?;
    validate_binding_against_journal(&journal, binding)
}

/// Validates a journal JSON object at any durable lifecycle boundary.
#[cfg(test)]
pub(in crate::repositories) fn validate_optiscaler_journal_json(
    journal_json: &str,
) -> AppResult<()> {
    let journal = parse_journal(journal_json, "OptiScaler journal")?;
    validate_journal_shape(&journal)
}

/// Validates the initial row written before native namespace materialization.
pub(in crate::repositories) fn validate_optiscaler_journal_for_begin(
    journal_json: &str,
) -> AppResult<()> {
    let journal = parse_journal(journal_json, "initial OptiScaler journal")?;
    validate_journal_shape(&journal)?;
    if journal.threat_model() != ThreatModel::CooperativeSameUid
        || journal.materialization() != &MaterializationState::Planned
        || journal.cleanup() != &renderpilot_domain::CleanupState::Inactive
    {
        return Err(AppError::storage_failed(
            "initial OptiScaler journal has an invalid lifecycle state",
        ));
    }
    if journal.control_namespace().identity().is_some() {
        return Err(AppError::storage_failed(
            "initial OptiScaler journal already contains a materialized namespace identity",
        ));
    }
    for workspace in journal.private_workspaces() {
        if workspace.identity().is_some() {
            return Err(AppError::storage_failed(
                "initial OptiScaler journal already contains a private workspace identity",
            ));
        }
    }
    Ok(())
}

/// Validates the complete journal required before the feature commit.
pub(in crate::repositories) fn validate_optiscaler_journal_for_prepared(
    journal_json: &str,
) -> AppResult<()> {
    let journal = parse_journal(journal_json, "prepared OptiScaler journal")?;
    validate_prepared_journal(&journal)?;
    validate_namespace_custody(&journal)
}

/// Parses and validates a journal selected for restart recovery.
///
/// Recovery needs the canonical value, not just a pass/fail result, so this
/// return-bearing seam shares the same parser and lifecycle overlay used by
/// the ordinary pending-row validators. It deliberately does not check a
/// terminal delete boundary: committed cleanup remains a later authority.
pub(in crate::repositories) fn parse_optiscaler_journal_for_recovery(
    journal_json: &str,
    row_state: &str,
) -> AppResult<OptiScalerJournal> {
    let journal = deserialize_journal(journal_json, "recovery OptiScaler journal")?;
    // The CAS shape validator is the canonical lifecycle-aware parser seam:
    // committed recovery may legally observe historical custody artifacts
    // already cleared by a previous cleanup step.
    validate_journal_shape_for_cas(&journal, row_state, None)?;
    let cleanup_lifecycle = match row_state {
        "preparing" | "prepared" => renderpilot_domain::OptiScalerCleanupLifecycle::Rollback,
        "committed" => renderpilot_domain::OptiScalerCleanupLifecycle::Committed,
        _ => {
            return Err(AppError::storage_failed(
                "OptiScaler recovery has an invalid pending row state",
            ));
        }
    };
    renderpilot_domain::validate_optiscaler_cleanup_overlay(&journal, cleanup_lifecycle)
        .map_err(|error| AppError::storage_failed(error.to_string()))?;
    Ok(journal)
}

/// Validates the exact terminal state required before a rollback row can be
/// deleted under an opaque recovery authority.
pub(in crate::repositories) fn validate_optiscaler_journal_for_rollback_terminal(
    journal_json: &str,
) -> AppResult<()> {
    let journal = parse_journal(journal_json, "OptiScaler rollback terminal journal")?;
    renderpilot_domain::validate_optiscaler_cleanup_overlay(
        &journal,
        renderpilot_domain::OptiScalerCleanupLifecycle::Rollback,
    )
    .map_err(|error| AppError::storage_failed(error.to_string()))?;
    if !journal.can_delete_after_rollback() {
        return Err(AppError::storage_failed(
            "OptiScaler rollback journal has not reached its exact terminal delete boundary",
        ));
    }
    Ok(())
}

/// Validates the exact terminal state required before a committed journal row
/// can be deleted after its storage-owned cleanup evidence is complete.
pub(in crate::repositories) fn validate_optiscaler_journal_for_committed_terminal(
    journal_json: &str,
) -> AppResult<()> {
    let journal =
        parse_committed_terminal_journal(journal_json, "committed OptiScaler terminal journal")?;
    renderpilot_domain::validate_optiscaler_cleanup_overlay(
        &journal,
        renderpilot_domain::OptiScalerCleanupLifecycle::Committed,
    )
    .map_err(|error| AppError::storage_failed(error.to_string()))?;
    if !journal.can_delete_after_commit() {
        return Err(AppError::storage_failed(
            "committed OptiScaler journal has not reached its exact terminal delete boundary",
        ));
    }
    Ok(())
}

/// Validates one compare-and-swap of the durable journal.
pub(in crate::repositories) fn validate_optiscaler_journal_for_cas(
    current_json: &str,
    next_json: &str,
    row_state: &str,
) -> AppResult<()> {
    let current = deserialize_journal(current_json, "current OptiScaler journal")?;
    let next = deserialize_journal(next_json, "next OptiScaler journal")?;
    let cleanup_lifecycle = match row_state {
        "preparing" | "prepared" => renderpilot_domain::OptiScalerCleanupLifecycle::Rollback,
        "committed" => renderpilot_domain::OptiScalerCleanupLifecycle::Committed,
        _ => {
            return Err(AppError::storage_failed(
                "OptiScaler CAS has an invalid pending row state",
            ));
        }
    };
    for journal in [&current, &next] {
        renderpilot_domain::validate_optiscaler_cleanup_overlay(journal, cleanup_lifecycle)
            .map_err(|error| AppError::storage_failed(error.to_string()))?;
    }
    let cleared_artifact = match (current.cleanup(), next.cleanup()) {
        (
            renderpilot_domain::CleanupState::ArtifactRemoveIntent {
                operation_id,
                artifact,
                ..
            },
            renderpilot_domain::CleanupState::Inactive,
        ) => Some((*operation_id, *artifact)),
        _ => None,
    };
    validate_journal_shape_for_cas(&current, row_state, None)?;
    validate_journal_shape_for_cas(&next, row_state, cleared_artifact)?;
    if row_state == PendingFileMutationState::Committed.as_str()
        && [&current, &next].into_iter().any(|journal| {
            matches!(
                journal.cleanup(),
                renderpilot_domain::CleanupState::Complete
            ) && !journal.can_delete_after_commit()
        })
    {
        return Err(AppError::storage_failed(
            "OptiScaler Complete cleanup lacks applied post-commit directories or empty namespaces",
        ));
    }
    validate_materialization_identity_state(&next)?;
    validate_materialization_transition(
        current.materialization(),
        next.materialization(),
        next.private_workspaces().len(),
    )?;
    if next.materialization() == &MaterializationState::Ready {
        validate_namespace_custody(&next)?;
    }
    validate_immutable_program(&current, &next)?;
    validate_identity_progress(&current, &next)?;
    validate_expected_after_progress(&current, &next)?;
    validate_effect_progress(&current, &next)?;
    validate_lifecycle_progress(&current, &next, row_state)?;
    validate_cas_frontier(&current, &next, row_state)
}
