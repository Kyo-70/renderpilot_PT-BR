fn set_prior_postimage(
    effect: &mut DomainOperationEffect,
    path: &Path,
    role: DomainEndpoint,
    operation_id: u32,
    prior_role: DomainEndpoint,
) -> Result<(), ServiceError> {
    let old = endpoint_for(effect, path)
        .filter(|endpoint| endpoint.endpoint() == role)
        .ok_or_else(|| crate::failed("repeated path is not an endpoint of its ordinal"))?;
    let replacement = DomainOperationEndpoint::new(
        role,
        old.path(),
        DomainPreimage::PriorPostimage {
            operation_id,
            endpoint: prior_role,
        },
        old.expected_after().clone(),
    )
    .map_err(|error| crate::failed(error.to_string()))?;
    match effect {
        DomainOperationEffect::Write(value) => *value.endpoint_mut() = replacement,
        DomainOperationEffect::Delete(value) => *value.endpoint_mut() = replacement,
        DomainOperationEffect::Verify(value) => *value.endpoint_mut() = replacement,
        DomainOperationEffect::CreateDirectory(value) => *value.endpoint_mut() = replacement,
        DomainOperationEffect::PostCommitRemoveDirectory(value) => {
            *value.endpoint_mut() = replacement;
        }
        DomainOperationEffect::Relocate(value) => match role {
            DomainEndpoint::Source => *value.source_mut() = replacement,
            DomainEndpoint::Destination => *value.destination_mut() = replacement,
            DomainEndpoint::Single => {
                return Err(crate::failed("relocation cannot use Single endpoint"));
            }
        },
    }
    Ok(())
}

fn validate_private_workspace_paths(
    transaction_id: &str,
    journal: &OptiScalerJournal,
) -> Result<(), ServiceError> {
    namespace::validate_paths(transaction_id, journal)
}
