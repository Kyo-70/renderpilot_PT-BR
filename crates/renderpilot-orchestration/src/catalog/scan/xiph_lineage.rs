//! Managed Xiph successor identity reconciliation.
//!
//! This module owns the narrow positive adoption proof used when detector
//! naming changes from a vendor-suffixed Xiph closure to canonical names. It
//! reads only the current SQLite component/baseline projection and never
//! performs catalog mutation or changes the durable component identity unless
//! every exact proof condition succeeds.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;

use renderpilot_application::{ComponentRepository, InstalledAddonRepository};
use renderpilot_domain::{
    ComponentFile, ComponentId, ComponentKind, ComponentRollbackBaseline, GameInstallation,
    LibraryComponent, LibraryTechnology, ManagedAddonFile, normalized_path_key, xiph,
};
use renderpilot_storage_sqlite::SqliteStorage;

use crate::ServiceError;

/// Rebinds a newly detected canonical Xiph successor to its managed vendor
/// predecessor only when every immutable and live-state fact agrees exactly.
pub(super) fn reconcile_managed_xiph_successor_ids(
    storage: &SqliteStorage,
    game: &GameInstallation,
    components: Vec<LibraryComponent>,
) -> Result<Vec<LibraryComponent>, ServiceError> {
    let previous_components = storage.list_components_for_game(game.id())?;
    let baselines = storage.component_backups_for_game(game.id())?;
    let installed_addon = storage.get_installed_addon(game.id())?;
    let managed_files = crate::coordinated_files::managed_files_of(installed_addon.as_ref());
    let mut reconciled = components;
    let mut claimed_old_ids = BTreeSet::new();

    for candidate in &mut reconciled {
        let Some(predecessor_id) = reconcile_candidate(
            candidate,
            &previous_components,
            &baselines,
            managed_files,
            game,
        )?
        else {
            continue;
        };
        if !claimed_old_ids.insert(predecessor_id) {
            return Err(lineage_error(
                candidate,
                "more than one detected successor claims the same predecessor identity",
            ));
        }
        let replacement = candidate.rebuild_with_id(predecessor_id.clone());
        *candidate = replacement;
    }

    validate_unique_ids(&reconciled)?;
    Ok(reconciled)
}

fn reconcile_candidate<'a>(
    candidate: &LibraryComponent,
    previous_components: &'a [LibraryComponent],
    baselines: &HashMap<ComponentId, ComponentRollbackBaseline>,
    managed_files: &[ManagedAddonFile],
    game: &GameInstallation,
) -> Result<Option<&'a ComponentId>, ServiceError> {
    if !is_lineage_candidate(candidate, game)
        || previous_components
            .iter()
            .any(|previous| previous.id() == candidate.id())
    {
        return Ok(None);
    }
    let potentials = potential_predecessors(candidate, previous_components, game);
    let previous = match potentials.as_slice() {
        [] => return Ok(None),
        [previous] => *previous,
        _ => {
            return Err(lineage_error(
                candidate,
                "multiple potential predecessors make Xiph lineage ambiguous",
            ));
        }
    };
    let proven = prove_lineage(candidate, previous, baselines, managed_files, game)?;
    Ok(Some(proven.id()))
}

fn is_lineage_candidate(candidate: &LibraryComponent, game: &GameInstallation) -> bool {
    candidate.game_id() == game.id()
        && candidate.kind() == ComponentKind::NativeLibrary
        && candidate.technology() == LibraryTechnology::XiphVorbis
}

fn potential_predecessors<'a>(
    candidate: &LibraryComponent,
    previous_components: &'a [LibraryComponent],
    game: &GameInstallation,
) -> Vec<&'a LibraryComponent> {
    if valid_xiph_layout(candidate).is_none() {
        return Vec::new();
    }
    let Ok(candidate_slots) = xiph_parent_slots(candidate) else {
        return Vec::new();
    };
    previous_components
        .iter()
        .filter(|previous| {
            previous.game_id() == game.id()
                && previous.kind() == ComponentKind::NativeLibrary
                && previous.technology() == LibraryTechnology::XiphVorbis
                && valid_xiph_layout(previous).is_some()
                && xiph_parent_slots(previous)
                    .ok()
                    .is_some_and(|previous_slots| previous_slots == candidate_slots)
        })
        .collect()
}

fn validate_unique_ids(components: &[LibraryComponent]) -> Result<(), ServiceError> {
    let mut final_ids = BTreeSet::new();
    for component in components {
        if !final_ids.insert(component.id()) {
            return Err(lineage_error(
                component,
                "reconciliation produced a duplicate component identity",
            ));
        }
    }
    Ok(())
}

fn prove_lineage<'a>(
    candidate: &LibraryComponent,
    previous: &'a LibraryComponent,
    baselines: &HashMap<ComponentId, ComponentRollbackBaseline>,
    managed_files: &[ManagedAddonFile],
    game: &GameInstallation,
) -> Result<&'a LibraryComponent, ServiceError> {
    let baseline = baselines.get(previous.id()).ok_or_else(|| {
        lineage_error(
            candidate,
            "a potential predecessor has no rollback baseline",
        )
    })?;
    if baseline.expected_active_files().is_empty() {
        return Err(lineage_error(
            candidate,
            "the potential predecessor has an empty expected-active projection",
        ));
    }
    require_vendor_baseline(baseline, candidate)?;
    exact_file_set(
        "candidate and expected-active projection",
        candidate.files(),
        baseline.expected_active_files(),
        candidate,
    )?;
    exact_file_set(
        "predecessor and expected-active projection",
        previous.files(),
        baseline.expected_active_files(),
        candidate,
    )?;
    crate::coordinated_files::validate_recorded_xiph_baseline(
        candidate.technology(),
        candidate.files(),
        baseline.files(),
        managed_files,
    )
    .map_err(|error| {
        lineage_error(
            candidate,
            &format!("recorded rollback baseline is unsafe: {error}"),
        )
    })?;
    crate::catalog::xiph_baseline_reservations::verify_vendor_xiph_baseline_reservations(baseline)
        .map_err(|error| {
            lineage_error(
                candidate,
                &format!("vendor rollback reservations are unsafe: {error}"),
            )
        })?;
    crate::coordinated_files::resolve_component_baseline(
        Path::new(game.install_path().as_str()),
        candidate.technology(),
        candidate.files(),
        Some(baseline.files()),
        managed_files,
    )
    .map_err(|error| {
        lineage_error(
            candidate,
            &format!("rollback baseline cannot be resolved: {error}"),
        )
    })?;
    Ok(previous)
}

fn lineage_error(candidate: &LibraryComponent, reason: &str) -> ServiceError {
    ServiceError::command_failed(format!(
        "cannot reconcile Xiph component {}: {reason}",
        candidate.id()
    ))
}

fn valid_xiph_layout(component: &LibraryComponent) -> Option<xiph::XiphLayout> {
    valid_xiph_layout_files(component.files())
}

fn valid_xiph_layout_files(files: &[ComponentFile]) -> Option<xiph::XiphLayout> {
    let names = files
        .iter()
        .map(|file| {
            file.install_as()
                .or_else(|| file.path().file_name())
                .map(|name| (name, file))
        })
        .collect::<Option<Vec<_>>>()?;
    xiph::detect_layout_with_file_names(names)
}

fn require_vendor_baseline(
    baseline: &ComponentRollbackBaseline,
    candidate: &LibraryComponent,
) -> Result<(), ServiceError> {
    if valid_xiph_layout_files(baseline.files()).is_none() {
        return Err(lineage_error(
            candidate,
            "potential predecessor baseline is not a valid Xiph layout",
        ));
    }
    let mut has_vendor_member = false;
    for file in baseline.files() {
        let name = file
            .install_as()
            .or_else(|| file.path().file_name())
            .ok_or_else(|| {
                lineage_error(candidate, "Xiph baseline file has no runtime basename")
            })?;
        let runtime = xiph::parse_runtime_file_name(name)
            .map_err(|error| {
                lineage_error(candidate, &format!("malformed Xiph baseline name: {error}"))
            })?
            .ok_or_else(|| {
                lineage_error(
                    candidate,
                    "Xiph baseline contains an unrelated runtime name",
                )
            })?;
        has_vendor_member |= runtime.is_vendor();
    }
    if !has_vendor_member {
        return Err(lineage_error(
            candidate,
            "potential predecessor baseline contains no vendor-named Xiph member",
        ));
    }
    Ok(())
}

fn xiph_parent_slots(
    component: &LibraryComponent,
) -> Result<BTreeMap<xiph::XiphMember, String>, ServiceError> {
    let mut slots = BTreeMap::new();
    for file in component.files() {
        let name = file
            .install_as()
            .or_else(|| file.path().file_name())
            .ok_or_else(|| lineage_error(component, "Xiph file has no runtime basename"))?;
        let member = xiph::parse_runtime_file_name(name)
            .ok()
            .flatten()
            .ok_or_else(|| {
                lineage_error(component, "Xiph file has an unsupported runtime basename")
            })?
            .member();
        let parent = file
            .path()
            .parent()
            .ok_or_else(|| lineage_error(component, "Xiph file has no parent directory"))?;
        if slots.insert(member, normalized_path_key(parent)).is_some() {
            return Err(lineage_error(
                component,
                "Xiph component contains duplicate semantic members",
            ));
        }
    }
    Ok(slots)
}

fn exact_file_set(
    label: &str,
    left: &[ComponentFile],
    right: &[ComponentFile],
    candidate: &LibraryComponent,
) -> Result<(), ServiceError> {
    let left_by_path = files_by_path(label, left, candidate)?;
    let right_by_path = files_by_path("expected-active projection", right, candidate)?;
    if left_by_path.len() != right_by_path.len() {
        return Err(lineage_error(
            candidate,
            &format!("{label} has different file counts"),
        ));
    }
    for (path, left_file) in left_by_path {
        let Some(right_file) = right_by_path.get(&path) else {
            return Err(lineage_error(
                candidate,
                &format!("{label} has different paths"),
            ));
        };
        let (Some(left_hash), Some(right_hash)) = (left_file.sha256(), right_file.sha256()) else {
            return Err(lineage_error(
                candidate,
                &format!("{label} is missing a SHA-256 hash"),
            ));
        };
        if left_hash != right_hash
            || left_file.version() != right_file.version()
            || left_file.install_as() != right_file.install_as()
            || left_file.pe_compatibility() != right_file.pe_compatibility()
        {
            return Err(lineage_error(
                candidate,
                &format!("{label} has mismatched file metadata"),
            ));
        }
    }
    Ok(())
}

fn files_by_path<'a>(
    label: &str,
    files: &'a [ComponentFile],
    candidate: &LibraryComponent,
) -> Result<BTreeMap<String, &'a ComponentFile>, ServiceError> {
    let mut by_path = BTreeMap::new();
    for file in files {
        if by_path
            .insert(normalized_path_key(file.path().as_str()), file)
            .is_some()
        {
            return Err(lineage_error(
                candidate,
                &format!("{label} contains duplicate paths"),
            ));
        }
    }
    Ok(by_path)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use renderpilot_application::{ComponentRepository, GameRepository};
    use renderpilot_domain::{
        Architecture, ComponentFile, ComponentId, ComponentKind, ComponentRollbackBaseline, GameId,
        GameIdentity, GameRuntime, Launcher, PathRef, PeCompatibilityProfile, PeExportSet,
        PeImportProfile, PeImportSet, Platform, Swappability,
    };

    use super::*;
    use crate::catalog::scan::xiph_test_support::{complete_split_component, vendor_xiph_files};

    #[test]
    fn managed_same_directory_mixed_alias_successor_reuses_stable_id() {
        let root = tempfile::tempdir().expect("root");
        let game = GameInstallation::new(
            GameIdentity::new(
                GameId::new("manual:xiph-same-directory-reconcile").expect("game id"),
                "Same-directory Xiph",
                Launcher::Manual,
            )
            .expect("identity"),
            Platform::Windows,
            GameRuntime::NativeWindows,
            PathRef::new(root.path().to_string_lossy().into_owned()).expect("root"),
        );
        let old_id = ComponentId::new("component:xiph-same-directory-old").expect("old id");
        let new_id = ComponentId::new("component:xiph-same-directory-new").expect("new id");
        let old_names = [
            "vorbisfile_vs2010_x64_rwdi.dll",
            "vorbis_vs2010_x64_rwdi.dll",
            "ogg_vs2010_x64_rwdi.dll",
        ];
        let active_names = ["vorbisfile_vs2010_x64_rwdi.dll", "vorbis.dll", "ogg.dll"];
        let imports = [
            vec![active_names[1], active_names[2]],
            vec![active_names[2]],
            vec![],
        ];
        let old_imports = [vec![old_names[1], old_names[2]], vec![old_names[2]], vec![]];
        let old_bytes = [
            b"old-vorbisfile".as_slice(),
            b"old-vorbis".as_slice(),
            b"old-ogg".as_slice(),
        ];
        let active_files = active_names
            .iter()
            .zip(imports.iter())
            .enumerate()
            .map(|(index, (name, imports))| {
                xiph_file_with_bytes(
                    root.path().join(name),
                    imports,
                    format!("active-{index}").as_bytes(),
                )
            })
            .collect::<Vec<_>>();
        let old_files = old_names
            .iter()
            .zip(old_imports.iter())
            .zip(old_bytes)
            .map(|((name, imports), bytes)| {
                let path = root.path().join(name);
                fs::create_dir_all(path.parent().expect("parent")).expect("parent");
                fs::write(crate::fs::backup_path(&path).expect("sidecar"), bytes).expect("sidecar");
                xiph_file_with_bytes(path, imports, bytes)
            })
            .collect::<Vec<_>>();
        let previous = active_files.iter().cloned().fold(
            LibraryComponent::new(
                old_id.clone(),
                game.id().clone(),
                ComponentKind::NativeLibrary,
                LibraryTechnology::XiphVorbis,
                Swappability::BundleOnly,
            ),
            LibraryComponent::with_file,
        );
        let candidate = active_files.iter().cloned().fold(
            LibraryComponent::new(
                new_id,
                game.id().clone(),
                ComponentKind::NativeLibrary,
                LibraryTechnology::XiphVorbis,
                Swappability::BundleOnly,
            ),
            LibraryComponent::with_file,
        );
        let storage = SqliteStorage::in_memory().expect("storage");
        storage.upsert_game(&game).expect("game");
        storage
            .replace_components_for_game(game.id(), std::slice::from_ref(&previous))
            .expect("previous component");
        storage
            .recover_component_rollback_baseline(
                game.id(),
                &old_id,
                &ComponentRollbackBaseline::new(old_files).with_expected_active_files(active_files),
            )
            .expect("baseline");

        let reconciled =
            reconcile_managed_xiph_successor_ids(&storage, &game, vec![candidate.clone()])
                .expect("same-directory managed successor proof");
        assert_eq!(reconciled.len(), 1);
        assert_eq!(reconciled[0].id(), previous.id());
        assert_eq!(reconciled[0].files(), candidate.files());

        let second = candidate.rebuild_with_id(
            ComponentId::new("component:xiph-same-directory-second").expect("second id"),
        );
        assert!(
            reconcile_managed_xiph_successor_ids(&storage, &game, vec![candidate, second]).is_err(),
            "two detected successors must not claim one predecessor identity"
        );
    }

    #[test]
    fn canonical_predecessor_baseline_is_not_adopted_as_vendor_lineage() {
        let root = tempfile::tempdir().expect("root");
        let game = GameInstallation::new(
            GameIdentity::new(
                GameId::new("manual:xiph-canonical-predecessor").expect("game id"),
                "Canonical predecessor",
                Launcher::Manual,
            )
            .expect("identity"),
            Platform::Windows,
            GameRuntime::NativeWindows,
            PathRef::new(root.path().to_string_lossy().into_owned()).expect("root"),
        );
        let previous = complete_split_component(&game).rebuild_with_id(
            ComponentId::new("component:xiph-canonical-predecessor-old").expect("old id"),
        );
        let candidate = previous.rebuild_with_id(
            ComponentId::new("component:xiph-canonical-predecessor-new").expect("new id"),
        );
        let storage = SqliteStorage::in_memory().expect("storage");
        storage.upsert_game(&game).expect("game");
        storage
            .replace_components_for_game(game.id(), std::slice::from_ref(&previous))
            .expect("previous component");
        storage
            .recover_component_rollback_baseline(
                game.id(),
                previous.id(),
                &ComponentRollbackBaseline::new(previous.files().to_vec())
                    .with_expected_active_files(previous.files().to_vec()),
            )
            .expect("canonical baseline");

        assert!(
            reconcile_managed_xiph_successor_ids(&storage, &game, vec![candidate]).is_err(),
            "a canonical/non-vendor predecessor is not a vendor-to-canonical lineage"
        );
    }

    #[test]
    fn potential_successor_with_empty_expected_active_projection_is_rejected() {
        let root = tempfile::tempdir().expect("root");
        let game = GameInstallation::new(
            GameIdentity::new(
                GameId::new("manual:xiph-empty-expected").expect("game id"),
                "Empty expected Xiph",
                Launcher::Manual,
            )
            .expect("identity"),
            Platform::Windows,
            GameRuntime::NativeWindows,
            PathRef::new(root.path().to_string_lossy().into_owned()).expect("root"),
        );
        let old_id = ComponentId::new("component:xiph-empty-expected-old").expect("old id");
        let old = complete_split_component(&game).rebuild_with_id(old_id.clone());
        let candidate = old.rebuild_with_id(
            ComponentId::new("component:xiph-empty-expected-new").expect("new id"),
        );
        let storage = SqliteStorage::in_memory().expect("storage");
        storage.upsert_game(&game).expect("game");
        storage
            .replace_components_for_game(game.id(), std::slice::from_ref(&old))
            .expect("component");
        storage
            .recover_component_rollback_baseline(
                game.id(),
                &old_id,
                &ComponentRollbackBaseline::new(vendor_xiph_files(game.install_path().as_str())),
            )
            .expect("baseline");

        assert!(
            reconcile_managed_xiph_successor_ids(&storage, &game, vec![candidate]).is_err(),
            "empty expected-active state is not adoption proof"
        );
    }

    #[test]
    fn potential_successor_without_previous_baseline_is_rejected() {
        let root = tempfile::tempdir().expect("root");
        let game = GameInstallation::new(
            GameIdentity::new(
                GameId::new("manual:xiph-missing-baseline").expect("game id"),
                "Missing baseline Xiph",
                Launcher::Manual,
            )
            .expect("identity"),
            Platform::Windows,
            GameRuntime::NativeWindows,
            PathRef::new(root.path().to_string_lossy().into_owned()).expect("root"),
        );
        let old = complete_split_component(&game).rebuild_with_id(
            ComponentId::new("component:xiph-missing-baseline-old").expect("old id"),
        );
        let candidate = old.rebuild_with_id(
            ComponentId::new("component:xiph-missing-baseline-new").expect("new id"),
        );
        let storage = SqliteStorage::in_memory().expect("storage");
        storage.upsert_game(&game).expect("game");
        storage
            .replace_components_for_game(game.id(), std::slice::from_ref(&old))
            .expect("component");

        assert!(
            reconcile_managed_xiph_successor_ids(&storage, &game, vec![candidate]).is_err(),
            "a potential predecessor without a baseline must not fall through"
        );
    }

    #[test]
    fn potential_successor_with_missing_expected_hash_is_rejected() {
        let root = tempfile::tempdir().expect("root");
        let game = GameInstallation::new(
            GameIdentity::new(
                GameId::new("manual:xiph-missing-hash").expect("game id"),
                "Missing hash Xiph",
                Launcher::Manual,
            )
            .expect("identity"),
            Platform::Windows,
            GameRuntime::NativeWindows,
            PathRef::new(root.path().to_string_lossy().into_owned()).expect("root"),
        );
        let old_id = ComponentId::new("component:xiph-missing-hash-old").expect("old id");
        let old = complete_split_component(&game).rebuild_with_id(old_id.clone());
        let candidate = old
            .rebuild_with_id(ComponentId::new("component:xiph-missing-hash-new").expect("new id"));
        let first = &candidate.files()[0];
        let missing_hash = ComponentFile::new(first.path().clone())
            .with_pe_compatibility(first.pe_compatibility().expect("synthetic profile").clone());
        let mut expected_active = candidate.files().to_vec();
        expected_active[0] = missing_hash;
        let storage = SqliteStorage::in_memory().expect("storage");
        storage.upsert_game(&game).expect("game");
        storage
            .replace_components_for_game(game.id(), std::slice::from_ref(&old))
            .expect("component");
        storage
            .recover_component_rollback_baseline(
                game.id(),
                &old_id,
                &ComponentRollbackBaseline::new(vendor_xiph_files(game.install_path().as_str()))
                    .with_expected_active_files(expected_active),
            )
            .expect("baseline");

        assert!(
            reconcile_managed_xiph_successor_ids(&storage, &game, vec![candidate]).is_err(),
            "missing SHA-256 in expected-active state must block adoption"
        );
    }

    fn xiph_file_with_bytes(
        path: impl AsRef<Path>,
        imports: &[&str],
        bytes: &[u8],
    ) -> ComponentFile {
        ComponentFile::new(
            PathRef::new(path.as_ref().to_string_lossy().into_owned()).expect("path"),
        )
        .with_sha256(renderpilot_detection::sha256_bytes(bytes).expect("hash"))
        .with_pe_compatibility(
            PeCompatibilityProfile::new(
                Architecture::X64,
                PeExportSet::from_observed_names(vec!["xiph_export".to_owned()]).expect("exports"),
            )
            .with_imports(PeImportProfile {
                regular: PeImportSet::from_observed_names(
                    imports.iter().map(|name| (*name).to_owned()).collect(),
                )
                .expect("imports"),
                delay: PeImportSet::default(),
            }),
        )
    }
}
