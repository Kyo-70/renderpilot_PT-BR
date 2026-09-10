use renderpilot_application::{ComponentRepository, GameRepository};
use renderpilot_domain::{
    ComponentFile, ComponentId, ComponentKind, ComponentRollbackBaseline, GameId, GameIdentity,
    GameInstallation, GameRuntime, Launcher, LibraryComponent, LibraryTechnology, PathRef,
    Platform, Swappability,
};
use renderpilot_storage_sqlite::SqliteStorage;
use sha2::Digest;

use super::catalog_cascade::{LumaCatalogCascadeError, lower_catalog_cascade};
use super::effects::{LumaPeerEffectAccumulator, LumaPeerOperationOrder};
use super::root_authority::LumaPeerRootAuthority;
use crate::catalog::cascade::cascade_for_managed_paths;

fn path_ref(path: &std::path::Path) -> PathRef {
    PathRef::new(path.to_string_lossy().into_owned()).expect("path")
}

fn authority(root: &std::path::Path) -> LumaPeerRootAuthority {
    LumaPeerRootAuthority::resolve(root, &root.join("dxgi.dll")).expect("root authority")
}

fn digest(bytes: &[u8]) -> renderpilot_domain::Sha256Hash {
    renderpilot_domain::Sha256Hash::new(hex::encode(sha2::Sha256::digest(bytes))).expect("digest")
}

fn component_file(path: &std::path::Path, bytes: &[u8]) -> ComponentFile {
    ComponentFile::new(path_ref(path)).with_sha256(digest(bytes))
}

fn game(root: &std::path::Path, id: &str) -> GameInstallation {
    GameInstallation::new(
        GameIdentity::new(
            GameId::new(id).expect("game id"),
            "Catalog cascade test game",
            Launcher::Manual,
        )
        .expect("identity"),
        Platform::Windows,
        GameRuntime::NativeWindows,
        path_ref(root),
    )
}

fn selected_plan(
    storage: &SqliteStorage,
    game: &GameInstallation,
    component: &LibraryComponent,
    baseline: &ComponentRollbackBaseline,
    owned_paths: &[std::path::PathBuf],
) -> crate::catalog::cascade::CascadeResult {
    storage.upsert_game(game).expect("game");
    storage
        .replace_components_for_game(game.id(), std::slice::from_ref(component))
        .expect("component");
    storage
        .recover_component_rollback_baseline(game.id(), component.id(), baseline)
        .expect("baseline");
    cascade_for_managed_paths(storage, game.id(), owned_paths).expect("cascade")
}

#[test]
fn lowers_all_catalog_shapes_from_real_snapshots_and_ignores_current_only_orphan() {
    let root = tempfile::tempdir().expect("root");
    let current_only = root.path().join("current-only.dll");
    let distinct = root.path().join("distinct.dll");
    let baseline_only = root.path().join("baseline-only.dll");
    let equal_present = root.path().join("equal-present.dll");
    let equal_absent = root.path().join("equal-absent.dll");
    std::fs::write(&current_only, b"current-only").expect("current-only");
    std::fs::write(&distinct, b"distinct-active").expect("distinct");
    std::fs::write(&equal_present, b"equal-present").expect("equal-present");
    std::fs::write(&equal_absent, b"equal-absent").expect("equal-absent");
    std::fs::write(
        current_only.with_extension("dll.bak"),
        b"unrelated orphan sidecar",
    )
    .expect("orphan sidecar");
    std::fs::write(distinct.with_extension("dll.bak"), b"distinct-baseline")
        .expect("distinct sidecar");
    std::fs::write(baseline_only.with_extension("dll.bak"), b"baseline-only")
        .expect("baseline-only sidecar");
    std::fs::write(equal_present.with_extension("dll.bak"), b"equal-present")
        .expect("equal-present sidecar");

    let game = game(root.path(), "manual:catalog-peer-shapes");
    let component_id = ComponentId::new("component:catalog-peer-shapes").expect("component id");
    let current_files = [
        component_file(&current_only, b"current-only"),
        component_file(&distinct, b"distinct-active"),
        component_file(&equal_present, b"equal-present"),
        component_file(&equal_absent, b"equal-absent"),
    ];
    let baseline_files = vec![
        component_file(&distinct, b"distinct-baseline"),
        component_file(&baseline_only, b"baseline-only"),
        component_file(&equal_present, b"equal-present"),
        component_file(&equal_absent, b"equal-absent"),
    ];
    let component = current_files.iter().cloned().fold(
        LibraryComponent::new(
            component_id,
            game.id().clone(),
            ComponentKind::NativeLibrary,
            LibraryTechnology::AmdFsr,
            Swappability::Swappable,
        ),
        LibraryComponent::with_file,
    );
    let baseline = ComponentRollbackBaseline::new(baseline_files);
    let owned_paths = [
        current_only,
        distinct,
        baseline_only,
        equal_present,
        equal_absent,
    ];
    let storage = SqliteStorage::in_memory().expect("storage");
    let cascade = selected_plan(&storage, &game, &component, &baseline, &owned_paths);
    assert_eq!(cascade.rollback_specs.len(), 1);

    let mut accumulator = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::Uninstall);
    let authority = authority(root.path());
    lower_catalog_cascade(&cascade.rollback_specs, &authority, &mut accumulator)
        .expect("catalog cascade lowering");
    let effects = accumulator.finalize().expect("finalize").expect("effects");
    let endpoints = effects.program().endpoints();
    let paths = endpoints
        .iter()
        .map(|endpoint| endpoint.path().as_str())
        .collect::<Vec<_>>();
    assert_eq!(endpoints.len(), 6);
    let labels = paths
        .iter()
        .map(|path| {
            [
                "baseline-only.dll",
                "baseline-only.dll.bak",
                "current-only.dll",
                "distinct.dll",
                "distinct.dll.bak",
                "equal-present.dll.bak",
            ]
            .into_iter()
            .find(|suffix| path.ends_with(suffix))
            .expect("known endpoint suffix")
        })
        .collect::<Vec<_>>();
    assert_eq!(
        labels,
        vec![
            "baseline-only.dll",
            "baseline-only.dll.bak",
            "current-only.dll",
            "distinct.dll",
            "distinct.dll.bak",
            "equal-present.dll.bak",
        ]
    );
    assert!(paths.iter().any(|path| path.ends_with("baseline-only.dll")));
    assert!(
        paths
            .iter()
            .any(|path| path.ends_with("baseline-only.dll.bak"))
    );
    assert!(paths.iter().any(|path| path.ends_with("current-only.dll")));
    assert!(paths.iter().any(|path| path.ends_with("distinct.dll")));
    assert!(paths.iter().any(|path| path.ends_with("distinct.dll.bak")));
    assert!(
        paths
            .iter()
            .any(|path| path.ends_with("equal-present.dll.bak"))
    );
    assert!(!paths.iter().any(|path| path.ends_with("equal-absent.dll")));
    assert_eq!(
        effects.payloads().len(),
        endpoints.len(),
        "payloads remain aligned with endpoint ordinals"
    );
    assert!(
        effects
            .payloads()
            .iter()
            .any(|payload| payload.as_deref() == Some(b"distinct-baseline".as_slice()))
    );
}

#[test]
fn rejects_normalized_duplicate_paths_across_selected_plans() {
    let root = tempfile::tempdir().expect("root");
    let path = root.path().join("duplicate.dll");
    std::fs::write(&path, b"duplicate").expect("file");
    let game = game(root.path(), "manual:catalog-peer-duplicate");
    let component_id = ComponentId::new("component:catalog-peer-duplicate").expect("component id");
    let component = LibraryComponent::new(
        component_id,
        game.id().clone(),
        ComponentKind::NativeLibrary,
        LibraryTechnology::AmdFsr,
        Swappability::Swappable,
    )
    .with_file(component_file(&path, b"duplicate"));
    let baseline = ComponentRollbackBaseline::new(Vec::new());
    let first_storage = SqliteStorage::in_memory().expect("storage");
    let second_storage = SqliteStorage::in_memory().expect("storage");
    let first = selected_plan(
        &first_storage,
        &game,
        &component,
        &baseline,
        std::slice::from_ref(&path),
    );
    let second = selected_plan(
        &second_storage,
        &game,
        &component,
        &baseline,
        std::slice::from_ref(&path),
    );
    let mut plans = first.rollback_specs;
    plans.extend(second.rollback_specs);
    let mut accumulator = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::Uninstall);
    let authority = authority(root.path());
    let error = lower_catalog_cascade(&plans, &authority, &mut accumulator)
        .expect_err("duplicate normalized paths must fail closed");
    assert!(matches!(error, LumaCatalogCascadeError::DuplicatePath(_)));
}

#[test]
fn lowers_catalog_paths_under_external_reshade_addon_path() {
    let root = tempfile::tempdir().expect("root");
    let payload = tempfile::tempdir().expect("payload");
    std::fs::write(
        root.path().join("ReShade.ini"),
        format!(
            "[ADDON]\r\nAddonPath={}\r\n",
            payload.path().to_string_lossy()
        ),
    )
    .expect("ini");
    let live = payload.path().join("external.dll");
    let sidecar = live.with_extension("dll.bak");
    std::fs::write(&live, b"active").expect("live");
    std::fs::write(&sidecar, b"baseline").expect("sidecar");

    // The catalog fixture accepts paths under its persisted installation root;
    // use the payload directory there so the lowerer exercises a path that is
    // genuinely external to the sealed authority's game root.
    let game = game(payload.path(), "manual:catalog-peer-external");
    let component_id = ComponentId::new("component:catalog-peer-external").expect("component id");
    let component = LibraryComponent::new(
        component_id,
        game.id().clone(),
        ComponentKind::NativeLibrary,
        LibraryTechnology::AmdFsr,
        Swappability::Swappable,
    )
    .with_file(component_file(&live, b"active"));
    let baseline = ComponentRollbackBaseline::new(vec![component_file(&live, b"baseline")]);
    let storage = SqliteStorage::in_memory().expect("storage");
    let cascade = selected_plan(
        &storage,
        &game,
        &component,
        &baseline,
        std::slice::from_ref(&live),
    );
    let mut accumulator = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::Uninstall);
    let authority = authority(root.path());
    assert!(authority.external_capability_root().is_some());
    lower_catalog_cascade(&cascade.rollback_specs, &authority, &mut accumulator)
        .expect("external catalog cascade");
    let effects = accumulator.finalize().expect("finalize").expect("effects");
    assert!(effects.program().endpoints().iter().all(|endpoint| {
        !crate::paths::is_within(
            std::path::Path::new(endpoint.path().as_str()),
            authority.canonical_game_root(),
        )
    }));
    assert!(
        effects
            .program()
            .endpoints()
            .iter()
            .any(|endpoint| endpoint.path() == &path_ref(&live))
    );
    assert!(
        effects
            .program()
            .endpoints()
            .iter()
            .any(|endpoint| endpoint.path() == &path_ref(&sidecar))
    );
}
