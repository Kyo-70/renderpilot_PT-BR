use super::prelude::*;

pub(in crate::repositories) fn validate_namespace_custody(
    journal: &OptiScalerJournal,
) -> AppResult<()> {
    if journal.control_namespace().identity().is_none() {
        return Err(AppError::storage_failed(
            "OptiScaler journal control namespace is not materially identified",
        ));
    }
    for workspace in journal.private_workspaces() {
        if workspace.identity().is_none() {
            return Err(AppError::storage_failed(
                "OptiScaler private workspace is not materially identified",
            ));
        }
        for endpoint in journal
            .operations()
            .iter()
            .flat_map(|operation| operation.effect().endpoints())
        {
            if renderpilot_domain::normalized_path_relation(workspace.path(), endpoint.path())
                != renderpilot_domain::NormalizedPathRelation::Disjoint
            {
                return Err(AppError::storage_failed(
                    "OptiScaler private workspace overlaps a target path",
                ));
            }
        }
    }
    Ok(())
}

pub(in crate::repositories) fn component_depth(path: &str) -> usize {
    normalized_path_key(path)
        .split('/')
        .filter(|component| !component.is_empty())
        .count()
}
