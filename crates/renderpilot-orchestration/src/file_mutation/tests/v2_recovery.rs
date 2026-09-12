use super::*;
fn assert_no_v2_staged_residue(root: &Path) {
    let staged = fs::read_dir(root)
        .expect("read game root")
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.contains(".renderpilot-v2-") && name.ends_with(".staged"))
        .collect::<Vec<_>>();
    assert!(staged.is_empty(), "staged residue: {staged:?}");
}
fn prepare_raw_v2_row(
    context: &Context,
    game_id: &GameId,
    id: &str,
    transaction_dir: &Path,
    scope_root: &Path,
) {
    let manifest = serde_json::json!({
        "format_version": 2,
        "roots": [scope_root.to_string_lossy().into_owned()],
        "transaction_dir": transaction_dir.to_string_lossy().into_owned(),
        "operations": [],
        "snapshots": [],
    })
    .to_string();
    context
        .storage()
        .begin_file_mutation_preparation(&BeginFileMutationPreparation {
            id: id.to_owned(),
            game_id: game_id.clone(),
            feature: renderpilot_domain::mutation_features::RENODX_DLSS_FIX_UPDATE.to_owned(),
            subject_id: None,
            initial_manifest_json: manifest.clone(),
        })
        .expect("begin raw v2 row");
    context
        .storage()
        .finish_preparing_file_mutation(id, &manifest)
        .expect("finish raw v2 row");
}

fn transaction_dir_for_pending_row(context: &Context, id: &str) -> PathBuf {
    let row = context
        .storage()
        .get_pending_file_mutation(id)
        .expect("read pending row")
        .expect("pending row");
    let manifest: serde_json::Value =
        serde_json::from_str(&row.manifest_json).expect("manifest JSON");
    PathBuf::from(
        manifest["transaction_dir"]
            .as_str()
            .expect("transaction directory"),
    )
}
#[test]
fn v2_sync_failure_restores_only_its_exact_postimage() {
    let root = tempfile::tempdir().expect("game");
    let context = Context::open_at(root.path().join("catalog.db")).expect("context");
    let game_id = GameId::new("manual:v2-rollback").expect("id");
    let guard = crate::game_mutation_lock::try_lock(&game_id).expect("guard");
    let target = root.path().join("renodx-dlssfix.addon64");
    fs::write(&target, b"before").expect("seed");
    let expected = observe(&target);
    let mutation = RetryableFileMutationV2::prepare(
        &context,
        &guard,
        &scope(root.path()),
        renderpilot_domain::mutation_features::RENODX_DLSS_FIX_UPDATE,
        None,
        &RetryableFilePlan {
            operations: vec![RetryableFileOperation::Write {
                path: target.clone(),
                bytes: b"after".to_vec(),
                expected,
            }],
        },
    )
    .expect("prepared");
    let error = mutation
        .commit_or_rollback(&context, |_| Err::<(), _>(crate::failed("database failed")))
        .expect_err("failure");
    assert!(error.to_string().contains("database failed"));
    assert_eq!(fs::read(&target).expect("restored"), b"before");
}

#[test]
fn v2_sync_rollback_preserves_a_foreign_postwrite_replacement() {
    let root = tempfile::tempdir().expect("game");
    let context = Context::open_at(root.path().join("catalog.db")).expect("context");
    let game_id = GameId::new("manual:v2-conflict").expect("id");
    let guard = crate::game_mutation_lock::try_lock(&game_id).expect("guard");
    let target = root.path().join("renodx-dlssfix.addon64");
    fs::write(&target, b"before").expect("seed");
    let mutation = RetryableFileMutationV2::prepare(
        &context,
        &guard,
        &scope(root.path()),
        renderpilot_domain::mutation_features::RENODX_DLSS_FIX_UPDATE,
        None,
        &RetryableFilePlan {
            operations: vec![RetryableFileOperation::Write {
                path: target.clone(),
                bytes: b"renderpilot".to_vec(),
                expected: observe(&target),
            }],
        },
    )
    .expect("prepared");
    let error = mutation
        .commit_or_rollback(&context, |_| {
            fs::write(&target, b"foreign").expect("external edit");
            Err::<(), _>(crate::failed("database failed"))
        })
        .expect_err("rollback conflict");
    assert!(error.to_string().contains("rollback"));
    assert_eq!(fs::read(&target).expect("foreign survives"), b"foreign");
}

#[test]
fn v2_prepared_recovery_keeps_applied_postimages_and_a_retry_converges() {
    let root = tempfile::tempdir().expect("game");
    let context = Context::open_at(root.path().join("catalog.db")).expect("context");
    let game_id = GameId::new("manual:v2-after-write").expect("id");
    let guard = crate::game_mutation_lock::try_lock(&game_id).expect("guard");
    let target = root.path().join("renodx-dlssfix.addon64");
    let mut mutation = RetryableFileMutationV2::prepare(
        &context,
        &guard,
        &scope(root.path()),
        renderpilot_domain::mutation_features::RENODX_DLSS_FIX_INSTALL,
        None,
        &RetryableFilePlan {
            operations: vec![RetryableFileOperation::Write {
                path: target.clone(),
                bytes: b"payload".to_vec(),
                expected: V2DiskObservation::Absent,
            }],
        },
    )
    .expect("prepared");
    mutation.apply().expect("first target write");
    drop(mutation); // crash after a write but before persistence/cleanup

    recover_pending(&context, &guard).expect("cleanup-only recovery");
    assert_eq!(fs::read(&target).expect("postimage remains"), b"payload");

    RetryableFileMutationV2::prepare(
        &context,
        &guard,
        &scope(root.path()),
        renderpilot_domain::mutation_features::RENODX_DLSS_FIX_UPDATE,
        None,
        &RetryableFilePlan {
            operations: vec![RetryableFileOperation::Write {
                path: target.clone(),
                bytes: b"payload".to_vec(),
                expected: observe(&target),
            }],
        },
    )
    .expect("retry prepared")
    .commit_or_rollback(&context, |mutation_id| {
        commit_empty_mutation(&context, &game_id, mutation_id)
    })
    .expect("idempotent retry");
    assert_eq!(
        fs::read(target).expect("unchanged retry payload"),
        b"payload"
    );
}

#[test]
fn v2_delete_rolls_back_only_while_the_target_stays_absent() {
    let root = tempfile::tempdir().expect("game");
    let context = Context::open_at(root.path().join("catalog.db")).expect("context");
    let game_id = GameId::new("manual:v2-delete").expect("id");
    let guard = crate::game_mutation_lock::try_lock(&game_id).expect("guard");
    let target = root.path().join("renodx-dlssfix.addon64");
    fs::write(&target, b"before-delete").expect("seed");
    let mutation = RetryableFileMutationV2::prepare(
        &context,
        &guard,
        &scope(root.path()),
        renderpilot_domain::mutation_features::RENODX_DLSS_FIX_UNINSTALL,
        None,
        &RetryableFilePlan {
            operations: vec![RetryableFileOperation::Delete {
                path: target.clone(),
                expected: observe(&target),
            }],
        },
    )
    .expect("prepared");
    let error = mutation
        .commit_or_rollback(&context, |_| Err::<(), _>(crate::failed("database failed")))
        .expect_err("rollback after delete");
    assert!(error.to_string().contains("database failed"));
    assert_eq!(fs::read(target).expect("restored delete"), b"before-delete");
}

#[test]
fn v2_token_drift_aborts_before_a_write_and_preserves_the_foreign_file() {
    let root = tempfile::tempdir().expect("game");
    let context = Context::open_at(root.path().join("catalog.db")).expect("context");
    let game_id = GameId::new("manual:v2-token-drift").expect("id");
    let guard = crate::game_mutation_lock::try_lock(&game_id).expect("guard");
    let target = root.path().join("renodx-dlssfix.addon64");
    fs::write(&target, b"before").expect("seed");
    let mutation = RetryableFileMutationV2::prepare(
        &context,
        &guard,
        &scope(root.path()),
        renderpilot_domain::mutation_features::RENODX_DLSS_FIX_UPDATE,
        None,
        &RetryableFilePlan {
            operations: vec![RetryableFileOperation::Write {
                path: target.clone(),
                bytes: b"renderpilot".to_vec(),
                expected: observe(&target),
            }],
        },
    )
    .expect("prepared");
    fs::write(&target, b"foreign").expect("external change before final token");

    let error = mutation
        .commit_or_rollback(&context, |_| Ok::<(), ServiceError>(()))
        .expect_err("token drift");
    assert!(error.to_string().contains("changed before apply"));
    assert_eq!(fs::read(target).expect("foreign survives"), b"foreign");
}

#[test]
fn v2_corrupted_prepared_payload_aborts_before_target_write() {
    let root = tempfile::tempdir().expect("game");
    let context = Context::open_at(root.path().join("catalog.db")).expect("context");
    let game_id = GameId::new("manual:v2-corrupted-payload").expect("id");
    let guard = crate::game_mutation_lock::try_lock(&game_id).expect("guard");
    let target = root.path().join("renodx-dlssfix.addon64");
    fs::write(&target, b"before").expect("seed");
    let mut mutation = RetryableFileMutationV2::prepare(
        &context,
        &guard,
        &scope(root.path()),
        renderpilot_domain::mutation_features::RENODX_DLSS_FIX_UPDATE,
        None,
        &RetryableFilePlan {
            operations: vec![RetryableFileOperation::Write {
                path: target.clone(),
                bytes: b"after".to_vec(),
                expected: observe(&target),
            }],
        },
    )
    .expect("prepared");
    let mutation_id = mutation.id().to_owned();
    fs::write(
        transaction_dir_for_pending_row(&context, &mutation_id).join("0.payload"),
        b"corrupted-payload",
    )
    .expect("corrupt immutable payload");

    let error = mutation.apply().expect_err("corrupted payload must fail");
    assert!(error.to_string().contains("payload digest"));
    assert_eq!(fs::read(&target).expect("live target"), b"before");
    assert_eq!(
        context
            .storage()
            .get_pending_file_mutation(&mutation_id)
            .expect("row")
            .expect("prepared row")
            .state,
        renderpilot_storage_sqlite::PendingFileMutationState::Prepared
    );
}

#[test]
fn v2_corrupted_preimage_refuses_restore_and_retains_pending_fence() {
    let root = tempfile::tempdir().expect("game");
    let context = Context::open_at(root.path().join("catalog.db")).expect("context");
    let game_id = GameId::new("manual:v2-corrupted-preimage").expect("id");
    store_game(&context, game_id.clone(), root.path());
    let guard = crate::game_mutation_lock::try_lock(&game_id).expect("guard");
    let target = root.path().join("renodx-dlssfix.addon64");
    fs::write(&target, b"before").expect("seed");
    let mutation = RetryableFileMutationV2::prepare(
        &context,
        &guard,
        &scope(root.path()),
        renderpilot_domain::mutation_features::RENODX_DLSS_FIX_UPDATE,
        None,
        &RetryableFilePlan {
            operations: vec![RetryableFileOperation::Write {
                path: target.clone(),
                bytes: b"after".to_vec(),
                expected: observe(&target),
            }],
        },
    )
    .expect("prepared");
    let mutation_id = mutation.id().to_owned();
    fs::write(
        transaction_dir_for_pending_row(&context, &mutation_id).join("0.before"),
        b"corrupted-preimage",
    )
    .expect("corrupt immutable preimage");

    let error = mutation
        .commit_or_rollback(&context, |_| Err::<(), _>(crate::failed("database failed")))
        .expect_err("corrupted preimage must block restore");
    assert!(error.to_string().contains("preimage snapshot"));
    assert_eq!(fs::read(&target).expect("postimage retained"), b"after");
    assert_eq!(
        context
            .storage()
            .get_pending_file_mutation(&mutation_id)
            .expect("row")
            .expect("prepared row")
            .state,
        renderpilot_storage_sqlite::PendingFileMutationState::Prepared
    );
    assert_eq!(
        context
            .storage()
            .catalog_readiness(&game_id)
            .expect("readiness"),
        CatalogReadiness::Invalidated {
            authority_epoch: 1,
            reason: "prepared_file_mutation".to_owned(),
            mutation_token: Some(mutation_id),
        }
    );
}

#[test]
fn v2_absent_write_failure_never_exposes_a_partial_live_payload() {
    let root = tempfile::tempdir().expect("game");
    let context = Context::open_at(root.path().join("catalog.db")).expect("context");
    let game_id = GameId::new("manual:v2-absent-publish-failure").expect("id");
    let guard = crate::game_mutation_lock::try_lock(&game_id).expect("guard");
    let target = root.path().join("renodx-dlssfix.addon64");
    let mutation = RetryableFileMutationV2::prepare(
        &context,
        &guard,
        &scope(root.path()),
        renderpilot_domain::mutation_features::RENODX_DLSS_FIX_INSTALL,
        None,
        &RetryableFilePlan {
            operations: vec![RetryableFileOperation::Write {
                path: target.clone(),
                bytes: b"complete-payload-that-must-never-be-partial".to_vec(),
                expected: V2DiskObservation::Absent,
            }],
        },
    )
    .expect("prepared");
    super::retryable_v2::fail_next_absent_publish_for_test(&target);

    let error = mutation
        .commit_or_rollback(&context, |_| Ok::<(), ServiceError>(()))
        .expect_err("injected publish failure");
    assert!(error.to_string().contains("publish failure"));
    assert_eq!(observe(&target), V2DiskObservation::Absent);
    assert!(!target.exists(), "the empty reservation must be cleaned");
}

#[test]
fn v2_reservation_flush_failure_removes_the_owned_empty_target_and_all_artifacts() {
    let root = tempfile::tempdir().expect("game");
    let context = Context::open_at(root.path().join("catalog.db")).expect("context");
    let game_id = GameId::new("manual:v2-reservation-flush-failure").expect("id");
    let guard = crate::game_mutation_lock::try_lock(&game_id).expect("guard");
    let target = root.path().join("renodx-dlssfix.addon64");
    let mutation = RetryableFileMutationV2::prepare(
        &context,
        &guard,
        &scope(root.path()),
        renderpilot_domain::mutation_features::RENODX_DLSS_FIX_INSTALL,
        None,
        &RetryableFilePlan {
            operations: vec![RetryableFileOperation::Write {
                path: target.clone(),
                bytes: b"complete-payload".to_vec(),
                expected: V2DiskObservation::Absent,
            }],
        },
    )
    .expect("prepared");
    super::retryable_v2::fail_next_reservation_flush_for_test(&target);

    let error = mutation
        .commit_or_rollback(&context, |_| Ok::<(), ServiceError>(()))
        .expect_err("injected reservation flush failure");
    assert!(error.to_string().contains("reservation flush failure"));
    assert_eq!(observe(&target), V2DiskObservation::Absent);
    assert!(
        context
            .storage()
            .pending_file_mutations_for_game(&game_id)
            .expect("pending rows")
            .is_empty(),
        "failed reservation must not leave a pending transaction"
    );
    assert_no_v2_staged_residue(root.path());
}

#[test]
fn v2_reservation_drift_preserves_a_foreign_nonempty_target() {
    let root = tempfile::tempdir().expect("game");
    let context = Context::open_at(root.path().join("catalog.db")).expect("context");
    let game_id = GameId::new("manual:v2-reservation-drift").expect("id");
    let guard = crate::game_mutation_lock::try_lock(&game_id).expect("guard");
    let target = root.path().join("renodx-dlssfix.addon64");
    let mutation = RetryableFileMutationV2::prepare(
        &context,
        &guard,
        &scope(root.path()),
        renderpilot_domain::mutation_features::RENODX_DLSS_FIX_INSTALL,
        None,
        &RetryableFilePlan {
            operations: vec![RetryableFileOperation::Write {
                path: target.clone(),
                bytes: b"renderpilot-payload".to_vec(),
                expected: V2DiskObservation::Absent,
            }],
        },
    )
    .expect("prepared");
    super::retryable_v2::drift_next_absent_reservation_for_test(&target);

    let error = mutation
        .commit_or_rollback(&context, |_| Ok::<(), ServiceError>(()))
        .expect_err("injected reservation drift failure");
    assert!(error.to_string().contains("reservation drift failure"));
    assert_eq!(
        fs::read(&target).expect("foreign reservation remains"),
        b"foreign-reservation"
    );
    assert!(
        context
            .storage()
            .pending_file_mutations_for_game(&game_id)
            .expect("pending rows")
            .is_empty(),
        "foreign drift must not leave a pending transaction"
    );
    assert_no_v2_staged_residue(root.path());
}

#[test]
fn v2_rollback_continues_after_a_restore_error_and_reverses_other_applied_operations() {
    let root = tempfile::tempdir().expect("game");
    let context = Context::open_at(root.path().join("catalog.db")).expect("context");
    let game_id = GameId::new("manual:v2-rollback-all").expect("id");
    let guard = crate::game_mutation_lock::try_lock(&game_id).expect("guard");
    let first = root.path().join("first.addon64");
    let second = root.path().join("second.addon64");
    fs::write(&first, b"first-before").expect("first seed");
    fs::write(&second, b"second-before").expect("second seed");
    let mutation = RetryableFileMutationV2::prepare(
        &context,
        &guard,
        &scope(root.path()),
        renderpilot_domain::mutation_features::RENODX_DLSS_FIX_UPDATE,
        None,
        &RetryableFilePlan {
            operations: vec![
                RetryableFileOperation::Write {
                    path: first.clone(),
                    bytes: b"first-after".to_vec(),
                    expected: observe(&first),
                },
                RetryableFileOperation::Write {
                    path: second.clone(),
                    bytes: b"second-after".to_vec(),
                    expected: observe(&second),
                },
            ],
        },
    )
    .expect("prepared");
    super::retryable_v2::fail_next_restore_snapshot_for_test(&second);
    let error = mutation
        .commit_or_rollback(&context, |_| Err::<(), _>(crate::failed("database failed")))
        .expect_err("rollback restore error");

    assert!(error.to_string().contains("rollback"));
    assert!(error.to_string().contains("restore snapshot failure"));
    assert_eq!(
        fs::read(&first).expect("first safe reversal"),
        b"first-before"
    );
    assert_eq!(
        fs::read(&second).expect("failed restore keeps postimage"),
        b"second-after"
    );
}

#[test]
fn v2_preimage_mismatch_aborts_before_manifest_publication() {
    let root = tempfile::tempdir().expect("game");
    let context = Context::open_at(root.path().join("catalog.db")).expect("context");
    let game_id = GameId::new("manual:v2-preimage-mismatch").expect("id");
    let guard = crate::game_mutation_lock::try_lock(&game_id).expect("guard");
    let target = root.path().join("renodx-dlssfix.addon64");
    fs::write(&target, b"before").expect("seed");
    super::retryable_v2::corrupt_next_preimage_snapshot_for_test(&target);

    let result = RetryableFileMutationV2::prepare(
        &context,
        &guard,
        &scope(root.path()),
        renderpilot_domain::mutation_features::RENODX_DLSS_FIX_UPDATE,
        None,
        &RetryableFilePlan {
            operations: vec![RetryableFileOperation::Write {
                path: target.clone(),
                bytes: b"after".to_vec(),
                expected: observe(&target),
            }],
        },
    );
    let error = match result {
        Ok(_) => panic!("mismatched preimage must fail preparation"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("preimage snapshot"));
    assert_eq!(fs::read(&target).expect("live file untouched"), b"before");
    assert!(
        context
            .storage()
            .pending_file_mutations_for_game(&game_id)
            .expect("pending rows")
            .is_empty()
    );
}

#[cfg(windows)]
#[test]
fn v2_cleanup_recovery_tolerates_an_unreachable_game_root() {
    let root = tempfile::tempdir().expect("data root");
    let context = Context::open_at(root.path().join("catalog.db")).expect("context");
    let game_id = GameId::new("manual:v2-unreachable-root").expect("id");
    let guard = crate::game_mutation_lock::try_lock(&game_id).expect("guard");
    let id = "v2-unreachable-root";
    let transaction_dir = context.file_mutation_root().join(id);
    fs::create_dir_all(&transaction_dir).expect("transaction directory");
    fs::write(transaction_dir.join("artifact"), b"cleanup-only").expect("artifact");
    prepare_raw_v2_row(
        &context,
        &game_id,
        id,
        &transaction_dir,
        &unreachable_game_root(),
    );

    recover_pending(&context, &guard).expect("cleanup-only V2 recovery");
    assert!(!transaction_dir.exists());
    assert!(
        context
            .storage()
            .pending_file_mutations_for_game(&game_id)
            .expect("pending rows")
            .is_empty()
    );
}

#[test]
fn v2_recovery_rejects_root_and_sibling_transaction_directories() {
    for (suffix, target_root) in [("root", true), ("sibling", false)] {
        let root = tempfile::tempdir().expect("game");
        let context = Context::open_at(root.path().join("catalog.db")).expect("context");
        let game_id = GameId::new(format!("manual:v2-owner-{suffix}")).expect("id");
        let guard = crate::game_mutation_lock::try_lock(&game_id).expect("guard");
        let victim = context.file_mutation_root().join("victim");
        fs::create_dir_all(&victim).expect("victim directory");
        let sentinel = victim.join("sentinel.payload");
        fs::write(&sentinel, b"keep").expect("sentinel");
        let declared = if target_root {
            context.file_mutation_root().to_path_buf()
        } else {
            victim
        };
        let row_id = format!("attacker-{suffix}");
        prepare_raw_v2_row(&context, &game_id, &row_id, &declared, root.path());

        let error = recover_pending(&context, &guard).expect_err("foreign directory must fail");
        assert!(error.to_string().contains("durable row id"));
        assert_eq!(fs::read(&sentinel).expect("victim retained"), b"keep");
        assert!(
            context
                .storage()
                .get_pending_file_mutation(&row_id)
                .expect("row lookup")
                .is_some(),
            "malformed row must remain for diagnosis"
        );
    }
}
