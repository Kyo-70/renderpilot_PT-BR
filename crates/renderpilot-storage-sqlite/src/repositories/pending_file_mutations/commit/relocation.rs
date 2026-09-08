use super::prelude::*;
use super::*;

pub(in crate::repositories) fn validate_exact_relocation_pair(
    path: &str,
    source: &FileReceipt,
    destination: &FileReceipt,
    entries: &[&FoldEntry],
    by_path: &BTreeMap<String, Vec<&FoldEntry>>,
    binding: &OptiScalerAggregateBinding,
) -> AppResult<()> {
    if entries.len() != 1 || entries[0].action != ActionKind::Relocate {
        return Err(AppError::storage_failed(format!(
            "OptiScaler relocation at '{path}' must have exactly one typed endpoint"
        )));
    }
    let entry = entries[0];
    let current_key = normalized_path_key(path);
    let mut counterparts = binding
        .paths
        .iter()
        .chain(binding.after_claims.iter())
        .filter(|(key, candidate)| {
            *key != &current_key
                && matches!(
                    &candidate.transition,
                    OptiScalerBoundTransition::Relocate {
                        source: candidate_source,
                        destination: candidate_destination,
                    } | OptiScalerBoundTransition::RelocateThenClaim {
                        source: candidate_source,
                        destination: candidate_destination,
                        ..
                    } if candidate_source == source && candidate_destination == destination
                )
        });
    let (counterpart_key, counterpart_candidate) = match (counterparts.next(), counterparts.next())
    {
        (None, _) => {
            let mut removal_counterparts = binding
                .paths
                .iter()
                .chain(binding.after_claims.iter())
                .filter(|(key, candidate)| {
                    *key != &current_key
                        && match &candidate.transition {
                            OptiScalerBoundTransition::RemoveOwnedFile {
                                installed,
                                restoration,
                                ..
                            }
                            | OptiScalerBoundTransition::RemoveReusedFile {
                                installed,
                                restoration,
                                ..
                            } => restoration
                                .as_ref()
                                .map_or(installed == source, |value| value == destination),
                            _ => false,
                        }
                });
            if let (Some((counterpart_key, counterpart)), None) =
                (removal_counterparts.next(), removal_counterparts.next())
            {
                let counterpart_entries =
                    by_path.get(counterpart_key).map_or(&[][..], Vec::as_slice);
                match &counterpart.transition {
                    OptiScalerBoundTransition::RemoveOwnedFile {
                        installed,
                        restoration: None,
                        ..
                    } => {
                        if entry.endpoint != Endpoint::Destination {
                            return Err(AppError::storage_failed(
                                "OptiScaler relocation destination counterpart must be Destination",
                            ));
                        }
                        if counterpart_entries.len() != 1
                            || counterpart_entries[0].action != ActionKind::Relocate
                            || counterpart_entries[0].endpoint != Endpoint::Source
                            || counterpart_entries[0].operation_id != entry.operation_id
                        {
                            return Err(AppError::storage_failed(
                                "OptiScaler relocation source has extra touches",
                            ));
                        }
                        expect_observation(
                            &counterpart_entries[0].before,
                            &file_observation(installed),
                            counterpart.path.as_str(),
                            "relocation source preimage",
                        )?;
                        expect_optional_observation(
                            counterpart_entries[0].after.as_ref(),
                            &DurableObservation::Absent,
                            counterpart.path.as_str(),
                        )?;
                        let operation_entries = by_path
                            .values()
                            .flat_map(|path_entries| path_entries.iter().copied())
                            .filter(|candidate| candidate.operation_id == entry.operation_id)
                            .count();
                        if operation_entries != 2 {
                            return Err(AppError::storage_failed(
                                "OptiScaler relocation operation has extra touches",
                            ));
                        }
                        expect_observation(
                            &entry.before,
                            &DurableObservation::Absent,
                            path,
                            "relocation destination preimage",
                        )?;
                        return expect_optional_observation(
                            entry.after.as_ref(),
                            &file_observation(destination),
                            path,
                        );
                    }
                    OptiScalerBoundTransition::RemoveOwnedFile {
                        installed,
                        restoration: Some(restoration),
                        ..
                    }
                    | OptiScalerBoundTransition::RemoveReusedFile {
                        installed,
                        restoration: Some(restoration),
                        ..
                    } => {
                        validate_relocation_destination_after_outer_release(
                            counterpart.path.as_str(),
                            installed,
                            restoration,
                            counterpart_entries,
                            by_path,
                            binding,
                        )?;
                        return Ok(());
                    }
                    _ => unreachable!("removal counterparts are filtered to exact file removal"),
                }
            }
            return Err(AppError::storage_failed(format!(
                "OptiScaler relocation at '{path}' has no unique typed counterpart"
            )));
        }
        (Some(single), None) => single,
        (Some(_), Some(_)) => {
            return Err(AppError::storage_failed(format!(
                "OptiScaler relocation at '{path}' has no unique typed counterpart"
            )));
        }
    };
    let counterpart_entries = by_path.get(counterpart_key).map_or(&[][..], Vec::as_slice);
    let counterpart_is_composite = matches!(
        &counterpart_candidate.transition,
        OptiScalerBoundTransition::RelocateThenClaim { .. }
    );
    let relocation_count = counterpart_entries
        .iter()
        .filter(|candidate| candidate.action == ActionKind::Relocate)
        .count();
    if relocation_count != 1
        || (!counterpart_is_composite && counterpart_entries.len() != 1)
        || (counterpart_is_composite && counterpart_entries.len() != 2)
    {
        return Err(AppError::storage_failed(
            "OptiScaler relocation counterpart has extra touches",
        ));
    }
    let Some(counterpart_entry) = counterpart_entries
        .iter()
        .copied()
        .find(|candidate| candidate.action == ActionKind::Relocate)
    else {
        return Err(AppError::storage_failed(
            "OptiScaler relocation counterpart has extra touches",
        ));
    };
    if !matches!(entry.endpoint, Endpoint::Source | Endpoint::Destination)
        || !matches!(
            counterpart_entry.endpoint,
            Endpoint::Source | Endpoint::Destination
        )
        || entry.endpoint == counterpart_entry.endpoint
    {
        return Err(AppError::storage_failed(
            "OptiScaler relocation endpoints do not form one Source/Destination pair",
        ));
    }
    if entry.operation_id != counterpart_entry.operation_id {
        return Err(AppError::storage_failed(
            "OptiScaler relocation source and destination use different operations",
        ));
    }
    let operation_entries = by_path
        .values()
        .flat_map(|path_entries| path_entries.iter().copied())
        .filter(|candidate| candidate.operation_id == entry.operation_id)
        .collect::<Vec<_>>();
    if operation_entries.len() != 2
        || !operation_entries
            .iter()
            .any(|candidate| candidate.endpoint == Endpoint::Source)
        || !operation_entries
            .iter()
            .any(|candidate| candidate.endpoint == Endpoint::Destination)
    {
        return Err(AppError::storage_failed(
            "OptiScaler relocation operation has extra touches or missing endpoint",
        ));
    }
    let source_entry = if entry.endpoint == Endpoint::Source {
        entry
    } else {
        counterpart_entry
    };
    let destination_entry = if entry.endpoint == Endpoint::Destination {
        entry
    } else {
        counterpart_entry
    };
    expect_observation(
        &source_entry.before,
        &file_observation(source),
        path,
        "relocation source preimage",
    )?;
    expect_optional_observation(
        source_entry.after.as_ref(),
        &DurableObservation::Absent,
        path,
    )?;
    expect_observation(
        &destination_entry.before,
        &DurableObservation::Absent,
        path,
        "relocation destination preimage",
    )?;
    expect_optional_observation(
        destination_entry.after.as_ref(),
        &file_observation(destination),
        path,
    )
}

pub(in crate::repositories) fn validate_relocate_then_claim(
    path: &str,
    source: &FileReceipt,
    destination: &FileReceipt,
    installed: &FileReceipt,
    entries: &[&FoldEntry],
    by_path: &BTreeMap<String, Vec<&FoldEntry>>,
    binding: &OptiScalerAggregateBinding,
) -> AppResult<()> {
    if installed.ownership() != FileOwnership::Owned || entries.len() != 2 {
        return Err(AppError::storage_failed(
            "OptiScaler relocated root claim requires one relocation and one owned claim",
        ));
    }
    let relocation_entries = entries
        .iter()
        .copied()
        .filter(|entry| entry.action == ActionKind::Relocate)
        .collect::<Vec<_>>();
    let [relocation] = relocation_entries.as_slice() else {
        return Err(AppError::storage_failed(
            "OptiScaler relocated root claim has no unique relocation endpoint",
        ));
    };
    if relocation.endpoint != Endpoint::Source {
        return Err(AppError::storage_failed(
            "OptiScaler relocated root claim must vacate its source endpoint",
        ));
    }
    let claim = entries
        .iter()
        .copied()
        .find(|entry| entry.action == ActionKind::Write)
        .ok_or_else(|| {
            AppError::storage_failed(
                "OptiScaler relocated root claim has no subsequent write endpoint",
            )
        })?;
    if claim.endpoint != Endpoint::Single
        || claim.operation_id <= relocation.operation_id
        || !matches!(
            claim.preimage,
            Preimage::PriorPostimage {
                operation_id,
                endpoint: Endpoint::Source,
            } if operation_id == relocation.operation_id
        )
    {
        return Err(AppError::storage_failed(
            "OptiScaler relocated root claim does not consume its relocation postimage",
        ));
    }
    validate_exact_relocation_pair(path, source, destination, &[relocation], by_path, binding)?;
    expect_observation(
        &claim.before,
        &DurableObservation::Absent,
        path,
        "relocated root claim preimage",
    )?;
    expect_optional_observation(claim.after.as_ref(), &file_observation(installed), path)
}

pub(in crate::repositories) fn validate_peer_baseline_relocation(
    key: &str,
    digest: &Sha256Hash,
    entries: &[&FoldEntry],
    by_path: &BTreeMap<String, Vec<&FoldEntry>>,
    binding: &OptiScalerAggregateBinding,
) -> AppResult<()> {
    if entries.len() != 1 || entries[0].action != ActionKind::Relocate {
        return Err(AppError::storage_failed(
            "OptiScaler peer baseline relocation must have one typed endpoint",
        ));
    }
    let counterparts = binding
        .paths
        .iter()
        .chain(binding.after_claims.iter())
        .filter(|(candidate_key, candidate)| {
            *candidate_key != key
                && matches!(
                    &candidate.transition,
                    OptiScalerBoundTransition::RelocatePeerBaseline {
                        digest: candidate_digest
                    } if candidate_digest == digest
                )
        })
        .collect::<Vec<_>>();
    let [(counterpart_key, _)] = counterparts.as_slice() else {
        return Err(AppError::storage_failed(
            "OptiScaler peer baseline relocation has no unique typed counterpart",
        ));
    };
    let counterpart_entries = by_path.get(*counterpart_key).map_or(&[][..], Vec::as_slice);
    if counterpart_entries.len() != 1 || counterpart_entries[0].action != ActionKind::Relocate {
        return Err(AppError::storage_failed(
            "OptiScaler peer baseline relocation counterpart has extra touches",
        ));
    }
    let entry = entries[0];
    let counterpart = counterpart_entries[0];
    if entry.operation_id != counterpart.operation_id
        || entry.endpoint == counterpart.endpoint
        || !matches!(entry.endpoint, Endpoint::Source | Endpoint::Destination)
        || !matches!(
            counterpart.endpoint,
            Endpoint::Source | Endpoint::Destination
        )
    {
        return Err(AppError::storage_failed(
            "OptiScaler peer baseline relocation endpoints are not one exact pair",
        ));
    }
    let count = by_path
        .values()
        .flat_map(|path_entries| path_entries.iter().copied())
        .filter(|candidate| candidate.operation_id == entry.operation_id)
        .count();
    if count != 2 {
        return Err(AppError::storage_failed(
            "OptiScaler peer baseline relocation has extra operation touches",
        ));
    }
    let source = if entry.endpoint == Endpoint::Source {
        entry
    } else {
        counterpart
    };
    let destination = if entry.endpoint == Endpoint::Destination {
        entry
    } else {
        counterpart
    };
    let Preimage::Initial {
        observation,
        receipt: Some(receipt),
        ..
    } = &source.preimage
    else {
        return Err(AppError::storage_failed(
            "OptiScaler peer baseline source has no exact initial receipt",
        ));
    };
    if observation != &source.before
        || file_observation(receipt) != source.before
        || receipt.digest() != digest
    {
        return Err(AppError::storage_failed(
            "OptiScaler peer baseline source receipt does not match its exact digest",
        ));
    }
    if !matches!(
        &destination.preimage,
        Preimage::Initial {
            observation: DurableObservation::Absent,
            receipt: None,
            ..
        }
    ) {
        return Err(AppError::storage_failed(
            "OptiScaler peer baseline destination must start absent",
        ));
    }
    expect_optional_observation(
        source.after.as_ref(),
        &DurableObservation::Absent,
        &source.path,
    )?;
    expect_observation(
        &destination.before,
        &DurableObservation::Absent,
        &destination.path,
        "peer baseline destination preimage",
    )?;
    expect_optional_observation(
        destination.after.as_ref(),
        &source.before,
        &destination.path,
    )
}

pub(in crate::repositories) fn validate_relocation_destination_after_outer_release(
    path: &str,
    installed: &FileReceipt,
    restoration: &FileReceipt,
    entries: &[&FoldEntry],
    by_path: &BTreeMap<String, Vec<&FoldEntry>>,
    binding: &OptiScalerAggregateBinding,
) -> AppResult<()> {
    if entries.len() != 2 {
        return Err(AppError::storage_failed(
            "OptiScaler outer release before relocation requires exactly two effects",
        ));
    }
    let release = entries[0];
    let relocation = entries[1];
    if release.endpoint != Endpoint::Single
        || relocation.action != ActionKind::Relocate
        || relocation.endpoint != Endpoint::Destination
        || relocation.operation_id != release.operation_id.saturating_add(1)
    {
        return Err(AppError::storage_failed(
            "OptiScaler outer release must precede Relocate d+1",
        ));
    }
    if !matches!(
        relocation.preimage,
        Preimage::PriorPostimage {
            operation_id,
            endpoint: Endpoint::Single,
        } if operation_id == release.operation_id
    ) {
        return Err(AppError::storage_failed(
            "OptiScaler relocation destination must consume the outer-release postimage",
        ));
    }
    match release.action {
        ActionKind::Delete => {
            if !matches!(release.preimage, Preimage::Initial { .. }) {
                return Err(AppError::storage_failed(
                    "OptiScaler outer delete before relocation must use its Initial preimage",
                ));
            }
            expect_observation(
                &release.before,
                &file_observation(installed),
                path,
                "outer delete before relocation preimage",
            )?;
            expect_optional_observation(release.after.as_ref(), &DurableObservation::Absent, path)?;
        }
        ActionKind::Verify => {
            if !matches!(
                release.preimage,
                Preimage::Initial {
                    observation: DurableObservation::Absent,
                    receipt: None,
                    owned_basis: None,
                }
            ) || release.before != DurableObservation::Absent
                || release.after.as_ref() != Some(&DurableObservation::Absent)
            {
                return Err(AppError::storage_failed(
                    "OptiScaler absent outer before relocation must be an exact Verify(Absent)",
                ));
            }
        }
        _ => {
            return Err(AppError::storage_failed(
                "OptiScaler outer release before relocation must be Delete or Verify(Absent)",
            ));
        }
    }
    expect_observation(
        &relocation.before,
        &DurableObservation::Absent,
        path,
        "outer-release relocation destination preimage",
    )?;
    expect_optional_observation(
        relocation.after.as_ref(),
        &file_observation(restoration),
        path,
    )?;

    let current_key = normalized_path_key(path);
    let mut source_candidates = binding
        .paths
        .iter()
        .chain(binding.after_claims.iter())
        .filter(|(key, candidate)| {
            *key != &current_key
                && matches!(
                    &candidate.transition,
                    OptiScalerBoundTransition::Relocate { destination, .. }
                        if destination == restoration
                )
        });
    let (Some((source_key, source_binding)), None) =
        (source_candidates.next(), source_candidates.next())
    else {
        return Err(AppError::storage_failed(
            "OptiScaler outer-release relocation has no unique typed source",
        ));
    };
    let OptiScalerBoundTransition::Relocate {
        source,
        destination,
    } = &source_binding.transition
    else {
        unreachable!("source candidates are filtered to relocation bindings")
    };
    if destination != restoration {
        return Err(AppError::storage_failed(
            "OptiScaler outer-release relocation changed its restoration receipt",
        ));
    }
    let source_entries = by_path.get(source_key).map_or(&[][..], Vec::as_slice);
    if source_entries.len() != 1
        || source_entries[0].operation_id != relocation.operation_id
        || source_entries[0].action != ActionKind::Relocate
        || source_entries[0].endpoint != Endpoint::Source
    {
        return Err(AppError::storage_failed(
            "OptiScaler outer-release relocation source is not the adjacent Relocate endpoint",
        ));
    }
    expect_observation(
        &source_entries[0].before,
        &file_observation(source),
        source_key,
        "outer-release relocation source preimage",
    )?;
    expect_optional_observation(
        source_entries[0].after.as_ref(),
        &DurableObservation::Absent,
        source_key,
    )?;
    let count = by_path
        .values()
        .flat_map(|path_entries| path_entries.iter().copied())
        .filter(|candidate| {
            candidate.operation_id == release.operation_id
                || candidate.operation_id == relocation.operation_id
        })
        .count();
    if count != 3 {
        return Err(AppError::storage_failed(
            "OptiScaler outer-release relocation has extra touches or consumers",
        ));
    }
    Ok(())
}

pub(in crate::repositories) fn validate_remove_owned_relocation_source(
    path: &str,
    installed: &FileReceipt,
    expected_after: &DurableObservation,
    source_entries: &[&FoldEntry],
    by_path: &BTreeMap<String, Vec<&FoldEntry>>,
    binding: &OptiScalerAggregateBinding,
) -> AppResult<()> {
    if !matches!(expected_after, DurableObservation::Absent) || source_entries.len() != 1 {
        return Err(AppError::storage_failed(format!(
            "OptiScaler removal at '{path}' has no exact no-replace relocation fold"
        )));
    }
    let source_entry = source_entries[0];
    if source_entry.action != ActionKind::Relocate || source_entry.endpoint != Endpoint::Source {
        return Err(AppError::storage_failed(format!(
            "OptiScaler removal at '{path}' has no exact relocation source"
        )));
    }
    let destination_entry = by_path
        .values()
        .flat_map(|entries| entries.iter().copied())
        .find(|entry| {
            entry.operation_id == source_entry.operation_id
                && entry.action == ActionKind::Relocate
                && entry.endpoint == Endpoint::Destination
        })
        .ok_or_else(|| {
            AppError::storage_failed(format!(
                "OptiScaler relocation source '{path}' has no matching destination"
            ))
        })?;
    let destination_key = normalized_path_key(&destination_entry.path);
    let destination_binding = binding
        .paths
        .get(&destination_key)
        .or_else(|| binding.after_claims.get(&destination_key))
        .ok_or_else(|| {
            AppError::storage_failed(format!(
                "OptiScaler relocation destination '{}' is not represented by the typed binding",
                destination_entry.path
            ))
        })?;
    let bound_destination = match &destination_binding.transition {
        OptiScalerBoundTransition::Relocate {
            source: bound_source,
            destination,
        } => {
            if bound_source != installed {
                return Err(AppError::storage_failed(format!(
                    "OptiScaler relocation source '{path}' changed its exact receipt"
                )));
            }
            destination
        }
        _ => {
            return Err(AppError::storage_failed(format!(
                "OptiScaler relocation destination '{}' has an unrelated binding",
                destination_entry.path
            )));
        }
    };
    if installed.identity() != bound_destination.identity()
        || installed.digest() != bound_destination.digest()
    {
        return Err(AppError::storage_failed(
            "OptiScaler relocation changed destination identity or digest",
        ));
    }
    expect_observation(
        &destination_entry.before,
        &DurableObservation::Absent,
        &destination_entry.path,
        "relocation destination preimage",
    )?;
    expect_optional_observation(
        destination_entry.after.as_ref(),
        &file_observation(bound_destination),
        &destination_entry.path,
    )
}
