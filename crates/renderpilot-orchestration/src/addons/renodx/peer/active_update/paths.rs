use std::path::{Component, Path};

use renderpilot_domain::PathRef;

use super::error::RenoDxActiveUpdateError;

pub(super) fn require_under_roots(
    path: &PathRef,
    canonical_game_root: &Path,
    payload_root: Option<&Path>,
) -> Result<(), RenoDxActiveUpdateError> {
    let candidate = Path::new(path.as_str());
    if has_parent_component(candidate)
        || (!is_strictly_under(candidate, canonical_game_root)
            && !payload_root.is_some_and(|root| is_strictly_under(candidate, root)))
    {
        return Err(RenoDxActiveUpdateError::InvalidPath(path.clone()));
    }
    Ok(())
}

fn is_strictly_under(candidate: &Path, root: &Path) -> bool {
    crate::paths::is_within(candidate, root) && !crate::paths::same_path(candidate, root)
}

pub(super) fn same_path(left: &PathRef, right: &PathRef) -> bool {
    crate::paths::same_path(Path::new(left.as_str()), Path::new(right.as_str()))
}

fn has_parent_component(path: &Path) -> bool {
    path.components()
        .any(|component| component == Component::ParentDir)
}
