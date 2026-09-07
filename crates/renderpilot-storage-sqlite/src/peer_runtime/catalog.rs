use renderpilot_domain::{
    ComponentFile, ComponentId, ComponentRollbackBaseline, D3d12ExecutableBaseline,
    D3d12ExecutableIdentity, LibraryComponent, PeerCatalogRollbackClaim,
};
use rusqlite::Transaction;

use crate::repositories::{
    ComponentBaselineMutation, component_backups_for_game_within_transaction, installed_addons,
    list_components_for_game_within_transaction,
};
use renderpilot_application::{AppError, AppResult};

#[derive(Debug, Clone, Default)]
pub(super) struct OwnedCatalogProjection {
    pub(super) component_set: Option<Vec<LibraryComponent>>,
    pub(super) baseline_mutations: Vec<OwnedBaselineMutation>,
    pub(super) catalog_claim: Option<PeerCatalogRollbackClaim>,
}

#[derive(Debug, Clone)]
pub(super) enum OwnedBaselineMutation {
    Capture {
        component_id: ComponentId,
        baseline: ComponentRollbackBaseline,
    },
    CaptureD3d12Executable {
        component_id: ComponentId,
        baseline: D3d12ExecutableBaseline,
    },
    UpdateD3d12ExecutableState {
        component_id: ComponentId,
        expected_active: D3d12ExecutableIdentity,
    },
    UpdateExpectedActiveFiles {
        component_id: ComponentId,
        files: Vec<ComponentFile>,
    },
    Delete {
        component_id: ComponentId,
    },
}

impl OwnedCatalogProjection {
    pub(super) fn from_parts(
        component_set: Option<&[LibraryComponent]>,
        baseline_mutations: &[ComponentBaselineMutation<'_>],
    ) -> Self {
        Self {
            component_set: component_set.map(<[LibraryComponent]>::to_vec),
            baseline_mutations: baseline_mutations
                .iter()
                .map(OwnedBaselineMutation::from)
                .collect(),
            catalog_claim: None,
        }
    }

    pub(super) fn from_claim(claim: PeerCatalogRollbackClaim) -> Self {
        let baseline_mutations = claim
            .deleted_baselines()
            .iter()
            .map(|entry| OwnedBaselineMutation::Delete {
                component_id: entry.component_id().clone(),
            })
            .collect();
        Self {
            component_set: Some(claim.after_components().to_vec()),
            baseline_mutations,
            catalog_claim: Some(claim),
        }
    }

    pub(super) fn catalog_claim(&self) -> Option<&PeerCatalogRollbackClaim> {
        self.catalog_claim.as_ref()
    }
}

/// Binds an optional catalog rollback claim to the exact SQLite preimage.
///
/// The caller-provided claim is only a proposal.  When present, every part of
/// the projection is checked against the database while the preparation
/// transaction is still open, and the returned projection is built from the
/// storage-derived claim rather than from caller-owned component/baseline
/// values.
pub(super) fn bind_preparation(
    transaction: &Transaction<'_>,
    game_id: &renderpilot_domain::GameId,
    before_peer: Option<&renderpilot_domain::InstalledAddon>,
    supplied_claim: Option<&PeerCatalogRollbackClaim>,
    component_set: Option<&[LibraryComponent]>,
    baseline_mutations: &[ComponentBaselineMutation<'_>],
) -> AppResult<OwnedCatalogProjection> {
    let Some(supplied_claim) = supplied_claim else {
        return Ok(OwnedCatalogProjection::from_parts(
            component_set,
            baseline_mutations,
        ));
    };

    if supplied_claim.game_id() != game_id {
        return Err(AppError::storage_failed(
            "catalog rollback claim belongs to a different game",
        ));
    }
    if component_set != Some(supplied_claim.after_components()) {
        return Err(AppError::storage_failed(
            "catalog rollback component projection differs from its claim",
        ));
    }
    validate_delete_mutations(supplied_claim, baseline_mutations)?;

    let persisted_peer = installed_addons::get_within_transaction(transaction, game_id)?;
    if persisted_peer.as_ref() != before_peer {
        return Err(AppError::storage_failed(
            "peer addon preimage changed before catalog claim binding",
        ));
    }

    let persisted_components = list_components_for_game_within_transaction(transaction, game_id)?;
    let mut persisted_baselines =
        component_backups_for_game_within_transaction(transaction, game_id)?;
    reject_d3d12_baselines(&persisted_baselines)?;

    if persisted_components != supplied_claim.before_components() {
        return Err(AppError::storage_failed(
            "catalog rollback claim does not match the persisted component preimage",
        ));
    }

    let mut selected_baselines = Vec::with_capacity(supplied_claim.deleted_baselines().len());
    for entry in supplied_claim.deleted_baselines() {
        let baseline = persisted_baselines
            .remove(entry.component_id())
            .ok_or_else(|| {
                AppError::storage_failed(format!(
                    "catalog rollback baseline {} is missing",
                    entry.component_id().as_str()
                ))
            })?;
        selected_baselines.push(renderpilot_domain::PeerCatalogDeletedBaseline::new(
            entry.component_id().clone(),
            baseline,
        ));
    }
    let derived = PeerCatalogRollbackClaim::new(persisted_components, selected_baselines).map_err(
        |error| AppError::storage_failed(format!("invalid catalog rollback claim: {error}")),
    )?;
    if &derived != supplied_claim {
        return Err(AppError::storage_failed(
            "catalog rollback claim changed while binding its persisted preimage",
        ));
    }
    Ok(OwnedCatalogProjection::from_claim(derived))
}

/// Rechecks the complete catalog preimage immediately before a prepared peer
/// commit mutates components and baselines.  This closes the gap between the
/// preparation transaction and the final peer/topology CAS transaction.
pub(super) fn validate_claim_preimage_within_transaction(
    transaction: &Transaction<'_>,
    game_id: &renderpilot_domain::GameId,
    claim: &PeerCatalogRollbackClaim,
) -> AppResult<()> {
    if claim.game_id() != game_id {
        return Err(AppError::storage_failed(
            "retained catalog rollback claim belongs to a different game",
        ));
    }
    let persisted_components = list_components_for_game_within_transaction(transaction, game_id)?;
    if persisted_components != claim.before_components() {
        return Err(AppError::storage_failed(
            "catalog component preimage changed before peer commit",
        ));
    }
    let persisted_baselines = component_backups_for_game_within_transaction(transaction, game_id)?;
    reject_d3d12_baselines(&persisted_baselines)?;
    for entry in claim.deleted_baselines() {
        let Some(persisted) = persisted_baselines.get(entry.component_id()) else {
            return Err(AppError::storage_failed(format!(
                "catalog rollback baseline {} disappeared before peer commit",
                entry.component_id().as_str()
            )));
        };
        if persisted != entry.baseline() {
            return Err(AppError::storage_failed(format!(
                "catalog rollback baseline {} changed before peer commit",
                entry.component_id().as_str()
            )));
        }
    }
    Ok(())
}

fn validate_delete_mutations(
    claim: &PeerCatalogRollbackClaim,
    mutations: &[ComponentBaselineMutation<'_>],
) -> AppResult<()> {
    if mutations.len() != claim.deleted_baselines().len() {
        return Err(AppError::storage_failed(
            "catalog rollback mutations must contain exactly one delete per claim baseline",
        ));
    }
    let mut actual = Vec::with_capacity(mutations.len());
    for mutation in mutations {
        let ComponentBaselineMutation::Delete { component_id } = mutation else {
            return Err(AppError::storage_failed(
                "catalog rollback claims cannot carry capture or update mutations",
            ));
        };
        actual.push((*component_id).clone());
    }
    actual.sort();
    if actual.windows(2).any(|window| window[0] == window[1]) {
        return Err(AppError::storage_failed(
            "catalog rollback mutations contain a duplicate baseline delete",
        ));
    }
    let mut expected = claim
        .deleted_baselines()
        .iter()
        .map(|entry| entry.component_id().clone())
        .collect::<Vec<_>>();
    expected.sort();
    if actual != expected {
        return Err(AppError::storage_failed(
            "catalog rollback mutations do not match claimed baseline identities",
        ));
    }
    Ok(())
}

fn reject_d3d12_baselines(
    baselines: &std::collections::HashMap<
        renderpilot_domain::ComponentId,
        renderpilot_domain::ComponentRollbackBaseline,
    >,
) -> AppResult<()> {
    if baselines
        .values()
        .any(|baseline| baseline.d3d12_executable().is_some())
    {
        return Err(AppError::storage_failed(
            "D3D12 executable rollback baselines cannot participate in a peer catalog claim",
        ));
    }
    Ok(())
}

impl From<&ComponentBaselineMutation<'_>> for OwnedBaselineMutation {
    fn from(value: &ComponentBaselineMutation<'_>) -> Self {
        match value {
            ComponentBaselineMutation::Capture {
                component_id,
                baseline,
            } => Self::Capture {
                component_id: (*component_id).clone(),
                baseline: (*baseline).clone(),
            },
            ComponentBaselineMutation::CaptureD3d12Executable {
                component_id,
                baseline,
            } => Self::CaptureD3d12Executable {
                component_id: (*component_id).clone(),
                baseline: (*baseline).clone(),
            },
            ComponentBaselineMutation::UpdateD3d12ExecutableState {
                component_id,
                expected_active,
            } => Self::UpdateD3d12ExecutableState {
                component_id: (*component_id).clone(),
                expected_active: (*expected_active).clone(),
            },
            ComponentBaselineMutation::UpdateExpectedActiveFiles {
                component_id,
                files,
            } => Self::UpdateExpectedActiveFiles {
                component_id: (*component_id).clone(),
                files: (*files).to_vec(),
            },
            ComponentBaselineMutation::Delete { component_id } => Self::Delete {
                component_id: (*component_id).clone(),
            },
        }
    }
}

impl OwnedBaselineMutation {
    pub(super) fn as_borrowed(&self) -> ComponentBaselineMutation<'_> {
        match self {
            Self::Capture {
                component_id,
                baseline,
            } => ComponentBaselineMutation::Capture {
                component_id,
                baseline,
            },
            Self::CaptureD3d12Executable {
                component_id,
                baseline,
            } => ComponentBaselineMutation::CaptureD3d12Executable {
                component_id,
                baseline,
            },
            Self::UpdateD3d12ExecutableState {
                component_id,
                expected_active,
            } => ComponentBaselineMutation::UpdateD3d12ExecutableState {
                component_id,
                expected_active,
            },
            Self::UpdateExpectedActiveFiles {
                component_id,
                files,
            } => ComponentBaselineMutation::UpdateExpectedActiveFiles {
                component_id,
                files,
            },
            Self::Delete { component_id } => ComponentBaselineMutation::Delete { component_id },
        }
    }
}
