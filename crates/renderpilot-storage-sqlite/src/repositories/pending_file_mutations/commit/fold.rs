use super::prelude::*;
use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::repositories) enum ActionKind {
    Write,
    Delete,
    Verify,
    Relocate,
    CreateDirectory,
    PostCommitRemoveDirectory,
}

#[derive(Debug, Clone)]
pub(in crate::repositories) struct FoldEntry {
    pub(in crate::repositories) operation_id: u32,
    pub(in crate::repositories) endpoint: Endpoint,
    pub(in crate::repositories) path: String,
    pub(in crate::repositories) preimage: Preimage,
    pub(in crate::repositories) before: DurableObservation,
    pub(in crate::repositories) after: Option<DurableObservation>,
    pub(in crate::repositories) action: ActionKind,
}

pub(in crate::repositories) fn validate_binding_against_journal(
    journal: &OptiScalerJournal,
    binding: &OptiScalerAggregateBinding,
) -> AppResult<()> {
    let entries = collect_fold_entries(journal)?;
    let mut by_path: BTreeMap<String, Vec<&FoldEntry>> = BTreeMap::new();
    for entry in &entries {
        by_path
            .entry(normalized_path_key(&entry.path))
            .or_default()
            .push(entry);
    }
    let auxiliary_directory_paths =
        validate_auxiliary_directory_closure(journal, binding, &by_path)?;
    let mut allowed = binding.known_paths.clone();
    allowed.extend(binding.paths.keys().cloned());
    allowed.extend(binding.after_claims.keys().cloned());
    for auxiliary in &binding.auxiliary {
        allowed.insert(normalized_path_key(auxiliary.source.as_str()));
        allowed.insert(normalized_path_key(auxiliary.destination.as_str()));
    }
    allowed.extend(auxiliary_directory_paths.iter().cloned());
    let auxiliary_neutral_paths = binding
        .auxiliary
        .iter()
        .map(|auxiliary| normalized_path_key(auxiliary.destination.as_str()))
        .chain(auxiliary_directory_paths.iter().cloned())
        .collect::<BTreeSet<_>>();
    for entry in &entries {
        let key = normalized_path_key(&entry.path);
        if !allowed.contains(&key) {
            return Err(AppError::storage_failed(format!(
                "OptiScaler journal effect at '{}' is outside the typed aggregate",
                entry.path
            )));
        }
    }
    validate_retained_fsr_custody(binding, &by_path)?;
    for (key, bound) in &binding.paths {
        let path_entries = by_path.get(key).map_or(&[][..], Vec::as_slice);
        validate_bound_path(key, bound, path_entries, &by_path, binding)?;
    }
    for (key, bound) in &binding.after_claims {
        if !binding.paths.contains_key(key) {
            let path_entries = by_path.get(key).map_or(&[][..], Vec::as_slice);
            validate_bound_path(key, bound, path_entries, &by_path, binding)?;
        }
    }
    for key in &binding.known_paths {
        if binding.paths.contains_key(key)
            || binding.after_claims.contains_key(key)
            || auxiliary_neutral_paths.contains(key)
        {
            continue;
        }
        if let Some(path_entries) = by_path.get(key) {
            for entry in path_entries {
                if entry.action != ActionKind::Verify || entry.after.as_ref() != Some(&entry.before)
                {
                    return Err(AppError::storage_failed(format!(
                        "OptiScaler neutral path '{}' (normalized '{}', action {:?}, before {:?}, after {:?}) was not an exact verification",
                        entry.path, key, entry.action, entry.before, entry.after,
                    )));
                }
            }
        }
    }
    validate_auxiliary_preservations(binding, &by_path)
}

/// Retained AMD FSR originals are the only Reused custody that must remain
/// physically present throughout an OptiScaler update. Generic Reused paths
/// intentionally keep their permissive handoff rules; this explicit binding
/// requires the one exact live verification sealed by the journal.
fn validate_retained_fsr_custody(
    binding: &OptiScalerAggregateBinding,
    by_path: &BTreeMap<String, Vec<&FoldEntry>>,
) -> AppResult<()> {
    for (key, original) in &binding.retained_fsr_custody {
        let Some(bound) = binding.paths.get(key) else {
            return Err(AppError::storage_failed(
                "retained FSR custody has no aggregate path binding",
            ));
        };
        if !matches!(
            &bound.transition,
            OptiScalerBoundTransition::RelinquishReusedFile { installed }
                if installed == original
        ) {
            return Err(AppError::storage_failed(
                "retained FSR custody has no exact Reused handoff binding",
            ));
        }
        let entries = by_path.get(key).map_or(&[][..], Vec::as_slice);
        if entries.len() != 1 || entries[0].action != ActionKind::Verify {
            return Err(AppError::storage_failed(format!(
                "retained FSR custody at '{}' must have exactly one Verify",
                bound.path
            )));
        }
        let entry = entries[0];
        let expected = file_observation(original);
        match (&entry.preimage, &entry.before, entry.after.as_ref()) {
            (
                Preimage::Initial {
                    observation,
                    receipt: Some(initial),
                    owned_basis: None,
                },
                before,
                Some(after),
            ) if initial == original
                && observation == &expected
                && before == &expected
                && after == &expected => {}
            _ => {
                return Err(AppError::storage_failed(format!(
                    "retained FSR custody at '{}' is not an exact original Verify",
                    bound.path
                )));
            }
        }
    }
    Ok(())
}

/// Returns only the missing parent directories that are proven to be part of
/// an auxiliary destination's direct dependency chain.  Auxiliary source and
/// destination paths remain independently authorized below; this closure is
/// deliberately not a general path allowlist.
pub(in crate::repositories) fn validate_auxiliary_directory_closure(
    journal: &OptiScalerJournal,
    binding: &OptiScalerAggregateBinding,
    by_path: &BTreeMap<String, Vec<&FoldEntry>>,
) -> AppResult<BTreeSet<String>> {
    let mut authorized = BTreeSet::new();
    for auxiliary in &binding.auxiliary {
        let destination_key = normalized_path_key(auxiliary.destination.as_str());
        let destination_entries = by_path.get(&destination_key).map_or(&[][..], Vec::as_slice);
        let mut writes = destination_entries
            .iter()
            .filter(|entry| entry.action == ActionKind::Write)
            .copied();
        let (Some(destination_write), None) = (writes.next(), writes.next()) else {
            return Err(AppError::storage_failed(format!(
                "OptiScaler auxiliary destination '{}' must have exactly one journal Write",
                auxiliary.destination.as_str()
            )));
        };
        let mut parents = Vec::new();
        for (path_key, entries) in by_path {
            if !is_strict_component_ancestor(path_key, &destination_key) {
                continue;
            }
            if entries.len() != 1 || entries[0].action != ActionKind::CreateDirectory {
                return Err(AppError::storage_failed(format!(
                    "OptiScaler auxiliary destination '{}' has a non-create ancestor effect at '{}'",
                    auxiliary.destination.as_str(),
                    path_key
                )));
            }
            let entry = entries[0];
            if entry.operation_id >= destination_write.operation_id
                || entry.before != DurableObservation::Absent
                || !matches!(entry.after, Some(DurableObservation::Directory { .. }))
            {
                return Err(AppError::storage_failed(format!(
                    "OptiScaler auxiliary parent '{}' is not an exact parent-first directory fold",
                    entry.path
                )));
            }
            if !journal
                .roots()
                .iter()
                .any(|root| is_lexically_within(path_key, root))
            {
                return Err(AppError::storage_failed(format!(
                    "OptiScaler auxiliary parent '{}' is outside journal custody roots",
                    entry.path
                )));
            }
            parents.push(entry);
        }
        parents.sort_by(|left, right| {
            component_depth(&left.path)
                .cmp(&component_depth(&right.path))
                .then_with(|| {
                    normalized_path_key(&left.path).cmp(&normalized_path_key(&right.path))
                })
        });
        for pair in parents.windows(2) {
            if !has_direct_parent_dependency(journal, pair[1].operation_id, pair[0].operation_id) {
                return Err(AppError::storage_failed(
                    "OptiScaler auxiliary parent directories are not a direct parent-first dependency chain",
                ));
            }
        }
        if let Some(deepest) = parents.last()
            && !has_direct_parent_dependency(
                journal,
                destination_write.operation_id,
                deepest.operation_id,
            )
        {
            return Err(AppError::storage_failed(
                "OptiScaler auxiliary destination Write is not directly dependent on its deepest parent",
            ));
        }
        authorized.extend(
            parents
                .into_iter()
                .map(|entry| normalized_path_key(&entry.path)),
        );
    }
    Ok(authorized)
}

pub(in crate::repositories) fn is_strict_component_ancestor(
    ancestor: &str,
    descendant: &str,
) -> bool {
    let ancestor_key = normalized_path_key(ancestor);
    let descendant_key = normalized_path_key(descendant);
    ancestor_key != descendant_key && is_lexically_within(&descendant_key, &ancestor_key)
}

pub(in crate::repositories) fn has_direct_parent_dependency(
    journal: &OptiScalerJournal,
    child_operation_id: u32,
    parent_operation_id: u32,
) -> bool {
    let Some(child) = usize::try_from(child_operation_id)
        .ok()
        .and_then(|idx| journal.operations().get(idx))
    else {
        return false;
    };
    let Some(parent) = usize::try_from(parent_operation_id)
        .ok()
        .and_then(|idx| journal.operations().get(idx))
    else {
        return false;
    };
    if !child.parent_dependencies().contains(&parent_operation_id)
        || !matches!(parent.effect(), OperationEffect::CreateDirectory(_))
    {
        return false;
    }
    let Some(parent_endpoint) = parent.effect().endpoints().into_iter().next() else {
        return false;
    };
    let Some(child_endpoint) = child.effect().endpoints().into_iter().next() else {
        return false;
    };
    direct_parent_lexical(child_endpoint.path()).is_some_and(|direct_parent| {
        normalized_path_key(&direct_parent) == normalized_path_key(parent_endpoint.path())
    })
}

pub(in crate::repositories) fn validate_bound_path(
    key: &str,
    bound: &OptiScalerBoundPath,
    entries: &[&FoldEntry],
    by_path: &BTreeMap<String, Vec<&FoldEntry>>,
    binding: &OptiScalerAggregateBinding,
) -> AppResult<()> {
    use OptiScalerBoundTransition::*;
    let path = bound.path.as_str();
    match &bound.transition {
        Relocate {
            source,
            destination,
        } => {
            validate_exact_relocation_pair(path, source, destination, entries, by_path, binding)?;
        }
        RelocateThenClaim {
            source,
            destination,
            installed,
        } => {
            validate_relocate_then_claim(
                path,
                source,
                destination,
                installed,
                entries,
                by_path,
                binding,
            )?;
        }
        RelocatePeerBaseline { digest } => {
            validate_peer_baseline_relocation(key, digest, entries, by_path, binding)?;
        }
        ClaimedFile { installed } => {
            if installed.ownership() != FileOwnership::Owned {
                return Err(AppError::storage_failed(
                    "OptiScaler claimed file must carry Owned custody",
                ));
            }
            expect_file_transition(
                entries,
                path,
                &DurableObservation::Absent,
                &file_observation(installed),
            )?;
            require_action(entries, &[ActionKind::Write, ActionKind::Verify], path)?;
        }
        ReplaceClaimedFile { before, after } => {
            if before.ownership() != FileOwnership::Owned
                || after.ownership() != FileOwnership::Owned
                || before.identity() != after.identity()
            {
                return Err(AppError::storage_failed(
                    "OptiScaler replacement changed Owned custody identity",
                ));
            }
            expect_file_transition(
                entries,
                path,
                &file_observation(before),
                &file_observation(after),
            )?;
            let has_write = entries
                .iter()
                .any(|entry| entry.action == ActionKind::Write);
            let has_delete_and_relocate = entries
                .iter()
                .any(|entry| entry.action == ActionKind::Delete)
                && entries
                    .iter()
                    .any(|entry| entry.action == ActionKind::Relocate);
            if !has_write && !has_delete_and_relocate {
                return Err(AppError::storage_failed(format!(
                    "OptiScaler replacement at '{path}' has no write or delete-to-relocate fold"
                )));
            }
        }
        AcquireReusedFile {
            prior,
            installed,
            mode,
            configuration_baseline,
        } => {
            if prior.ownership() != FileOwnership::Reused
                || installed.ownership() != FileOwnership::Owned
            {
                return Err(AppError::storage_failed(
                    "OptiScaler acquisition has invalid custody",
                ));
            }
            let Some(first) = entries.first() else {
                return Err(AppError::storage_failed(format!(
                    "OptiScaler acquisition at '{path}' has no journal effect"
                )));
            };
            match &first.before {
                DurableObservation::Absent => {
                    // A missing Reused configuration has no live preimage
                    // receipt to carry through the journal. Its only legal
                    // merge input is therefore the immutable Reused receipt
                    // retained by the adopted predecessor. Do not accept a
                    // caller-selected baseline merely because the endpoint is
                    // absent: that would let an unrelated digest enter the
                    // durable configuration lineage without any evidence.
                    if *mode == ReusedAcquisitionMode::Configuration
                        && configuration_baseline.as_ref() != Some(prior)
                    {
                        return Err(AppError::storage_failed(format!(
                            "OptiScaler absent configuration acquisition at '{path}' does not retain its adopted baseline"
                        )));
                    }
                }
                DurableObservation::File { identity, digest } => {
                    if (*mode == ReusedAcquisitionMode::ExactOrAbsent
                        && (identity != prior.identity() || digest != prior.digest()))
                        || initial_receipt(entries).is_none_or(|receipt| {
                            receipt.ownership() != FileOwnership::Reused
                                || receipt.identity() != identity
                                || receipt.digest() != digest
                        })
                        || installed.identity() != identity
                        || (*mode == ReusedAcquisitionMode::Configuration
                            && initial_receipt(entries) != configuration_baseline.as_ref())
                    {
                        return Err(AppError::storage_failed(format!(
                            "OptiScaler acquisition at '{path}' does not bind its exact Reused preimage"
                        )));
                    }
                }
                _ => {
                    return Err(AppError::storage_failed(format!(
                        "OptiScaler acquisition at '{path}' has a non-file preimage"
                    )));
                }
            }
            if (*mode == ReusedAcquisitionMode::Configuration) != configuration_baseline.is_some() {
                return Err(AppError::storage_failed(format!(
                    "OptiScaler acquisition at '{path}' has an invalid configuration baseline binding"
                )));
            }
            expect_optional_observation(
                entries.last().and_then(|entry| entry.after.as_ref()),
                &file_observation(installed),
                path,
            )?;
            require_action(entries, &[ActionKind::Write], path)?;
        }
        RecreateOwnedFile { prior, installed } => {
            if prior.ownership() != FileOwnership::Owned
                || installed.ownership() != FileOwnership::Owned
            {
                return Err(AppError::storage_failed(
                    "OptiScaler recreation has invalid Owned custody",
                ));
            }
            expect_file_transition(
                entries,
                path,
                &DurableObservation::Absent,
                &file_observation(installed),
            )?;
            require_action(entries, &[ActionKind::Write], path)?;
        }
        ClaimedDirectory { identity } => {
            expect_file_transition(
                entries,
                path,
                &DurableObservation::Absent,
                &DurableObservation::Directory {
                    identity: identity.clone(),
                },
            )?;
            require_action(
                entries,
                &[ActionKind::CreateDirectory, ActionKind::Verify],
                path,
            )?;
        }
        RemoveOwnedFile {
            installed,
            restoration,
            allow_absent,
        } => {
            if installed.ownership() != FileOwnership::Owned {
                return Err(AppError::storage_failed(
                    "OptiScaler removal requires Owned installed custody",
                ));
            }
            let expected_after = restoration
                .as_ref()
                .map_or(DurableObservation::Absent, file_observation);
            if entries
                .first()
                .is_some_and(|entry| entry.before == DurableObservation::Absent)
            {
                validate_absent_removal(entries, path, &expected_after, *allow_absent)?;
            } else {
                if initial_receipt(entries) != Some(installed) {
                    return Err(AppError::storage_failed(format!(
                        "OptiScaler Owned removal at '{path}' has no exact receipt"
                    )));
                }
                expect_file_transition(
                    entries,
                    path,
                    &file_observation(installed),
                    &expected_after,
                )?;
                if restoration.is_some() {
                    validate_relocation_destination_after_outer_release(
                        path,
                        installed,
                        restoration.as_ref().expect("checked above"),
                        entries,
                        by_path,
                        binding,
                    )?;
                } else if !entries
                    .iter()
                    .any(|entry| matches!(entry.action, ActionKind::Delete | ActionKind::Write))
                {
                    validate_remove_owned_relocation_source(
                        path,
                        installed,
                        &expected_after,
                        entries,
                        by_path,
                        binding,
                    )?;
                }
            }
        }
        RemoveReusedFile {
            installed,
            restoration,
            allow_absent,
        } => {
            if installed.ownership() != FileOwnership::Reused {
                return Err(AppError::storage_failed(
                    "OptiScaler Reused removal has invalid custody",
                ));
            }
            let expected_after = restoration
                .as_ref()
                .map_or(DurableObservation::Absent, file_observation);
            if entries
                .first()
                .is_some_and(|entry| entry.before == DurableObservation::Absent)
            {
                validate_absent_removal(entries, path, &expected_after, *allow_absent)?;
            } else {
                if initial_receipt(entries) != Some(installed) {
                    return Err(AppError::storage_failed(format!(
                        "OptiScaler Reused removal at '{path}' has no exact receipt"
                    )));
                }
                expect_file_transition(
                    entries,
                    path,
                    &file_observation(installed),
                    &expected_after,
                )?;
                if let Some(restoration) = restoration {
                    validate_relocation_destination_after_outer_release(
                        path,
                        installed,
                        restoration,
                        entries,
                        by_path,
                        binding,
                    )?;
                } else {
                    require_action(entries, &[ActionKind::Delete], path)?;
                }
            }
        }
        RestoreOwnedFile {
            installed,
            restoration,
        } => {
            if installed.ownership() != FileOwnership::Owned
                || restoration.ownership() != FileOwnership::Reused
                || installed.identity() != restoration.identity()
            {
                return Err(AppError::storage_failed(
                    "OptiScaler configuration restoration changed exact custody identity",
                ));
            }
            if entries.len() == 1
                && entries[0].action == ActionKind::Verify
                && entries[0].before == DurableObservation::Absent
                && entries[0].after.as_ref() == Some(&DurableObservation::Absent)
            {
                return Ok(());
            }
            if entries.len() != 1 || entries[0].action != ActionKind::Write {
                return Err(AppError::storage_failed(format!(
                    "OptiScaler configuration restoration at '{path}' must have exactly one terminal Write"
                )));
            }
            expect_file_transition(
                entries,
                path,
                &file_observation(installed),
                &file_observation(restoration),
            )?;
        }
        VerifyOwnedFile { prior, current } => {
            if prior.ownership() != FileOwnership::Owned
                || current.ownership() != FileOwnership::Owned
                || prior.identity() != current.identity()
            {
                return Err(AppError::storage_failed(
                    "OptiScaler verification changed Owned custody identity",
                ));
            }
            expect_file_transition(
                entries,
                path,
                &file_observation(prior),
                &file_observation(current),
            )?;
            require_action(entries, &[ActionKind::Verify], path)?;
        }
        RelinquishReusedFile { installed } => {
            if installed.ownership() != FileOwnership::Reused {
                return Err(AppError::storage_failed(
                    "OptiScaler retained handoff must carry Reused custody",
                ));
            }
            if entries.is_empty() {
                return Ok(());
            }
            if entries.len() == 1
                && entries[0].action == ActionKind::Verify
                && entries[0].before == DurableObservation::Absent
                && entries[0].after.as_ref() == Some(&DurableObservation::Absent)
            {
                return Ok(());
            }
            expect_file_transition(
                entries,
                path,
                &file_observation(installed),
                &file_observation(installed),
            )?;
            require_action(entries, &[ActionKind::Verify], path)?;
        }
        ObserveReusedConfiguration { persisted } => {
            if persisted.ownership() != FileOwnership::Reused {
                return Err(AppError::storage_failed(
                    "OptiScaler configuration observation requires Reused adoption custody",
                ));
            }
            if entries.len() != 1 || entries[0].action != ActionKind::Verify {
                return Err(AppError::storage_failed(format!(
                    "OptiScaler configuration observation at '{path}' must have exactly one Verify"
                )));
            }
            let entry = entries[0];
            match (&entry.preimage, &entry.before, entry.after.as_ref()) {
                (
                    Preimage::Initial {
                        observation: DurableObservation::File { identity, digest },
                        receipt: Some(observed),
                        owned_basis: None,
                    },
                    DurableObservation::File {
                        identity: before_identity,
                        digest: before_digest,
                    },
                    Some(after),
                ) if observed.ownership() == FileOwnership::Reused
                    && observed.identity() == identity
                    && observed.digest() == digest
                    && before_identity == identity
                    && before_digest == digest
                    && after == &entry.before => {}
                (
                    Preimage::Initial {
                        observation: DurableObservation::Absent,
                        receipt: None,
                        owned_basis: None,
                    },
                    DurableObservation::Absent,
                    Some(DurableObservation::Absent),
                ) => {}
                _ => {
                    return Err(AppError::storage_failed(format!(
                        "OptiScaler configuration observation at '{path}' is not an exact live Reused verify"
                    )));
                }
            }
        }
        PostCommitRemoveDirectory { identity } => {
            let Some(entry) = entries
                .iter()
                .find(|entry| entry.action == ActionKind::PostCommitRemoveDirectory)
            else {
                return Err(AppError::storage_failed(format!(
                    "OptiScaler directory cleanup at '{path}' has no matching journal effect"
                )));
            };
            expect_observation(
                &entry.before,
                &DurableObservation::Directory {
                    identity: identity.clone(),
                },
                path,
                "directory cleanup preimage",
            )?;
            if let Some(after) = &entry.after {
                expect_observation(
                    after,
                    &DurableObservation::Absent,
                    path,
                    "directory cleanup postimage",
                )?;
            }
        }
    }
    Ok(())
}

fn initial_receipt<'a>(entries: &'a [&'a FoldEntry]) -> Option<&'a FileReceipt> {
    entries.first().and_then(|entry| match &entry.preimage {
        Preimage::Initial { receipt, .. } => receipt.as_ref(),
        Preimage::PriorPostimage { .. } => None,
    })
}

fn validate_absent_removal(
    entries: &[&FoldEntry],
    path: &str,
    expected_after: &DurableObservation,
    allow_absent: bool,
) -> AppResult<()> {
    if !allow_absent {
        return Err(AppError::storage_failed(format!(
            "OptiScaler absent removal at '{path}' is not authorized"
        )));
    }
    expect_file_transition(entries, path, &DurableObservation::Absent, expected_after)?;
    if expected_after == &DurableObservation::Absent {
        require_action(entries, &[ActionKind::Verify], path)
    } else {
        require_action(entries, &[ActionKind::Relocate], path)
    }
}
