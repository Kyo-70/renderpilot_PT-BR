use super::prelude::*;
use super::*;

pub(in crate::repositories) fn validate_preimage_bindings(
    journal: &OptiScalerJournal,
) -> AppResult<()> {
    let mut last_touch: BTreeMap<String, (u32, Endpoint)> = BTreeMap::new();
    for (consumer_index, operation) in journal.operations().iter().enumerate() {
        for endpoint in operation.effect().endpoints() {
            let path_key = normalized_path_key(endpoint.path());
            match endpoint.preimage() {
                Preimage::Initial {
                    observation,
                    receipt,
                    owned_basis,
                } => {
                    if !observation.is_exact() {
                        return Err(AppError::storage_failed(
                            "OptiScaler initial preimage is not exact",
                        ));
                    }
                    if let Some(receipt) = receipt
                        && !matches!(
                            observation,
                            DurableObservation::File { identity, digest }
                                if identity == receipt.identity() && digest == receipt.digest()
                        )
                    {
                        return Err(AppError::storage_failed(
                            "OptiScaler initial preimage receipt does not match its observation",
                        ));
                    }
                    if let Some(owned_basis) = owned_basis
                        && let Some(receipt) = receipt
                        && (owned_basis.identity() != receipt.identity()
                            || owned_basis.digest() != receipt.digest())
                    {
                        return Err(AppError::storage_failed(
                            "OptiScaler initial owned basis changed its file identity or digest",
                        ));
                    }
                    if last_touch.contains_key(&path_key) {
                        return Err(AppError::storage_failed(
                            "OptiScaler repeated path must use its prior postimage",
                        ));
                    }
                }
                Preimage::PriorPostimage {
                    operation_id,
                    endpoint: prior_endpoint,
                } => {
                    let prior_index = usize::try_from(*operation_id).map_err(|_| {
                        AppError::storage_failed(
                            "OptiScaler prior postimage ordinal overflows usize",
                        )
                    })?;
                    if prior_index >= consumer_index {
                        return Err(AppError::storage_failed(format!(
                            "OptiScaler prior postimage ordinal is not earlier than operation {}",
                            operation.operation_id()
                        )));
                    }
                    let prior_operation =
                        journal.operations().get(prior_index).ok_or_else(|| {
                            AppError::storage_failed(
                                "OptiScaler prior postimage operation is missing",
                            )
                        })?;
                    if last_touch.get(&path_key) != Some(&(*operation_id, *prior_endpoint)) {
                        return Err(AppError::storage_failed(
                            "OptiScaler prior postimage does not name the last producer",
                        ));
                    }
                    let Some(prior) = prior_operation
                        .effect()
                        .endpoints()
                        .into_iter()
                        .find(|candidate| candidate.endpoint() == *prior_endpoint)
                    else {
                        return Err(AppError::storage_failed(
                            "OptiScaler prior postimage endpoint is missing",
                        ));
                    };
                    if normalized_path_key(prior.path()) != path_key {
                        return Err(AppError::storage_failed(
                            "OptiScaler prior postimage path changed its durable producer",
                        ));
                    }
                    let produced = applied_observation(prior_operation.effect(), *prior_endpoint);
                    match produced {
                        Some(produced) => {
                            if resolve_preimage(journal, endpoint) != Some(produced) {
                                return Err(AppError::storage_failed(
                                    "OptiScaler prior postimage changed its immutable result chain",
                                ));
                            }
                        }
                        None if !(operation.effect().is_planned()
                            || operation.effect().is_preserved())
                            || !matches!(endpoint.expected_after(), ExpectedAfter::Pending) =>
                        {
                            return Err(AppError::storage_failed(
                                "OptiScaler consumer advanced before its prior producer was Applied",
                            ));
                        }
                        None => {}
                    }
                }
            }
            last_touch.insert(path_key, (operation.operation_id(), endpoint.endpoint()));
        }
    }
    Ok(())
}

pub(in crate::repositories) fn resolve_preimage<'a>(
    journal: &'a OptiScalerJournal,
    endpoint: &'a OperationEndpoint,
) -> Option<&'a DurableObservation> {
    match endpoint.preimage() {
        Preimage::Initial { observation, .. } => Some(observation),
        Preimage::PriorPostimage {
            operation_id,
            endpoint,
        } => journal
            .operations()
            .get(usize::try_from(*operation_id).ok()?)
            .and_then(|operation| applied_observation(operation.effect(), *endpoint)),
    }
}

pub(in crate::repositories) fn initial_preimage(
    endpoint: &OperationEndpoint,
) -> Option<&DurableObservation> {
    match endpoint.preimage() {
        Preimage::Initial { observation, .. } => Some(observation),
        Preimage::PriorPostimage { .. } => None,
    }
}
