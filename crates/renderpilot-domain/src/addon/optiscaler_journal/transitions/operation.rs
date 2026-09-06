/// Validates one operation's complete action transition, including immutable
/// endpoint identity, endpoint postimage continuity, and action-specific
/// artifact slots.
pub fn validate_optiscaler_operation_transition(
    current: &OperationRecord,
    next: &OperationRecord,
) -> Result<OptiScalerTransitionDirection, OptiScalerJournalError> {
    if current.operation_id() != next.operation_id()
        || current.parent_dependencies() != next.parent_dependencies()
        || current.workspace_id() != next.workspace_id()
    {
        return Err(OptiScalerJournalError::Invalid(
            "operation transition changed immutable operation identity",
        ));
    }
    let current_endpoints = current.effect().endpoints();
    let next_endpoints = next.effect().endpoints();
    if current_endpoints.len() != next_endpoints.len()
        || current_endpoints
            .iter()
            .zip(next_endpoints.iter())
            .any(|(left, right)| {
                left.endpoint() != right.endpoint()
                    || normalized_path_key(left.path()) != normalized_path_key(right.path())
                    || left.preimage() != right.preimage()
            })
    {
        return Err(OptiScalerJournalError::Invalid(
            "operation transition changed an endpoint identity",
        ));
    }
    let direction = validate_optiscaler_effect_transition(current.effect(), next.effect())?;
    validate_action_slots(current.effect(), current.slots())?;
    validate_action_slots(next.effect(), next.slots())?;
    validate_action_slot_transition(
        current.effect(),
        current.slots(),
        next.effect(),
        next.slots(),
    )?;
    for (left_endpoint, right_endpoint) in current
        .effect()
        .endpoints()
        .into_iter()
        .zip(next.effect().endpoints())
    {
        match (
            left_endpoint.expected_after(),
            right_endpoint.expected_after(),
        ) {
            (left, right) if left == right => {}
            (ExpectedAfter::Pending, ExpectedAfter::Known(observation))
                if terminal_observation(next.effect(), right_endpoint.endpoint())
                    == Some(observation) => {}
            _ => {
                return Err(OptiScalerJournalError::Invalid(
                    "operation transition changed its endpoint postimage illegally",
                ));
            }
        }
    }
    Ok(direction)
}
