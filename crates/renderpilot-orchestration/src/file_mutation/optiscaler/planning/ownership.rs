fn planned_input_ownership(
    path: &Path,
    item: &OptiScalerPlannedOperation,
    last_touch: &HashMap<String, (u32, DomainEndpoint)>,
    last_ownership: &HashMap<String, FileOwnership>,
) -> Option<FileOwnership> {
    let key = crate::paths::normalized_key(path);
    if last_touch.contains_key(&key) {
        return last_ownership.get(&key).copied();
    }
    match item {
        OptiScalerPlannedOperation::Write(value)
        | OptiScalerPlannedOperation::Delete(value)
        | OptiScalerPlannedOperation::Verify(value)
        | OptiScalerPlannedOperation::CreateDirectory(value) => match &value.preimage {
            PlannedPreimage::Exact { current, .. }
            | PlannedPreimage::ExactReused { current, .. } => Some(current.ownership()),
            PlannedPreimage::Absent | PlannedPreimage::Verify => None,
        },
        OptiScalerPlannedOperation::Relocate { source, .. } => match &source.preimage {
            PlannedPreimage::Exact { current, .. }
            | PlannedPreimage::ExactReused { current, .. } => Some(current.ownership()),
            PlannedPreimage::Absent | PlannedPreimage::Verify => None,
        },
        OptiScalerPlannedOperation::PostCommitRemoveDirectory(_) => None,
    }
}
