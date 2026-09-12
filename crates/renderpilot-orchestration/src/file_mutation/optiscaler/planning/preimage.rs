fn planned_before(
    participant: &PlannedParticipant,
    wanted: OptiScalerAction,
    absent_dirs: &HashSet<String>,
    repeated: bool,
) -> Result<DiskObservation, ServiceError> {
    let virtual_absent = participant
        .path
        .ancestors()
        .skip(1)
        .any(|path| absent_dirs.contains(&crate::paths::normalized_key(path)));
    let actual = if virtual_absent {
        DiskObservation::Absent
    } else {
        observe(&participant.path)
    };
    match &participant.preimage {
        PlannedPreimage::Absent => {
            if !matches!(
                wanted,
                OptiScalerAction::Write
                    | OptiScalerAction::CreateDirectory
                    | OptiScalerAction::Verify
            ) || (!repeated && actual != DiskObservation::Absent)
            {
                return Err(token_drift(
                    &participant.path,
                    &DiskObservation::Absent,
                    &actual,
                ));
            }
            Ok(actual)
        }
        PlannedPreimage::Verify => {
            if wanted != OptiScalerAction::Verify
                || matches!(
                    actual,
                    DiskObservation::NonRegular | DiskObservation::Unreadable
                )
            {
                return Err(crate::failed(format!(
                    "verification preimage at {} is not an exact file or absence: {actual:?}",
                    participant.path.display()
                )));
            }
            Ok(actual)
        }
        PlannedPreimage::Exact {
            current,
            prior_owned,
        } => {
            current
                .validate()
                .map_err(|error| crate::failed(error.to_string()))?;
            if current.ownership() != FileOwnership::Owned {
                return Err(crate::failed(
                    "generic exact preimage requires an exact Owned receipt",
                ));
            }
            if matches!(wanted, OptiScalerAction::Write | OptiScalerAction::Delete)
                && (!current.authorizes_destructive_cleanup()
                    || prior_owned
                        .as_ref()
                        .is_none_or(|value| value.identity() != current.identity()))
            {
                return Err(crate::failed(
                    "destructive operation requires exact Owned authority",
                ));
            }
            let expected = DiskObservation::File {
                identity: current.identity().to_owned(),
                digest: current.digest().clone(),
            };
            if !virtual_absent && !repeated && actual != expected {
                return Err(token_drift(&participant.path, &expected, &actual));
            }
            Ok(expected)
        }
        PlannedPreimage::ExactReused { current, authority } => {
            current
                .validate()
                .map_err(|error| crate::failed(error.to_string()))?;
            if current.ownership() != FileOwnership::Reused {
                return Err(crate::failed(
                    "Reused mutation authority requires an exact Reused receipt",
                ));
            }
            let action_allowed = match authority {
                ReusedMutationAuthority::ObservationOnly => wanted == OptiScalerAction::Verify,
                ReusedMutationAuthority::ConfigurationWrite => wanted == OptiScalerAction::Write,
                ReusedMutationAuthority::OptiScalerArtifact => {
                    matches!(wanted, OptiScalerAction::Write | OptiScalerAction::Delete)
                }
                ReusedMutationAuthority::RelocationSource => wanted == OptiScalerAction::Relocate,
            };
            if !action_allowed {
                return Err(crate::failed(
                    "Reused mutation authority does not permit this action",
                ));
            }
            let expected = DiskObservation::File {
                identity: current.identity().to_owned(),
                digest: current.digest().clone(),
            };
            if !virtual_absent && !repeated && actual != expected {
                return Err(token_drift(&participant.path, &expected, &actual));
            }
            Ok(expected)
        }
    }
}

pub(super) fn preimage_for(
    participant: &PlannedParticipant,
    before: &DiskObservation,
    prior: Option<(u32, DomainEndpoint)>,
) -> DomainPreimage {
    if let Some((operation_id, endpoint)) = prior {
        return DomainPreimage::PriorPostimage {
            operation_id,
            endpoint,
        };
    }
    match &participant.preimage {
        PlannedPreimage::Exact {
            current,
            prior_owned,
        } => DomainPreimage::Initial {
            observation: durable(before),
            receipt: Some(current.clone()),
            // A historical owned receipt is an ownership witness only when
            // it is still the exact durable basis.  The current receipt may
            // legitimately preserve an edited file with the same identity;
            // persisting its stale digest as an Initial basis would make the
            // exact storage contract reject an otherwise exact preimage.
            owned_basis: prior_owned
                .as_ref()
                .filter(|basis| {
                    basis.identity() == current.identity() && basis.digest() == current.digest()
                })
                .cloned(),
        },
        PlannedPreimage::ExactReused { current, .. } => DomainPreimage::Initial {
            observation: durable(before),
            receipt: Some(current.clone()),
            owned_basis: None,
        },
        _ => DomainPreimage::Initial {
            observation: durable(before),
            receipt: None,
            owned_basis: None,
        },
    }
}
