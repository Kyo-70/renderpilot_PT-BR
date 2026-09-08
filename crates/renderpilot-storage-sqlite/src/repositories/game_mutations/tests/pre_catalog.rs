use super::*;

#[test]
fn pre_catalog_mutation_permits_only_addon_commit_effects() {
    let storage = SqliteStorage::in_memory().expect("storage");
    let game_id = GameId::new("steam:pre-catalog-addon").expect("id");
    prepare_pre_catalog_mutation(&storage, &game_id, "tx-pre-catalog-addon");
    let addon = InstalledAddon::new(
        game_id.clone(),
        AddonKind::Luma,
        PathRef::new(r"C:\Games\Test\Luma-Game.addon").expect("path"),
    );

    storage
        .commit_game_mutation(GameMutationCommit {
            game_id: &game_id,
            component_set: None,
            baseline_mutations: &[],
            addon: InstalledAddonMutation::Upsert(&addon),
            mutation_id: Some("tx-pre-catalog-addon"),
        })
        .expect("pre-catalog add-on commit");

    assert!(
        storage
            .get_installed_addon(&game_id)
            .expect("addon")
            .is_some()
    );
    assert_eq!(
        storage
            .get_pending_file_mutation("tx-pre-catalog-addon")
            .expect("row")
            .expect("row")
            .state,
        PendingFileMutationState::Committed
    );
    assert!(storage.catalog_readiness(&game_id).is_err());
}

#[test]
fn pre_catalog_mutation_allows_empty_component_cleanup_without_creating_catalog_state() {
    let storage = SqliteStorage::in_memory().expect("storage");
    let game_id = GameId::new("steam:pre-catalog-empty-cleanup").expect("id");
    let addon = InstalledAddon::new(
        game_id.clone(),
        AddonKind::Luma,
        PathRef::new(r"C:\Games\Test\Luma-Game.addon").expect("path"),
    );
    storage
        .upsert_installed_addon(&addon)
        .expect("seed orphan add-on");
    prepare_pre_catalog_mutation(&storage, &game_id, "tx-pre-catalog-empty-cleanup");

    storage
        .commit_game_mutation(GameMutationCommit {
            game_id: &game_id,
            component_set: Some(&[]),
            baseline_mutations: &[],
            addon: InstalledAddonMutation::Delete(AddonKind::Luma),
            mutation_id: Some("tx-pre-catalog-empty-cleanup"),
        })
        .expect("empty orphan component cleanup is a no-op");

    assert!(storage.find_game(&game_id).expect("game query").is_none());
    assert!(storage.catalog_readiness(&game_id).is_err());
    assert!(
        storage
            .list_components_for_game(&game_id)
            .expect("component query")
            .is_empty()
    );
    assert!(
        storage
            .get_installed_addon(&game_id)
            .expect("add-on query")
            .is_none()
    );
}

#[test]
fn pre_catalog_mutation_rejects_nonempty_component_and_baseline_effects() {
    let storage = SqliteStorage::in_memory().expect("storage");
    let game_id = GameId::new("steam:pre-catalog-catalog-write").expect("id");
    prepare_pre_catalog_mutation(&storage, &game_id, "tx-pre-catalog-nonempty");

    let component = LibraryComponent::new(
        ComponentId::new("component:pre-catalog-set").expect("component id"),
        game_id.clone(),
        ComponentKind::NativeLibrary,
        LibraryTechnology::DlssSuperResolution,
        Swappability::BundleOnly,
    );
    storage
        .commit_game_mutation(GameMutationCommit {
            game_id: &game_id,
            component_set: Some(std::slice::from_ref(&component)),
            baseline_mutations: &[],
            addon: InstalledAddonMutation::Keep,
            mutation_id: Some("tx-pre-catalog-nonempty"),
        })
        .expect_err("pre-catalog component write must fail");

    let component_id = ComponentId::new("component:pre-catalog").expect("component id");
    let baseline = [ComponentBaselineMutation::Delete {
        component_id: &component_id,
    }];
    prepare_pre_catalog_mutation(&storage, &game_id, "tx-pre-catalog-baseline");
    storage
        .commit_game_mutation(GameMutationCommit {
            game_id: &game_id,
            component_set: None,
            baseline_mutations: &baseline,
            addon: InstalledAddonMutation::Keep,
            mutation_id: Some("tx-pre-catalog-baseline"),
        })
        .expect_err("pre-catalog baseline write must fail");
    assert_eq!(
        storage
            .get_pending_file_mutation("tx-pre-catalog-nonempty")
            .expect("row")
            .expect("row")
            .state,
        PendingFileMutationState::Prepared
    );
    assert_eq!(
        storage
            .get_pending_file_mutation("tx-pre-catalog-baseline")
            .expect("row")
            .expect("row")
            .state,
        PendingFileMutationState::Prepared
    );
}

#[test]
fn late_catalog_insertion_binds_prepared_addon_commit_before_feature_writes() {
    let storage = SqliteStorage::in_memory().expect("storage");
    let game_id = GameId::new("steam:late-catalog-binding").expect("id");
    prepare_pre_catalog_mutation(&storage, &game_id, "tx-late-catalog-binding");
    let game = test_game(game_id.clone());
    storage.upsert_game(&game).expect("late game insert");
    let addon = InstalledAddon::new(
        game_id.clone(),
        AddonKind::Luma,
        PathRef::new(r"C:\Games\Test\Luma-Game.addon").expect("path"),
    );

    storage
        .commit_game_mutation(GameMutationCommit {
            game_id: &game_id,
            component_set: Some(&[]),
            baseline_mutations: &[],
            addon: InstalledAddonMutation::Upsert(&addon),
            mutation_id: Some("tx-late-catalog-binding"),
        })
        .expect("late-bound add-on commit");
    assert_eq!(
        storage.catalog_readiness(&game_id).expect("authority"),
        CatalogReadiness::Invalidated {
            authority_epoch: 1,
            reason: "prepared_file_mutation".to_owned(),
            mutation_token: Some("tx-late-catalog-binding".to_owned()),
        }
    );
}

#[test]
fn complete_scan_publication_is_excluded_while_late_bound_mutation_is_pending() {
    let storage = SqliteStorage::in_memory().expect("storage");
    let game_id = GameId::new("steam:pending-complete-exclusion").expect("id");
    prepare_pre_catalog_mutation(&storage, &game_id, "tx-pending-complete-exclusion");
    let game = test_game(game_id.clone());
    storage.upsert_game(&game).expect("late game insert");

    storage
        .save_complete_scan_write_unit(CompleteScanWriteUnit {
            game: &game,
            components: &[],
            artifacts: &[],
            observations: &[],
            authority: AuthorityCas::new(0),
            prune_empty_operations: false,
        })
        .expect_err("a pending file mutation must exclude Complete publication");
    assert!(matches!(
        storage.catalog_readiness(&game_id).expect("authority"),
        CatalogReadiness::NeverCompleted { authority_epoch: 0 }
    ));
}
