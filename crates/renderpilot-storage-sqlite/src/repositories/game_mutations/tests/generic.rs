use super::*;

#[test]
fn commit_game_mutation_rolls_back_addon_when_mutation_mark_fails() {
    let storage = SqliteStorage::in_memory().expect("storage");
    let game_id = GameId::new("steam:mutation-atomic").expect("id");
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
            mutation_id: Some("missing-tx"),
        })
        .expect_err("missing mutation id must fail the whole commit");

    assert!(
        storage
            .get_installed_addon(&game_id)
            .expect("query")
            .is_none(),
        "addon upsert must roll back when mutation mark fails"
    );
}

#[test]
fn commit_game_mutation_marks_prepared_mutation_with_addon_upsert() {
    let storage = SqliteStorage::in_memory().expect("storage");
    let game_id = GameId::new("steam:mutation-ok").expect("id");
    storage
        .upsert_game(&test_game(game_id.clone()))
        .expect("store game");
    let addon = InstalledAddon::new(
        game_id.clone(),
        AddonKind::Luma,
        PathRef::new(r"C:\Games\Test\Luma-Game.addon").expect("path"),
    );
    storage
        .prepare_file_mutation(&PendingFileMutationRow {
            id: "tx-ok".to_owned(),
            game_id: game_id.clone(),
            feature: renderpilot_domain::mutation_features::LUMA_UPDATE.to_owned(),
            subject_id: None,
            state: PendingFileMutationState::Preparing,
            manifest_json: r#"{"snapshots":[]}"#.to_owned(),
        })
        .expect("prepare");
    storage
        .finish_preparing_file_mutation("tx-ok", r#"{"snapshots":[]}"#)
        .expect("finish prepare");

    storage
        .commit_game_mutation(GameMutationCommit {
            game_id: &game_id,
            component_set: Some(&[]),
            baseline_mutations: &[],
            addon: InstalledAddonMutation::Upsert(&addon),
            mutation_id: Some("tx-ok"),
        })
        .expect("commit");

    assert!(
        storage
            .get_installed_addon(&game_id)
            .expect("query")
            .is_some()
    );
    assert_eq!(
        storage
            .get_pending_file_mutation("tx-ok")
            .expect("get")
            .expect("row")
            .state,
        PendingFileMutationState::Committed
    );
    assert_eq!(
        storage.catalog_readiness(&game_id).expect("readiness"),
        CatalogReadiness::Invalidated {
            authority_epoch: 1,
            reason: "prepared_file_mutation".to_owned(),
            mutation_token: Some("tx-ok".to_owned()),
        }
    );
}

#[test]
fn metadata_only_mutation_leaves_a_complete_authority_unchanged() {
    let storage = SqliteStorage::in_memory().expect("storage");
    let game_id = GameId::new("steam:metadata-only").expect("id");
    let game = test_game(game_id.clone());
    complete_game_scan(&storage, &game);

    storage
        .commit_game_mutation(GameMutationCommit {
            game_id: &game_id,
            component_set: None,
            baseline_mutations: &[],
            addon: InstalledAddonMutation::Keep,
            mutation_id: None,
        })
        .expect("metadata-only mutation");

    let CatalogReadiness::Complete(ready) = storage.catalog_readiness(&game_id).expect("readiness")
    else {
        panic!("metadata-only mutation must preserve Complete authority");
    };
    assert_eq!(ready.game_id(), &game_id);
    assert_eq!(ready.authority_epoch(), 1);
}

#[test]
fn component_set_without_file_mutation_invalidates_authority() {
    let storage = SqliteStorage::in_memory().expect("storage");
    let game_id = GameId::new("steam:component-set").expect("id");
    let game = test_game(game_id.clone());
    complete_game_scan(&storage, &game);

    storage
        .commit_game_mutation(GameMutationCommit {
            game_id: &game_id,
            component_set: Some(&[]),
            baseline_mutations: &[],
            addon: InstalledAddonMutation::Keep,
            mutation_id: None,
        })
        .expect("component mutation");

    assert_eq!(
        storage.catalog_readiness(&game_id).expect("readiness"),
        CatalogReadiness::Invalidated {
            authority_epoch: 2,
            reason: "game_mutation_component_set".to_owned(),
            mutation_token: None,
        }
    );
}

#[test]
fn mutation_id_requires_same_game_prepared_row_with_matching_invalidation() {
    let storage = SqliteStorage::in_memory().expect("storage");
    let game_id = GameId::new("steam:mutation-conditions").expect("id");
    let other_game_id = GameId::new("steam:mutation-other-game").expect("id");
    let game = test_game(game_id.clone());
    let other_game = test_game(other_game_id.clone());
    complete_game_scan(&storage, &game);
    complete_game_scan(&storage, &other_game);
    prepare_mutation(&storage, &other_game_id, "tx-other-game");

    storage
        .commit_game_mutation(GameMutationCommit {
            game_id: &game_id,
            component_set: Some(&[]),
            baseline_mutations: &[],
            addon: InstalledAddonMutation::Keep,
            mutation_id: Some("tx-other-game"),
        })
        .expect_err("a different game's prepared mutation cannot commit");

    storage
        .prepare_file_mutation(&PendingFileMutationRow {
            id: "tx-without-invalidation".to_owned(),
            game_id: game_id.clone(),
            feature: renderpilot_domain::mutation_features::LUMA_UPDATE.to_owned(),
            subject_id: None,
            state: PendingFileMutationState::Prepared,
            manifest_json: r#"{"snapshots":[]}"#.to_owned(),
        })
        .expect("fixture prepared row");
    storage
        .commit_game_mutation(GameMutationCommit {
            game_id: &game_id,
            component_set: None,
            baseline_mutations: &[],
            addon: InstalledAddonMutation::Keep,
            mutation_id: Some("tx-without-invalidation"),
        })
        .expect_err("prepared state without matching invalidation cannot commit");
    let CatalogReadiness::Complete(ready) = storage.catalog_readiness(&game_id).expect("readiness")
    else {
        panic!("rejected mutation must preserve Complete authority");
    };
    assert_eq!(ready.game_id(), &game_id);
    assert_eq!(ready.authority_epoch(), 1);
}
