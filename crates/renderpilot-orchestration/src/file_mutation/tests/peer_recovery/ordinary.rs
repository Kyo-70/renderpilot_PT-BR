use std::fs;

use renderpilot_domain::GameId;
use renderpilot_storage_sqlite::{BeginFileMutationPreparation, PendingFileMutationState};
use serde_json::json;

use super::super::{commit_empty_mutation, recover_pending, store_game};
use super::fixtures::*;
use crate::Context;

#[test]
fn preparing_peer_row_abandons_without_parsing_its_manifest() {
    let root = tempfile::tempdir().expect("root");
    let context = Context::open_at(root.path().join("catalog.db")).expect("context");
    let game_id = GameId::new("manual:peer-preparing").expect("game id");
    store_game(&context, game_id.clone(), root.path());
    let id = "peer-preparing";
    let transaction_dir = context.file_mutation_root().join(id);
    fs::create_dir_all(&transaction_dir).expect("transaction directory");
    context
        .storage()
        .begin_file_mutation_preparation(&BeginFileMutationPreparation {
            id: id.to_owned(),
            game_id: game_id.clone(),
            feature: renderpilot_domain::mutation_features::LUMA_INSTALL.to_owned(),
            subject_id: None,
            initial_manifest_json: json!({
                "feature": renderpilot_domain::RENODX_INSTALL,
                "peer_program": { "endpoints": [{ "role": "renodx_reshade_ini" }] }
            })
            .to_string(),
        })
        .expect("begin malformed peer row");
    let guard = crate::game_mutation_lock::try_lock(&game_id).expect("guard");

    recover_pending(&context, &guard).expect("preparing rows are abandon-only");

    assert!(!transaction_dir.exists());
    assert!(
        context
            .storage()
            .get_pending_file_mutation(id)
            .expect("row")
            .is_none()
    );
}

#[test]
fn prepared_ordinary_remove_restores_a_missing_endpoint_from_its_snapshot() {
    let root = tempfile::tempdir().expect("root");
    let context = Context::open_at(root.path().join("catalog.db")).expect("context");
    let game_id = GameId::new("manual:peer-ordinary-remove").expect("game id");
    store_game(&context, game_id.clone(), root.path());
    let target = root.path().join("remove.dll");
    let id = "peer-ordinary-remove";
    let transaction_dir = prepare_ordinary_remove_row(&context, &game_id, id, root.path(), &target);
    let guard = crate::game_mutation_lock::try_lock(&game_id).expect("guard");

    recover_pending(&context, &guard).expect("restore ordinary remove");

    assert_eq!(fs::read(&target).expect("restored target"), b"before");
    assert!(!transaction_dir.exists());
    assert!(
        context
            .storage()
            .get_pending_file_mutation(id)
            .expect("row")
            .is_none()
    );
}

#[test]
fn prepared_peer_row_strictly_restores_and_cleans() {
    let root = tempfile::tempdir().expect("root");
    let context = Context::open_at(root.path().join("catalog.db")).expect("context");
    let game_id = GameId::new("manual:peer-prepared").expect("game id");
    store_game(&context, game_id.clone(), root.path());
    let target = root.path().join("peer.dll");
    fs::write(&target, b"after!").expect("after image");
    let id = "peer-prepared";
    let (_manifest, transaction_dir) = prepare_peer_row(&context, &game_id, id, &target);
    let guard = crate::game_mutation_lock::try_lock(&game_id).expect("guard");

    recover_pending(&context, &guard).expect("restore peer row");

    assert_eq!(fs::read(&target).expect("target"), b"before");
    assert!(!transaction_dir.exists());
    assert!(
        context
            .storage()
            .get_pending_file_mutation(id)
            .expect("row")
            .is_none()
    );
}

#[test]
fn prepared_peer_row_rejects_same_before_content_with_foreign_identity() {
    let root = tempfile::tempdir().expect("root");
    let context = Context::open_at(root.path().join("catalog.db")).expect("context");
    let game_id = GameId::new("manual:peer-before-identity-mismatch").expect("game id");
    store_game(&context, game_id.clone(), root.path());
    let target = root.path().join("peer.dll");
    let id = "peer-before-identity-mismatch";
    let transaction_dir = context.file_mutation_root().join(id);
    fs::create_dir_all(&transaction_dir).expect("transaction directory");
    fs::write(transaction_dir.join("before.bin"), b"before").expect("before snapshot");
    fs::write(&target, b"before").expect("before endpoint");
    let mut manifest: serde_json::Value =
        serde_json::from_str(&peer_manifest(root.path(), &transaction_dir, id, &target))
            .expect("manifest");
    manifest["peer_program"]["endpoints"][0]["before"]["identity"] =
        json!("foreign-before-identity");
    context
        .storage()
        .begin_file_mutation_preparation(&BeginFileMutationPreparation {
            id: id.to_owned(),
            game_id: game_id.clone(),
            feature: renderpilot_domain::mutation_features::LUMA_INSTALL.to_owned(),
            subject_id: None,
            initial_manifest_json: "{}".to_owned(),
        })
        .expect("begin peer row");
    context
        .storage()
        .finish_preparing_file_mutation(id, &manifest.to_string())
        .expect("finish peer row");
    let guard = crate::game_mutation_lock::try_lock(&game_id).expect("guard");

    let error = recover_pending(&context, &guard)
        .expect_err("same digest and length with a different identity must fail closed");

    assert!(error.to_string().contains("requires repair"));
    assert_eq!(fs::read(&target).expect("endpoint"), b"before");
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
fn prepared_peer_row_restores_endpoint_then_removes_empty_ancestors() {
    let root = tempfile::tempdir().expect("root");
    let context = Context::open_at(root.path().join("catalog.db")).expect("context");
    let game_id = GameId::new("manual:peer-nested-empty").expect("game id");
    store_game(&context, game_id.clone(), root.path());
    let target = root.path().join("nested/deeper/peer.dll");
    fs::create_dir_all(target.parent().expect("target parent")).expect("parents");
    fs::write(&target, b"after!").expect("after image");
    let id = "peer-nested-empty";
    let transaction_dir =
        prepare_nested_peer_row(&context, &game_id, id, root.path(), &target, None);
    let guard = crate::game_mutation_lock::try_lock(&game_id).expect("guard");

    recover_pending(&context, &guard).expect("restore peer row");

    assert!(!target.exists());
    assert!(!root.path().join("nested").exists());
    assert!(!transaction_dir.exists());
    assert!(
        context
            .storage()
            .get_pending_file_mutation(id)
            .expect("row")
            .is_none()
    );
}

#[test]
fn prepared_ordinary_create_that_never_applied_converges_without_compensation() {
    let root = tempfile::tempdir().expect("root");
    let context = Context::open_at(root.path().join("catalog.db")).expect("context");
    let game_id = GameId::new("manual:peer-ordinary-create-absent").expect("game id");
    store_game(&context, game_id.clone(), root.path());
    let target = root.path().join("nested/deeper/peer.dll");
    let id = "peer-ordinary-create-absent";
    let transaction_dir =
        prepare_nested_peer_row(&context, &game_id, id, root.path(), &target, None);
    let guard = crate::game_mutation_lock::try_lock(&game_id).expect("guard");

    recover_pending(&context, &guard).expect("unapplied create is already restored");

    assert!(!target.exists());
    assert!(!root.path().join("nested").exists());
    assert!(!transaction_dir.exists());
    assert!(
        context
            .storage()
            .get_pending_file_mutation(id)
            .expect("row")
            .is_none()
    );
}

#[test]
fn prepared_ordinary_create_that_applied_is_compensated_and_cleans_ancestors() {
    let root = tempfile::tempdir().expect("root");
    let context = Context::open_at(root.path().join("catalog.db")).expect("context");
    let game_id = GameId::new("manual:peer-ordinary-create-applied").expect("game id");
    store_game(&context, game_id.clone(), root.path());
    let target = root.path().join("nested/deeper/peer.dll");
    let id = "peer-ordinary-create-applied";
    let transaction_dir =
        prepare_nested_peer_row(&context, &game_id, id, root.path(), &target, None);
    fs::create_dir_all(target.parent().expect("target parent")).expect("parents");
    fs::write(&target, b"after!").expect("applied endpoint");
    let guard = crate::game_mutation_lock::try_lock(&game_id).expect("guard");

    recover_pending(&context, &guard).expect("compensate applied create");

    assert!(!target.exists());
    assert!(!root.path().join("nested").exists());
    assert!(!transaction_dir.exists());
    assert!(
        context
            .storage()
            .get_pending_file_mutation(id)
            .expect("row")
            .is_none()
    );
}

#[test]
fn prepared_ordinary_create_with_foreign_file_is_retained_for_repair() {
    let root = tempfile::tempdir().expect("root");
    let context = Context::open_at(root.path().join("catalog.db")).expect("context");
    let game_id = GameId::new("manual:peer-ordinary-create-foreign").expect("game id");
    store_game(&context, game_id.clone(), root.path());
    let target = root.path().join("nested/deeper/peer.dll");
    let id = "peer-ordinary-create-foreign";
    let transaction_dir =
        prepare_nested_peer_row(&context, &game_id, id, root.path(), &target, None);
    fs::create_dir_all(target.parent().expect("target parent")).expect("parents");
    fs::write(&target, b"foreign").expect("foreign endpoint");
    let guard = crate::game_mutation_lock::try_lock(&game_id).expect("guard");

    let error = recover_pending(&context, &guard).expect_err("foreign create must fail closed");

    assert!(error.to_string().contains("requires repair"));
    assert_eq!(fs::read(&target).expect("foreign endpoint"), b"foreign");
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
fn prepared_peer_row_preserves_nonempty_ancestor_but_completes() {
    let root = tempfile::tempdir().expect("root");
    let context = Context::open_at(root.path().join("catalog.db")).expect("context");
    let game_id = GameId::new("manual:peer-nested-residual").expect("game id");
    store_game(&context, game_id.clone(), root.path());
    let target = root.path().join("nested/deeper/peer.dll");
    let deeper = target.parent().expect("target parent");
    fs::create_dir_all(deeper).expect("parents");
    fs::write(&target, b"after!").expect("after image");
    fs::write(deeper.join("foreign.dll"), b"foreign").expect("foreign residual");
    let id = "peer-nested-residual";
    let transaction_dir =
        prepare_nested_peer_row(&context, &game_id, id, root.path(), &target, None);
    let guard = crate::game_mutation_lock::try_lock(&game_id).expect("guard");

    recover_pending(&context, &guard).expect("restore peer row");

    assert!(!target.exists());
    assert!(deeper.join("foreign.dll").exists());
    assert!(deeper.is_dir());
    assert!(root.path().join("nested").is_dir());
    assert!(!transaction_dir.exists());
    assert!(
        context
            .storage()
            .get_pending_file_mutation(id)
            .expect("row")
            .is_none()
    );
}

#[test]
fn prepared_peer_row_retains_ancestors_when_endpoint_restore_fails() {
    let root = tempfile::tempdir().expect("root");
    let context = Context::open_at(root.path().join("catalog.db")).expect("context");
    let game_id = GameId::new("manual:peer-nested-restore-failure").expect("game id");
    store_game(&context, game_id.clone(), root.path());
    let target = root.path().join("nested/deeper/peer.dll");
    let deeper = target.parent().expect("target parent");
    fs::create_dir_all(deeper).expect("parents");
    fs::write(&target, b"after!").expect("after image");
    let id = "peer-nested-restore-failure";
    let transaction_dir = context.file_mutation_root().join(id);
    let snapshot = transaction_dir.join("before.bin");
    let _ = prepare_nested_peer_row(
        &context,
        &game_id,
        id,
        root.path(),
        &target,
        Some(&snapshot),
    );
    fs::remove_file(&snapshot).expect("remove restore source");
    let guard = crate::game_mutation_lock::try_lock(&game_id).expect("guard");

    let error = recover_pending(&context, &guard).expect_err("restore must fail");

    assert!(
        error.to_string().contains("source file") || error.to_string().contains("snapshot"),
        "unexpected restore error: {error}"
    );
    assert!(root.path().join("nested").is_dir());
    assert!(deeper.is_dir());
    assert!(
        context
            .storage()
            .get_pending_file_mutation(id)
            .expect("row")
            .is_some_and(|row| row.state == PendingFileMutationState::Prepared)
    );
}

#[test]
fn prepared_peer_row_out_of_transaction_snapshot_blocks_before_live_write() {
    let root = tempfile::tempdir().expect("root");
    let context = Context::open_at(root.path().join("catalog.db")).expect("context");
    let game_id = GameId::new("manual:peer-out-of-transaction").expect("game id");
    store_game(&context, game_id.clone(), root.path());
    let target = root.path().join("peer.dll");
    fs::write(&target, b"after!").expect("after image");
    let id = "peer-out-of-transaction";
    let transaction_dir = context.file_mutation_root().join(id);
    fs::create_dir_all(&transaction_dir).expect("transaction directory");
    fs::write(transaction_dir.join("before.bin"), b"before").expect("before snapshot");
    let outside_snapshot = root.path().join("outside-before.bin");
    fs::write(&outside_snapshot, b"before").expect("outside snapshot");
    let mut manifest: serde_json::Value =
        serde_json::from_str(&peer_manifest(root.path(), &transaction_dir, id, &target))
            .expect("manifest");
    manifest["snapshots"][0]["snapshot"] = json!(slash(&outside_snapshot));
    context
        .storage()
        .begin_file_mutation_preparation(&BeginFileMutationPreparation {
            id: id.to_owned(),
            game_id: game_id.clone(),
            feature: renderpilot_domain::mutation_features::LUMA_INSTALL.to_owned(),
            subject_id: None,
            initial_manifest_json: "{}".to_owned(),
        })
        .expect("begin row");
    context
        .storage()
        .finish_preparing_file_mutation(id, &manifest.to_string())
        .expect("finish row");
    let guard = crate::game_mutation_lock::try_lock(&game_id).expect("guard");

    let error =
        recover_pending(&context, &guard).expect_err("outside snapshot must block recovery");

    assert!(error.to_string().contains("requires repair"));
    assert_eq!(fs::read(&target).expect("target"), b"after!");
    assert!(outside_snapshot.exists());
    assert!(
        context
            .storage()
            .get_pending_file_mutation(id)
            .expect("row")
            .is_some()
    );
}

#[test]
fn malformed_prepared_peer_row_is_retained_for_repair() {
    let root = tempfile::tempdir().expect("root");
    let context = Context::open_at(root.path().join("catalog.db")).expect("context");
    let game_id = GameId::new("manual:peer-malformed").expect("game id");
    store_game(&context, game_id.clone(), root.path());
    let target = root.path().join("peer.dll");
    fs::write(&target, b"after!").expect("after image");
    let id = "peer-malformed";
    let transaction_dir = context.file_mutation_root().join(id);
    fs::create_dir_all(&transaction_dir).expect("transaction directory");
    fs::write(transaction_dir.join("before.bin"), b"before").expect("before snapshot");
    let malformed = json!({
        "format_version": 1,
        "roots": [root.path().to_string_lossy()],
        "transaction_dir": transaction_dir.to_string_lossy(),
        "snapshots": [],
        "peer_program": { "format": 2 }
    })
    .to_string();
    context
        .storage()
        .begin_file_mutation_preparation(&BeginFileMutationPreparation {
            id: id.to_owned(),
            game_id: game_id.clone(),
            feature: "test".to_owned(),
            subject_id: None,
            initial_manifest_json: "{}".to_owned(),
        })
        .expect("reserve malformed peer row");
    context
        .storage()
        .finish_preparing_file_mutation(id, &malformed)
        .expect("finish malformed peer row");
    let guard = crate::game_mutation_lock::try_lock(&game_id).expect("guard");
    let error = recover_pending(&context, &guard).expect_err("malformed peer retained");

    assert!(error.to_string().contains("requires repair"));
    assert_eq!(
        context
            .storage()
            .get_pending_file_mutation(id)
            .expect("row")
            .expect("retained row")
            .state,
        PendingFileMutationState::Prepared
    );
    assert!(transaction_dir.exists());
}

#[test]
fn malformed_committed_peer_row_is_retained_for_repair() {
    let root = tempfile::tempdir().expect("root");
    let context = Context::open_at(root.path().join("catalog.db")).expect("context");
    let game_id = GameId::new("manual:peer-committed-malformed").expect("game id");
    store_game(&context, game_id.clone(), root.path());
    let target = root.path().join("peer.dll");
    fs::write(&target, b"after!").expect("after image");
    let id = "peer-committed-malformed";
    let transaction_dir = context.file_mutation_root().join(id);
    fs::create_dir_all(&transaction_dir).expect("transaction directory");
    let malformed = json!({
        "format_version": 1,
        "roots": [root.path().to_string_lossy()],
        "transaction_dir": transaction_dir.to_string_lossy(),
        "snapshots": [],
        "peer_program": { "format": 2 }
    })
    .to_string();
    context
        .storage()
        .begin_file_mutation_preparation(&BeginFileMutationPreparation {
            id: id.to_owned(),
            game_id: game_id.clone(),
            feature: "test".to_owned(),
            subject_id: None,
            initial_manifest_json: "{}".to_owned(),
        })
        .expect("reserve malformed peer row");
    context
        .storage()
        .finish_preparing_file_mutation(id, &malformed)
        .expect("finish malformed peer row");
    commit_empty_mutation(&context, &game_id, id).expect("commit row");
    let guard = crate::game_mutation_lock::try_lock(&game_id).expect("guard");

    let error = recover_pending(&context, &guard).expect_err("malformed committed retained");

    assert!(error.to_string().contains("requires repair"));
    assert_eq!(
        context
            .storage()
            .get_pending_file_mutation(id)
            .expect("row")
            .expect("retained row")
            .state,
        PendingFileMutationState::Committed
    );
    assert!(transaction_dir.exists());
}

#[test]
fn committed_peer_row_only_cleans_and_never_restores() {
    let root = tempfile::tempdir().expect("root");
    let context = Context::open_at(root.path().join("catalog.db")).expect("context");
    let game_id = GameId::new("manual:peer-committed").expect("game id");
    store_game(&context, game_id.clone(), root.path());
    let target = root.path().join("peer.dll");
    fs::write(&target, b"after!").expect("after image");
    let id = "peer-committed";
    let (_manifest, transaction_dir) = prepare_peer_row(&context, &game_id, id, &target);
    commit_empty_mutation(&context, &game_id, id).expect("commit row");
    let guard = crate::game_mutation_lock::try_lock(&game_id).expect("guard");

    recover_pending(&context, &guard).expect("cleanup committed peer");

    assert_eq!(fs::read(&target).expect("target"), b"after!");
    assert!(!transaction_dir.exists());
    assert!(
        context
            .storage()
            .get_pending_file_mutation(id)
            .expect("row")
            .is_none()
    );
}
