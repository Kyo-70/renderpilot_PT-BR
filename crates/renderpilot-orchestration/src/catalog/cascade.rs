//! Compound catalog rollback driven by disappearing owned managed paths.
//!
//! Feature-neutral: any tool that records owned [`ManagedAddonFile`] paths can
//! request whole-component restore when those paths leave the game tree.
//! Tool-specific composition (which owned paths disappear) stays in the tool.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use renderpilot_application::{
    AppError, AppResult, ComponentRepository, InstalledAddonRepository, OperationKind,
};
use renderpilot_domain::{
    ComponentFile, ComponentId, ComponentRollbackBaseline, GameId, LibraryComponent,
    PeerCatalogDeletedBaseline, PeerCatalogRollbackClaim, component_version_report,
};
use renderpilot_storage_sqlite::SqliteStorage;

use crate::catalog::execute::{
    JournalEntryItem, JournalEntryParams, ROLLBACK_TARGET_LABEL, record_operation_journal_entry,
    revert_to_baseline_fs,
};

/// Full catalog component rollback selected by an owned managed-file intersection.
#[derive(Debug)]
pub(crate) struct ValidatedRollbackPlan {
    component: LibraryComponent,
    rollback_baseline: ComponentRollbackBaseline,
}

/// Named result of [`cascade_for_managed_paths`].
#[derive(Debug)]
pub(crate) struct CascadeResult {
    pub(crate) rollback_specs: Vec<ValidatedRollbackPlan>,
    catalog_claim: Option<PeerCatalogRollbackClaim>,
    pub(crate) next_components: Vec<LibraryComponent>,
    pub(crate) mutation_paths: Vec<PathBuf>,
}

impl CascadeResult {
    #[cfg(test)]
    pub(crate) fn empty_for_test() -> Self {
        Self {
            rollback_specs: Vec::new(),
            catalog_claim: None,
            next_components: Vec::new(),
            mutation_paths: Vec::new(),
        }
    }

    #[cfg(test)]
    pub(crate) fn from_parts_for_test(
        rollback_specs: Vec<ValidatedRollbackPlan>,
        catalog_claim: Option<PeerCatalogRollbackClaim>,
        next_components: Vec<LibraryComponent>,
    ) -> Self {
        Self {
            mutation_paths: cascade_mutation_paths(&rollback_specs),
            rollback_specs,
            catalog_claim,
            next_components,
        }
    }

    /// Returns the exact durable catalog rollback claim, when this cascade
    /// selected one or more persisted component baselines.
    pub(crate) fn catalog_claim(&self) -> Option<&PeerCatalogRollbackClaim> {
        self.catalog_claim.as_ref()
    }
}

impl ValidatedRollbackPlan {
    #[cfg(test)]
    pub(crate) fn for_test(
        component: LibraryComponent,
        rollback_baseline: ComponentRollbackBaseline,
    ) -> Self {
        Self {
            component,
            rollback_baseline,
        }
    }

    /// Returns the selected active component file projection.
    pub(crate) fn current_files(&self) -> &[ComponentFile] {
        self.component.files()
    }

    /// Returns the selected immutable rollback file projection.
    pub(crate) fn baseline_files(&self) -> &[ComponentFile] {
        self.rollback_baseline.files()
    }

    pub(crate) fn component_id(&self) -> &ComponentId {
        self.component.id()
    }

    pub(crate) fn contains_path(&self, path: &Path) -> bool {
        let path = crate::paths::normalized_key(path);
        self.component
            .files()
            .iter()
            .chain(self.baseline_files())
            .any(|file| crate::paths::normalized_key(Path::new(file.path().as_str())) == path)
    }
}

struct SelectedCascade {
    rollback_specs: Vec<ValidatedRollbackPlan>,
    catalog_claim: Option<PeerCatalogRollbackClaim>,
    next_components: Vec<LibraryComponent>,
}

/// Reads the persisted catalog projection once and derives both the durable
/// claim and the filesystem-facing rollback specs from that same snapshot.
///
/// The rollback specs deliberately retain freshly validated filesystem images;
/// those observations are never used to build the durable claim.
fn select_cascade(
    storage: &SqliteStorage,
    game_id: &GameId,
    owned_paths: &[PathBuf],
) -> AppResult<SelectedCascade> {
    let persisted_components = storage.list_components_for_game(game_id)?;
    if owned_paths.is_empty() {
        return Ok(SelectedCascade {
            rollback_specs: Vec::new(),
            catalog_claim: None,
            next_components: persisted_components,
        });
    }

    let game_root = crate::catalog::game_root_for_mutation(
        storage,
        game_id,
        owned_paths
            .first()
            .and_then(|path| path.parent().map(Path::to_path_buf)),
    )?;
    let managed_files =
        crate::coordinated_files::managed_files_of(storage.get_installed_addon(game_id)?.as_ref())
            .to_vec();
    let owned: HashSet<String> = owned_paths
        .iter()
        .map(|path| crate::paths::normalized_key(path))
        .collect();
    let mut persisted_baselines = storage.component_backups_for_game(game_id)?;
    let mut specs = Vec::new();
    let mut deleted_baselines = Vec::new();
    for persisted_component in &persisted_components {
        let Some(recorded_baseline) = persisted_baselines.remove(persisted_component.id()) else {
            continue;
        };
        let Some(baseline) = crate::coordinated_files::classify_component_backup(
            Some(recorded_baseline.clone()),
            persisted_component.files(),
        )
        .into_available() else {
            continue;
        };
        let intersects = persisted_component
            .files()
            .iter()
            .chain(baseline.files())
            .map(|file| crate::paths::normalized_key(Path::new(file.path().as_str())))
            .any(|path| owned.contains(&path));
        if intersects {
            if baseline.d3d12_executable().is_some() {
                return Err(AppError::invalid_input(format!(
                    "component {} has auxiliary rollback state; fully roll it back before an add-on can consume its managed files",
                    persisted_component.id().as_str()
                )));
            }
            let component = crate::coordinated_files::current_component_snapshot(
                persisted_component,
                &managed_files,
            )
            .map_err(|error| {
                AppError::invalid_input(format!(
                    "cannot validate active component {} for cascade rollback: {error}",
                    persisted_component.id().as_str()
                ))
            })?
            .into_component();
            let resolved_files = crate::coordinated_files::resolve_component_baseline(
                &game_root,
                component.technology(),
                component.files(),
                Some(baseline.files()),
                &managed_files,
            )
            .map_err(|error| {
                AppError::invalid_input(format!(
                    "cannot validate baseline for cascade rollback {}: {error}",
                    component.id().as_str()
                ))
            })?;
            let rollback_baseline = ComponentRollbackBaseline::new(resolved_files);
            deleted_baselines.push(PeerCatalogDeletedBaseline::new(
                persisted_component.id().clone(),
                recorded_baseline,
            ));
            specs.push(ValidatedRollbackPlan {
                component,
                rollback_baseline,
            });
        }
    }

    if deleted_baselines.is_empty() {
        return Ok(SelectedCascade {
            rollback_specs: specs,
            catalog_claim: None,
            next_components: persisted_components,
        });
    }

    let catalog_claim = PeerCatalogRollbackClaim::new(persisted_components, deleted_baselines)
        .map_err(|error| {
            AppError::invalid_input(format!("cannot build catalog cascade claim: {error}"))
        })?;
    let next_components = catalog_claim.after_components().to_vec();
    Ok(SelectedCascade {
        rollback_specs: specs,
        catalog_claim: Some(catalog_claim),
        next_components,
    })
}

pub(crate) fn cascade_mutation_paths(specs: &[ValidatedRollbackPlan]) -> Vec<PathBuf> {
    crate::catalog::execute::mutation_paths_from_component_files(
        specs
            .iter()
            .flat_map(|spec| spec.current_files().iter().chain(spec.baseline_files())),
    )
}

/// Plans cascade rollback for any owned managed paths that are about to leave
/// the game tree: validated specs, the post-cascade component set, and
/// live/sidecar mutation paths.
///
/// Path selection is the caller's responsibility:
/// - full uninstall → all [`crate::addons::records::owned_managed_paths`]
/// - Luma update when payload drops DLSS →
///   [`crate::addons::luma::dlss::cascade_for_disappearing_owned`]
/// - mutation-path snapshotting may intentionally use a wider owned set than
///   the apply-time cascade plan
pub(crate) fn cascade_for_managed_paths(
    storage: &SqliteStorage,
    game_id: &GameId,
    owned_paths: &[PathBuf],
) -> AppResult<CascadeResult> {
    let selected = select_cascade(storage, game_id, owned_paths)?;
    let mutation_paths = cascade_mutation_paths(&selected.rollback_specs);
    Ok(CascadeResult {
        rollback_specs: selected.rollback_specs,
        catalog_claim: selected.catalog_claim,
        next_components: selected.next_components,
        mutation_paths,
    })
}

pub(crate) fn apply_cascade_rollback_fs(specs: &[ValidatedRollbackPlan]) -> AppResult<()> {
    for spec in specs {
        revert_to_baseline_fs(spec.current_files(), spec.baseline_files())?;
    }
    Ok(())
}

pub(crate) fn record_cascade_rollback_journal(
    storage: &SqliteStorage,
    game_id: &GameId,
    specs: &[ValidatedRollbackPlan],
) {
    for spec in specs {
        // Owned so the version string outlives the temporary version report.
        let to_version = cascade_rollback_to_version(spec);
        record_operation_journal_entry(
            storage,
            JournalEntryParams {
                game_id,
                component_id: spec.component.id(),
                kind: OperationKind::RollbackComponent,
                component: &spec.component,
                to_version: Some(to_version.as_str()),
                items: cascade_rollback_journal_items(spec),
                d3d12_executable_action: None,
            },
        );
    }
}

fn cascade_rollback_to_version(spec: &ValidatedRollbackPlan) -> String {
    component_version_report(spec.baseline_files(), spec.component.technology())
        .known_version()
        .map(|version| version.as_str().to_owned())
        .unwrap_or_else(|| ROLLBACK_TARGET_LABEL.to_owned())
}

fn cascade_rollback_journal_items(spec: &ValidatedRollbackPlan) -> Vec<JournalEntryItem<'_>> {
    spec.baseline_files()
        .iter()
        .map(|file| JournalEntryItem::component_file(file.path(), None))
        .collect()
}

#[cfg(test)]
mod tests;
