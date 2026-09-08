//! Component identity rekey validation tests.

use super::*;
#[test]
fn component_rekeys_are_one_to_one_across_the_whole_plan() {
    let storage = SqliteStorage::in_memory().expect("storage");
    let destination = game("game:destination", "C:/Games/Example");
    let first_source = game("manual:first", "C:/Games/Example/First");
    let second_source = game("manual:second", "C:/Games/Example/Second");
    let destination_component =
        component("component:destination", destination.id(), "C:/Games/a.dll");
    let first_component = component("component:first", first_source.id(), "C:/Games/a.dll");
    let second_component = component("component:second", second_source.id(), "C:/Games/a.dll");
    for game in [&destination, &first_source, &second_source] {
        storage.upsert_game(game).expect("game");
    }
    storage
        .replace_components_for_game(
            destination.id(),
            std::slice::from_ref(&destination_component),
        )
        .expect("destination component");
    storage
        .replace_components_for_game(first_source.id(), std::slice::from_ref(&first_component))
        .expect("first component");
    storage
        .replace_components_for_game(second_source.id(), std::slice::from_ref(&second_component))
        .expect("second component");

    let plan = ConsolidationPlan {
        destination_game_id: destination.id().clone(),
        sources: vec![
            ConsolidationSource {
                source_game_id: first_source.id().clone(),
                component_rekeys: vec![ComponentRekey {
                    source_component_id: first_component.id().as_str().to_owned(),
                    destination_component_id: destination_component.id().as_str().to_owned(),
                }],
            },
            ConsolidationSource {
                source_game_id: second_source.id().clone(),
                component_rekeys: vec![ComponentRekey {
                    source_component_id: second_component.id().as_str().to_owned(),
                    destination_component_id: destination_component.id().as_str().to_owned(),
                }],
            },
        ],
    };

    let error = storage
        .inspect_consolidation_conflicts(&plan)
        .expect_err("cross-source destination reuse must be rejected");
    assert!(error.message().contains("whole consolidation plan"));
}

#[test]
fn component_rekey_rejects_a_destination_owned_by_another_game() {
    let storage = SqliteStorage::in_memory().expect("storage");
    let destination = game("game:destination", "C:/Games/Example");
    let source = game("manual:child", "C:/Games/Example/D3D12");
    let unrelated = game("game:unrelated", "C:/Games/Other");
    let source_component = component(
        "component:source",
        source.id(),
        "C:/Games/Example/D3D12/a.dll",
    );
    let unrelated_component = component("component:other", unrelated.id(), "C:/Games/Other/a.dll");
    for game in [&destination, &source, &unrelated] {
        storage.upsert_game(game).expect("game");
    }
    storage
        .replace_components_for_game(source.id(), std::slice::from_ref(&source_component))
        .expect("source component");
    storage
        .replace_components_for_game(unrelated.id(), std::slice::from_ref(&unrelated_component))
        .expect("unrelated component");
    let plan = ConsolidationPlan {
        destination_game_id: destination.id().clone(),
        sources: vec![ConsolidationSource {
            source_game_id: source.id().clone(),
            component_rekeys: vec![ComponentRekey {
                source_component_id: source_component.id().as_str().to_owned(),
                destination_component_id: unrelated_component.id().as_str().to_owned(),
            }],
        }],
    };
    let conflicts = storage
        .inspect_consolidation_conflicts(&plan)
        .expect("preview");

    let error = storage
        .save_install_scan_with_consolidation(
            ScanWriteUnit {
                game: &destination,
                components: &[],
                artifacts: &[],
                prune_empty_operations: false,
            },
            &plan,
            &conflicts,
        )
        .expect_err("foreign component ownership must fail");

    assert!(error.message().contains("does not belong"));
    assert!(storage.find_game(source.id()).expect("source").is_some());
}

#[test]
fn changed_conflict_preview_aborts_before_scan_write() {
    let storage = SqliteStorage::in_memory().expect("storage");
    let destination = game("game:destination", "C:/Games/Example");
    let source = game("manual:child", "C:/Games/Example/D3D12");
    storage.upsert_game(&destination).expect("destination");
    storage.upsert_game(&source).expect("source");

    let plan = ConsolidationPlan {
        destination_game_id: destination.id().clone(),
        sources: vec![ConsolidationSource {
            source_game_id: source.id().clone(),
            component_rekeys: Vec::new(),
        }],
    };
    let preview = storage
        .inspect_consolidation_conflicts(&plan)
        .expect("empty conflict preview");
    assert!(!preview.requires_recovery_bundle());

    storage
        .upsert_game_cover(destination.id(), "destination.webp")
        .expect("destination cover");
    storage
        .upsert_game_cover(source.id(), "source.webp")
        .expect("source cover");

    let error = storage
        .save_install_scan_with_consolidation(
            ScanWriteUnit {
                game: &destination,
                components: &[],
                artifacts: &[],
                prune_empty_operations: false,
            },
            &plan,
            &preview,
        )
        .expect_err("stale conflict preview must abort");

    assert!(error.message().contains("conflict state changed"));
    assert!(
        storage
            .find_game(source.id())
            .expect("find source")
            .is_some(),
        "source must remain after a stale preview"
    );
}
