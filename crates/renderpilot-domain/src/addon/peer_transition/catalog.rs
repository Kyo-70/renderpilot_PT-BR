//! Exact catalog rollback projections used by peer physical planning.

use std::collections::{BTreeMap, BTreeSet};

use crate::{
    ComponentId, ComponentRollbackBaseline, GameId, LibraryComponent, NormalizedPathRelation,
    PathRef, normalized_path_key, normalized_path_relation,
};

use super::model::PeerTransitionError;

/// One component baseline selected for an exact catalog rollback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerCatalogDeletedBaseline {
    component_id: ComponentId,
    baseline: ComponentRollbackBaseline,
}

impl PeerCatalogDeletedBaseline {
    /// Binds a rollback baseline to the exact persisted component identity.
    #[must_use]
    pub fn new(component_id: ComponentId, baseline: ComponentRollbackBaseline) -> Self {
        Self {
            component_id,
            baseline,
        }
    }

    /// Alias emphasizing that the entry is reconstructed from persisted parts.
    #[must_use]
    pub fn from_parts(component_id: ComponentId, baseline: ComponentRollbackBaseline) -> Self {
        Self::new(component_id, baseline)
    }

    /// Returns the component identity being rolled back.
    #[must_use]
    pub fn component_id(&self) -> &ComponentId {
        &self.component_id
    }

    /// Returns the immutable rollback baseline.
    #[must_use]
    pub fn baseline(&self) -> &ComponentRollbackBaseline {
        &self.baseline
    }
}

/// Closed before/after catalog projection for one rollback operation.
///
/// The after projection is derived during construction and cannot be supplied
/// independently by a caller. This prevents a later layer from pairing a
/// rollback card with a different catalog aggregate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerCatalogRollbackClaim {
    game_id: GameId,
    before_components: Vec<LibraryComponent>,
    after_components: Vec<LibraryComponent>,
    deleted_baselines: Vec<PeerCatalogDeletedBaseline>,
}

impl PeerCatalogRollbackClaim {
    /// Builds an exact persisted-after aggregate from the persisted-before
    /// components and unique component rollback baselines.
    pub fn new(
        before_components: Vec<LibraryComponent>,
        deleted_baselines: Vec<PeerCatalogDeletedBaseline>,
    ) -> Result<Self, PeerTransitionError> {
        let game_id = validate_before(&before_components)?;
        let mut canonical_baselines = deleted_baselines;
        canonical_baselines.sort_by(|left, right| left.component_id.cmp(&right.component_id));
        let baselines = index_baselines(&canonical_baselines)?;
        if baselines.is_empty() {
            return Err(PeerTransitionError::CatalogClaimInvalid(
                "catalog rollback claim has no deleted baselines",
            ));
        }
        let mut affected = BTreeSet::new();
        let mut after_components = Vec::with_capacity(before_components.len());
        for component in &before_components {
            let Some(selected) = baselines.get(component.id()) else {
                after_components.push(component.clone());
                continue;
            };
            affected.insert(component.id());
            validate_baseline(component, selected)?;
            if selected.baseline.files().is_empty() {
                continue;
            }
            let mut files = selected.baseline.files().to_vec();
            crate::fsr::sort_representative_first(&mut files);
            after_components.push(component.rebuild_with_files(files));
        }
        if affected.len() != baselines.len() {
            return Err(PeerTransitionError::CatalogClaimInvalid(
                "rollback baseline has no exact persisted component",
            ));
        }
        validate_affected_paths(&before_components, &canonical_baselines, &affected)?;
        Ok(Self {
            game_id,
            before_components,
            after_components,
            deleted_baselines: canonical_baselines,
        })
    }

    /// Reconstructs the closed claim from persisted rollback parts.
    pub fn from_parts(
        before_components: Vec<LibraryComponent>,
        deleted_baselines: Vec<PeerCatalogDeletedBaseline>,
    ) -> Result<Self, PeerTransitionError> {
        Self::new(before_components, deleted_baselines)
    }

    /// Returns the one game shared by every component in the claim.
    #[must_use]
    pub fn game_id(&self) -> &GameId {
        &self.game_id
    }

    /// Returns the exact persisted-before component order.
    #[must_use]
    pub fn before_components(&self) -> &[LibraryComponent] {
        &self.before_components
    }

    /// Returns the internally derived persisted-after component order.
    #[must_use]
    pub fn after_components(&self) -> &[LibraryComponent] {
        &self.after_components
    }

    /// Returns the selected component baselines in canonical ComponentId order.
    #[must_use]
    pub fn deleted_baselines(&self) -> &[PeerCatalogDeletedBaseline] {
        &self.deleted_baselines
    }
}

fn validate_before(components: &[LibraryComponent]) -> Result<GameId, PeerTransitionError> {
    let Some(first) = components.first() else {
        return Err(PeerTransitionError::CatalogClaimInvalid(
            "catalog rollback claim has no persisted components",
        ));
    };
    let game_id = first.game_id().clone();
    let mut ids = BTreeSet::new();
    for component in components {
        if component.game_id() != &game_id {
            return Err(PeerTransitionError::CatalogComponentGameMismatch(
                component.id().clone(),
            ));
        }
        if !ids.insert(component.id()) {
            return Err(PeerTransitionError::CatalogClaimInvalid(
                "persisted components contain a duplicate identity",
            ));
        }
    }
    Ok(game_id)
}

fn index_baselines(
    entries: &[PeerCatalogDeletedBaseline],
) -> Result<BTreeMap<&ComponentId, &PeerCatalogDeletedBaseline>, PeerTransitionError> {
    let mut indexed = BTreeMap::new();
    for entry in entries {
        if indexed.insert(&entry.component_id, entry).is_some() {
            return Err(PeerTransitionError::CatalogClaimInvalid(
                "rollback baselines contain a duplicate identity",
            ));
        }
    }
    Ok(indexed)
}

fn validate_baseline(
    component: &LibraryComponent,
    entry: &PeerCatalogDeletedBaseline,
) -> Result<(), PeerTransitionError> {
    if entry.baseline.d3d12_executable().is_some() {
        return Err(PeerTransitionError::CatalogClaimInvalid(
            "D3D12 executable rollback is outside the peer catalog contract",
        ));
    }
    for file in component.files().iter().chain(entry.baseline.files()) {
        if file.sha256().is_none() {
            return Err(PeerTransitionError::CatalogClaimInvalid(
                "affected catalog file has no SHA-256",
            ));
        }
    }
    Ok(())
}

fn validate_affected_paths(
    components: &[LibraryComponent],
    entries: &[PeerCatalogDeletedBaseline],
    affected: &BTreeSet<&ComponentId>,
) -> Result<(), PeerTransitionError> {
    let mut current = Vec::new();
    for component in components {
        if affected.contains(&component.id()) {
            current.extend(
                component
                    .files()
                    .iter()
                    .map(|file| (component.id(), file.path())),
            );
        }
    }
    validate_path_set(current.iter().map(|(_, path)| *path))?;

    let mut baselines = Vec::new();
    for entry in entries {
        baselines.extend(
            entry
                .baseline
                .files()
                .iter()
                .map(|file| (entry.component_id(), file.path())),
        );
    }
    validate_path_set(baselines.iter().map(|(_, path)| *path))?;

    for (current_id, current_path) in &current {
        for (baseline_id, baseline_path) in &baselines {
            if matches!(
                normalized_path_relation(current_path.as_str(), baseline_path.as_str()),
                NormalizedPathRelation::Equal
            ) {
                if current_id != baseline_id {
                    return Err(PeerTransitionError::CatalogClaimInvalid(
                        "components share an affected catalog path",
                    ));
                }
            } else if normalized_path_relation(current_path.as_str(), baseline_path.as_str())
                .overlaps()
            {
                return Err(PeerTransitionError::CatalogClaimInvalid(
                    "affected catalog paths overlap",
                ));
            }
        }
    }
    for entry in entries {
        for file in entry.baseline.files() {
            let sidecar = super::managed_sidecar_path(file.path())?;
            for (_, affected_path) in current.iter().chain(baselines.iter()) {
                if normalized_path_relation(sidecar.as_str(), affected_path.as_str()).overlaps() {
                    return Err(PeerTransitionError::CatalogClaimInvalid(
                        "catalog baseline sidecar collides with an affected path",
                    ));
                }
            }
        }
    }
    Ok(())
}

fn validate_path_set<'a>(
    paths: impl Iterator<Item = &'a PathRef>,
) -> Result<(), PeerTransitionError> {
    let mut ordered = paths.collect::<Vec<_>>();
    ordered.sort_by_key(|path| normalized_path_key(path.as_str()));
    for (index, left) in ordered.iter().enumerate() {
        for right in ordered.iter().skip(index + 1) {
            if normalized_path_relation(left.as_str(), right.as_str()).overlaps() {
                return Err(PeerTransitionError::CatalogClaimInvalid(
                    "affected catalog paths are duplicate or overlapping",
                ));
            }
        }
    }
    Ok(())
}
