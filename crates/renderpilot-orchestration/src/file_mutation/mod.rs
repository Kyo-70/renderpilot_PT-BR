//! Crash-recoverable transactions for game and external add-on file roots.
//!
//! ## Contract
//!
//! 1. Hold [`crate::game_mutation_lock::GameMutationGuard`].
//! 2. [`recover_pending`] runs (also from [`DurableFileTransaction::prepare`]).
//!    Boundary entry already recovers; prepare re-runs it idempotently so
//!    hand-rolled multi-step flows cannot skip recovery.
//! 3. Snapshot every path the feature may touch (over-inclusive is correct).
//! 4. Mutate files, then feature-commit DB with the reserved `mutation_id`.
//! 5. On success clean snapshots; on failure restore exact before-state.
//!
//! Prefer [`run_durable_mutation`] at call sites. Hand-rolled prepare/finish is
//! reserved for multi-step flows that open an engine sentinel first.

mod manifest;
mod peer_recovery;
mod recover;
mod retryable_v2;
mod scope;
mod transaction;

#[cfg(test)]
mod tests;

use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use renderpilot_application::ProxyTopologyRepository;
use renderpilot_domain::{AddonKind, mutation_features::MutationFeatureOwner};

pub(crate) use recover::{recover_pending, recover_pending_matching};
pub(crate) use retryable_v2::{
    RetryableFileMutationV2, RetryableFileOperation, RetryableFilePlan, V2DiskObservation, observe,
};
pub(crate) use scope::MutationScope;
pub(crate) use transaction::{DurableFileTransaction, DurableMutation, run_durable_mutation};

/// Fences framework-owned mutations while a peer proxy topology is active.
/// Recovery remains available at the boundary; only creation of a new
/// mutation is rejected for a conflicting add-on owner.
pub(crate) fn ensure_feature_allowed_with_proxy_topology(
    context: &crate::Context,
    game_id: &renderpilot_domain::GameId,
    feature: &str,
) -> Result<(), crate::ServiceError> {
    if context.storage().get_proxy_topology(game_id)?.is_none() {
        return Ok(());
    }
    let owner = renderpilot_domain::mutation_features::feature_owner(feature).ok_or_else(|| {
        crate::ServiceError::invalid_input(format!(
            "durable mutation feature cannot be classified under an active proxy topology: {feature}"
        ))
    })?;
    let peer_kind = match owner {
        MutationFeatureOwner::Luma => Some(AddonKind::Luma),
        MutationFeatureOwner::RenoDx => Some(AddonKind::RenoDx),
        MutationFeatureOwner::Catalog
        | MutationFeatureOwner::OptiScaler
        | MutationFeatureOwner::SharedVulkan => None,
    };
    if let Some(peer_kind) = peer_kind {
        return Err(crate::ServiceError::peer_topology_conflict(peer_kind));
    }
    Ok(())
}

pub(super) fn remove_dir_if_exists(directory: &Path) -> Result<(), crate::ServiceError> {
    match fs::remove_dir_all(directory) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(crate::failed(format!(
            "failed to remove file transaction directory {}: {error}",
            directory.display()
        ))),
    }
}

pub(super) fn canonical_candidate(path: &Path) -> Result<PathBuf, crate::ServiceError> {
    crate::paths::canonical_candidate(path).map_err(|error| crate::failed(error.to_string()))
}

/// Proves that a durable row owns exactly its app-private `root/<id>`
/// directory. Containment alone is insufficient because a corrupt manifest
/// could otherwise target the root or a sibling transaction's artifacts.
pub(super) fn validate_transaction_directory_owner(
    root: &Path,
    id: &str,
    declared: &Path,
) -> Result<PathBuf, crate::ServiceError> {
    let mut components = Path::new(id).components();
    if !matches!(components.next(), Some(Component::Normal(_))) || components.next().is_some() {
        return Err(crate::failed(
            "pending transaction id is not a single path component",
        ));
    }
    let canonical_root = canonical_candidate(root)?;
    let expected = canonical_root.join(id);
    let declared = canonical_candidate(declared)?;
    if !crate::paths::is_within(&expected, &canonical_root)
        || crate::paths::normalized_key(&declared) != crate::paths::normalized_key(&expected)
    {
        return Err(crate::failed(
            "pending transaction directory does not match its durable row id",
        ));
    }
    Ok(declared)
}
