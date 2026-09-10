use renderpilot_application::{ComponentRepository, GameRepository};
use renderpilot_domain::{
    ComponentFile, ComponentId, ComponentKind, ComponentRollbackBaseline, D3d12ExecutableBaseline,
    D3d12ExecutableIdentity, GameId, GameIdentity, GameInstallation, GameRuntime, Launcher,
    LibraryComponent, LibraryTechnology, PathRef, Platform, Swappability,
};
use renderpilot_storage_sqlite::SqliteStorage;

use super::cascade_for_managed_paths;

#[test]
fn cascade_claim_uses_persisted_projection_and_derives_next_components() {
    let root = tempfile::tempdir().expect("root");
    let selected_path = root.path().join("selected.dll");
    let unaffected_path = root.path().join("unaffected.dll");
    std::fs::write(&selected_path, b"selected").expect("selected");
    std::fs::write(&unaffected_path, b"unaffected").expect("unaffected");

    let game = test_game("manual:cascade-claim", root.path());
    let selected_id = ComponentId::new("component:cascade-selected").expect("id");
    let unaffected_id = ComponentId::new("component:cascade-unaffected").expect("id");
    let selected_file = component_file(&selected_path);
    let unaffected_file = component_file(&unaffected_path);
    let selected = test_component(selected_id, game.id().clone(), selected_file.clone());
    let unaffected = test_component(unaffected_id, game.id().clone(), unaffected_file.clone());

    // This metadata exists only in the persisted baseline. The physical
    // resolver may refresh its apply-time copy, but it must not enter the
    // durable claim.
    let persisted_baseline = ComponentRollbackBaseline::new(vec![selected_file])
        .with_expected_active_files(vec![unaffected_file.clone()]);
    let unrelated_baseline = ComponentRollbackBaseline::new(vec![unaffected_file]);

    let storage = SqliteStorage::in_memory().expect("storage");
    storage.upsert_game(&game).expect("game");
    storage
        .replace_components_for_game(game.id(), &[selected.clone(), unaffected.clone()])
        .expect("components");
    storage
        .recover_component_rollback_baseline(game.id(), selected.id(), &persisted_baseline)
        .expect("selected baseline");
    storage
        .recover_component_rollback_baseline(game.id(), unaffected.id(), &unrelated_baseline)
        .expect("unrelated baseline");

    let result =
        cascade_for_managed_paths(&storage, game.id(), std::slice::from_ref(&selected_path))
            .expect("cascade");
    let claim = result.catalog_claim().expect("selected rollback claim");
    let persisted_components = storage
        .list_components_for_game(game.id())
        .expect("persisted components");

    assert_eq!(claim.before_components(), persisted_components.as_slice());
    assert_eq!(claim.deleted_baselines().len(), 1);
    assert_eq!(claim.deleted_baselines()[0].component_id(), selected.id());
    assert_eq!(claim.deleted_baselines()[0].baseline(), &persisted_baseline);
    assert_ne!(
        result.rollback_specs[0].rollback_baseline, persisted_baseline,
        "filesystem-refreshed rollback data must not become the durable baseline"
    );
    assert!(
        result.rollback_specs[0]
            .rollback_baseline
            .expected_active_files()
            .is_empty(),
        "physical rollback snapshots must not retain persisted-only metadata"
    );
    assert_eq!(result.next_components, claim.after_components());
    assert!(
        result
            .next_components
            .iter()
            .any(|component| component.id() == unaffected.id())
    );
    assert!(
        claim
            .deleted_baselines()
            .iter()
            .all(|entry| entry.component_id() != unaffected.id())
    );
}

#[test]
fn cascade_without_intersection_has_no_claim_and_preserves_projection() {
    let root = tempfile::tempdir().expect("root");
    let component_path = root.path().join("component.dll");
    let unrelated_path = root.path().join("unrelated.dll");
    std::fs::write(&component_path, b"component").expect("component");
    std::fs::write(&unrelated_path, b"unrelated").expect("unrelated");

    let game = test_game("manual:cascade-no-intersection", root.path());
    let component_id = ComponentId::new("component:cascade-no-intersection").expect("id");
    let file = component_file(&component_path);
    let component = test_component(component_id.clone(), game.id().clone(), file.clone());
    let baseline = ComponentRollbackBaseline::new(vec![file]);

    let storage = SqliteStorage::in_memory().expect("storage");
    storage.upsert_game(&game).expect("game");
    storage
        .replace_components_for_game(game.id(), std::slice::from_ref(&component))
        .expect("component");
    storage
        .recover_component_rollback_baseline(game.id(), &component_id, &baseline)
        .expect("baseline");

    let persisted = storage
        .list_components_for_game(game.id())
        .expect("persisted components");
    let result =
        cascade_for_managed_paths(&storage, game.id(), &[unrelated_path]).expect("cascade");

    assert!(result.catalog_claim().is_none());
    assert_eq!(result.next_components, persisted);
    assert!(result.rollback_specs.is_empty());
}

#[test]
fn empty_owned_input_has_no_claim_and_preserves_persisted_projection() {
    let root = tempfile::tempdir().expect("root");
    let component_path = root.path().join("component.dll");
    std::fs::write(&component_path, b"component").expect("component");

    let game = test_game("manual:cascade-empty", root.path());
    let component_id = ComponentId::new("component:cascade-empty").expect("id");
    let component = test_component(
        component_id,
        game.id().clone(),
        component_file(&component_path),
    );

    let storage = SqliteStorage::in_memory().expect("storage");
    storage.upsert_game(&game).expect("game");
    storage
        .replace_components_for_game(game.id(), std::slice::from_ref(&component))
        .expect("component");
    let persisted = storage
        .list_components_for_game(game.id())
        .expect("persisted components");

    let result = cascade_for_managed_paths(&storage, game.id(), &[]).expect("empty cascade");

    assert!(result.catalog_claim().is_none());
    assert!(result.rollback_specs.is_empty());
    assert_eq!(result.next_components, persisted);
    assert!(result.mutation_paths.is_empty());
}

#[test]
fn selected_baselines_are_canonical_and_complete() {
    let root = tempfile::tempdir().expect("root");
    let a_path = root.path().join("a.dll");
    let z_path = root.path().join("z.dll");
    std::fs::write(&a_path, b"a").expect("a");
    std::fs::write(&z_path, b"z").expect("z");

    let game = test_game("manual:cascade-canonical", root.path());
    // Deliberately provide both components and owned paths in reverse
    // order; storage and the claim must impose their own canonical order.
    let z_id = ComponentId::new("component:cascade-z").expect("id");
    let a_id = ComponentId::new("component:cascade-a").expect("id");
    let z_file = component_file(&z_path);
    let a_file = component_file(&a_path);
    let z_component = test_component(z_id.clone(), game.id().clone(), z_file.clone());
    let a_component = test_component(a_id.clone(), game.id().clone(), a_file.clone());
    let z_baseline = ComponentRollbackBaseline::new(vec![z_file]);
    let a_baseline = ComponentRollbackBaseline::new(vec![a_file]);

    let storage = SqliteStorage::in_memory().expect("storage");
    storage.upsert_game(&game).expect("game");
    storage
        .replace_components_for_game(game.id(), &[z_component, a_component])
        .expect("components");
    storage
        .recover_component_rollback_baseline(game.id(), &z_id, &z_baseline)
        .expect("z baseline");
    storage
        .recover_component_rollback_baseline(game.id(), &a_id, &a_baseline)
        .expect("a baseline");

    let result =
        cascade_for_managed_paths(&storage, game.id(), &[z_path, a_path]).expect("cascade");
    let claim = result.catalog_claim().expect("selected rollback claim");

    let before_ids = claim
        .before_components()
        .iter()
        .map(|component| component.id().as_str())
        .collect::<Vec<_>>();
    let deleted_ids = claim
        .deleted_baselines()
        .iter()
        .map(|entry| entry.component_id().as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        before_ids,
        vec!["component:cascade-a", "component:cascade-z"]
    );
    assert_eq!(
        deleted_ids,
        vec!["component:cascade-a", "component:cascade-z"]
    );
    assert_eq!(result.next_components, claim.after_components());
    assert_eq!(claim.after_components().len(), 2);
}

fn test_game(id: &str, root: &std::path::Path) -> GameInstallation {
    GameInstallation::new(
        GameIdentity::new(
            GameId::new(id).expect("game id"),
            "Cascade test game",
            Launcher::Manual,
        )
        .expect("identity"),
        Platform::Windows,
        GameRuntime::NativeWindows,
        path_ref(root),
    )
}

fn test_component(id: ComponentId, game_id: GameId, file: ComponentFile) -> LibraryComponent {
    LibraryComponent::new(
        id,
        game_id,
        ComponentKind::NativeLibrary,
        LibraryTechnology::AmdFsr,
        Swappability::Swappable,
    )
    .with_file(file)
}

fn component_file(path: &std::path::Path) -> ComponentFile {
    ComponentFile::new(path_ref(path))
        .with_sha256(renderpilot_detection::sha256_file(path).expect("hash"))
}

#[test]
fn cascade_never_consumes_a_component_with_auxiliary_rollback_state() {
    let root = tempfile::tempdir().expect("root");
    let runtime = root.path().join("D3D12Core.dll");
    let executable = root.path().join("game.exe");
    std::fs::write(&runtime, b"original runtime").expect("runtime");
    std::fs::write(&executable, b"original executable").expect("executable");

    let game = GameInstallation::new(
        GameIdentity::new(
            GameId::new("manual:cascade-d3d12").expect("game id"),
            "Cascade D3D12",
            Launcher::Manual,
        )
        .expect("identity"),
        Platform::Windows,
        GameRuntime::NativeWindows,
        path_ref(root.path()),
    )
    .with_executable_candidate(path_ref(&executable));
    let component_id = ComponentId::new("component:cascade-d3d12").expect("component id");
    let runtime_file = ComponentFile::new(path_ref(&runtime))
        .with_sha256(renderpilot_detection::sha256_file(&runtime).expect("runtime hash"));
    let component = LibraryComponent::new(
        component_id.clone(),
        game.id().clone(),
        ComponentKind::NativeLibrary,
        LibraryTechnology::D3D12Agility,
        Swappability::Swappable,
    )
    .with_file(runtime_file.clone());
    let executable_hash = renderpilot_detection::sha256_file(&executable).expect("executable hash");
    let baseline = ComponentRollbackBaseline::new(vec![runtime_file]).with_d3d12_executable(
        D3d12ExecutableBaseline::new(
            path_ref(&executable),
            D3d12ExecutableIdentity::new(606, executable_hash.clone()),
            D3d12ExecutableIdentity::new(606, executable_hash),
        ),
    );

    let storage = SqliteStorage::in_memory().expect("storage");
    storage.upsert_game(&game).expect("game");
    storage
        .replace_components_for_game(game.id(), std::slice::from_ref(&component))
        .expect("component");
    storage
        .recover_component_rollback_baseline(game.id(), &component_id, &baseline)
        .expect("baseline");

    let error = cascade_for_managed_paths(&storage, game.id(), &[runtime])
        .expect_err("cascade must not discard auxiliary state");
    assert!(
        error.message().contains("fully roll it back"),
        "unexpected error: {error}"
    );
    assert_eq!(
        storage
            .get_component_backup(&component_id)
            .expect("query")
            .as_ref(),
        Some(&baseline),
        "rejected cascade must preserve the complete aggregate"
    );
}

fn path_ref(path: &std::path::Path) -> PathRef {
    PathRef::new(path.to_string_lossy().into_owned()).expect("path")
}
