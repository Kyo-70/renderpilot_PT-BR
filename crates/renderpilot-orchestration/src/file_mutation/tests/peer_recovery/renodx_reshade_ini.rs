use std::fs;

use renderpilot_domain::GameId;
use renderpilot_storage_sqlite::{BeginFileMutationPreparation, PendingFileMutationState};
use serde_json::json;

use super::super::{commit_empty_mutation, recover_pending, store_game};
use super::fixtures::*;
use crate::Context;

fn prepare_typed_renodx_row(
    context: &Context,
    game_id: &GameId,
    id: &str,
    root: &std::path::Path,
) -> std::path::PathBuf {
    let target = root.join("ReShade.ini");
    fs::write(&target, b"after!").expect("after image");
    let transaction_dir = context.file_mutation_root().join(id);
    fs::create_dir_all(&transaction_dir).expect("transaction directory");
    fs::write(transaction_dir.join("before.bin"), b"before").expect("before snapshot");
    let mut manifest: serde_json::Value =
        serde_json::from_str(&peer_manifest(root, &transaction_dir, id, &target))
            .expect("manifest");
    manifest["peer_program"]["endpoints"][0]["role"] = json!("renodx_reshade_ini");
    manifest["peer_program"]["endpoints"][0]["read_guards"] =
        json!([format!("{}:ReShade.ini", slash(root))]);
    context
        .storage()
        .begin_file_mutation_preparation(&BeginFileMutationPreparation {
            id: id.to_owned(),
            game_id: game_id.clone(),
            feature: renderpilot_domain::RENODX_INSTALL.to_owned(),
            subject_id: None,
            initial_manifest_json: "{}".to_owned(),
        })
        .expect("begin typed row");
    context
        .storage()
        .finish_preparing_file_mutation(id, &manifest.to_string())
        .expect("finish typed row");
    transaction_dir
}

fn rewrite_file_mutation_feature(context: &Context, id: &str, feature: &str) {
    let database = context
        .storage()
        .catalog_file_path()
        .expect("catalog path")
        .expect("catalog database");
    let connection = rusqlite::Connection::open(database).expect("fixture connection");
    connection
        .execute(
            "UPDATE pending_file_mutations SET feature = ?1 WHERE id = ?2",
            rusqlite::params![feature, id],
        )
        .expect("rewrite durable feature");
}

#[test]
fn prepared_typed_renodx_row_with_durable_feature_drift_is_retained_without_writes() {
    let root = tempfile::tempdir().expect("root");
    let context = Context::open_at(root.path().join("catalog.db")).expect("context");
    let game_id = GameId::new("manual:peer-renodx-feature-drift-prepared").expect("game id");
    store_game(&context, game_id.clone(), root.path());
    let id = "peer-renodx-feature-drift-prepared";
    let transaction_dir = prepare_typed_renodx_row(&context, &game_id, id, root.path());
    rewrite_file_mutation_feature(
        &context,
        id,
        renderpilot_domain::mutation_features::LUMA_INSTALL,
    );
    let guard = crate::game_mutation_lock::try_lock(&game_id).expect("guard");

    let error = recover_pending(&context, &guard).expect_err("feature drift must fail closed");

    assert!(error.to_string().contains("requires repair"));
    assert_eq!(
        fs::read(root.path().join("ReShade.ini")).expect("target"),
        b"after!"
    );
    assert!(transaction_dir.exists());
    assert!(
        context
            .storage()
            .get_pending_file_mutation(id)
            .expect("row")
            .is_some_and(|row| row.state == PendingFileMutationState::Prepared)
    );
}

#[test]
fn committed_typed_renodx_row_with_durable_feature_drift_is_retained_without_cleanup() {
    let root = tempfile::tempdir().expect("root");
    let context = Context::open_at(root.path().join("catalog.db")).expect("context");
    let game_id = GameId::new("manual:peer-renodx-feature-drift-committed").expect("game id");
    store_game(&context, game_id.clone(), root.path());
    let id = "peer-renodx-feature-drift-committed";
    let transaction_dir = prepare_typed_renodx_row(&context, &game_id, id, root.path());
    rewrite_file_mutation_feature(
        &context,
        id,
        renderpilot_domain::mutation_features::LUMA_INSTALL,
    );
    commit_empty_mutation(&context, &game_id, id).expect("commit row");
    let guard = crate::game_mutation_lock::try_lock(&game_id).expect("guard");

    let error = recover_pending(&context, &guard).expect_err("feature drift must fail closed");

    assert!(error.to_string().contains("requires repair"));
    assert_eq!(
        fs::read(root.path().join("ReShade.ini")).expect("target"),
        b"after!"
    );
    assert!(transaction_dir.exists());
    assert!(
        context
            .storage()
            .get_pending_file_mutation(id)
            .expect("row")
            .is_some_and(|row| row.state == PendingFileMutationState::Committed)
    );
}
