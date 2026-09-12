use super::*;

#[test]
fn legacy_prepared_dlss_features_clean_rows_without_restoring_live_files() {
    let root = tempfile::tempdir().expect("game");
    let context = Context::open_at(root.path().join("catalog.db")).expect("context");
    let game_id = GameId::new("manual:legacy-dlss").expect("id");
    let guard = crate::game_mutation_lock::try_lock(&game_id).expect("guard");
    for feature in [
        renderpilot_domain::mutation_features::RENODX_DLSS_FIX_INSTALL,
        renderpilot_domain::mutation_features::RENODX_DLSS_FIX_UPDATE,
        renderpilot_domain::mutation_features::RENODX_DLSS_FIX_UNINSTALL,
        renderpilot_domain::mutation_features::RENODX_UPDATE,
    ] {
        let addon = root.path().join("renodx-game.addon64");
        let companion = root.path().join("renodx-dlssfix.addon64");
        let ini = root.path().join("ReShade.ini");
        fs::write(&addon, b"before-addon").expect("addon");
        fs::write(&companion, b"before-companion").expect("companion");
        fs::write(&ini, b"before-ini").expect("ini");
        let mutation = DurableFileTransaction::prepare(
            &context,
            &guard,
            &scope(root.path()),
            feature,
            None,
            [addon.clone(), companion.clone(), ini.clone()],
        )
        .expect("legacy prepared");
        drop(mutation);
        fs::write(&addon, b"foreign-addon").expect("edit");
        fs::write(&companion, b"foreign-companion").expect("edit");
        fs::write(&ini, b"foreign-ini").expect("edit");

        recover_pending(&context, &guard).expect("non-destructive legacy cleanup");
        assert_eq!(fs::read(&addon).unwrap(), b"foreign-addon");
        assert_eq!(fs::read(&companion).unwrap(), b"foreign-companion");
        assert_eq!(fs::read(&ini).unwrap(), b"foreign-ini");
    }
}
#[cfg(windows)]
#[test]
fn legacy_cleanup_recovery_tolerates_an_unreachable_game_root() {
    let root = tempfile::tempdir().expect("data root");
    let context = Context::open_at(root.path().join("catalog.db")).expect("context");
    let game_id = GameId::new("manual:legacy-unreachable-root").expect("id");
    let guard = crate::game_mutation_lock::try_lock(&game_id).expect("guard");
    let id = "legacy-unreachable-root";
    let transaction_dir = context.file_mutation_root().join(id);
    fs::create_dir_all(&transaction_dir).expect("transaction directory");
    let manifest = serialize_manifest(&FileMutationManifest {
        format_version: MANIFEST_FORMAT_VERSION,
        roots: vec![unreachable_game_root().to_string_lossy().into_owned()],
        transaction_dir: transaction_dir.to_string_lossy().into_owned(),
        snapshots: Vec::new(),
        peer_ancestors: Vec::new(),
    })
    .expect("manifest");
    context
        .storage()
        .begin_file_mutation_preparation(&BeginFileMutationPreparation {
            id: id.to_owned(),
            game_id: game_id.clone(),
            feature: renderpilot_domain::mutation_features::RENODX_DLSS_FIX_UPDATE.to_owned(),
            subject_id: None,
            initial_manifest_json: manifest.clone(),
        })
        .expect("begin");
    context
        .storage()
        .finish_preparing_file_mutation(id, &manifest)
        .expect("prepare");

    recover_pending(&context, &guard).expect("cleanup-only legacy recovery");
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
fn ordinary_v1_prepared_recovery_requires_a_valid_live_scope() {
    let root = tempfile::tempdir().expect("data root");
    let external = tempfile::tempdir().expect("external root");
    let context = Context::open_at(root.path().join("catalog.db")).expect("context");
    let game_id = GameId::new("manual:v1-invalid-scope").expect("id");
    let guard = crate::game_mutation_lock::try_lock(&game_id).expect("guard");
    let id = "v1-invalid-scope";
    let transaction_dir = context.file_mutation_root().join(id);
    fs::create_dir_all(&transaction_dir).expect("transaction directory");
    let manifest = serialize_manifest(&FileMutationManifest {
        format_version: MANIFEST_FORMAT_VERSION,
        roots: vec![root.path().to_string_lossy().into_owned()],
        transaction_dir: transaction_dir.to_string_lossy().into_owned(),
        snapshots: vec![FileBeforeSnapshot {
            path: external
                .path()
                .join("outside.dll")
                .to_string_lossy()
                .into_owned(),
            snapshot: None,
        }],
        peer_ancestors: Vec::new(),
    })
    .expect("manifest");
    context
        .storage()
        .begin_file_mutation_preparation(&BeginFileMutationPreparation {
            id: id.to_owned(),
            game_id,
            feature: "ordinary_v1_restore".to_owned(),
            subject_id: None,
            initial_manifest_json: manifest.clone(),
        })
        .expect("begin");
    context
        .storage()
        .finish_preparing_file_mutation(id, &manifest)
        .expect("prepare");

    let error = recover_pending(&context, &guard).expect_err("invalid scope must stop restore");
    assert!(error.to_string().contains("outside authorized roots"));
    assert_eq!(
        context
            .storage()
            .get_pending_file_mutation(id)
            .expect("row")
            .expect("prepared row")
            .state,
        renderpilot_storage_sqlite::PendingFileMutationState::Prepared
    );
    assert!(transaction_dir.exists());
}

#[test]
fn legacy_dlss_near_prefix_uses_normal_v1_restore_policy() {
    let root = tempfile::tempdir().expect("game");
    let context = Context::open_at(root.path().join("catalog.db")).expect("context");
    let game_id = GameId::new("manual:legacy-dlss-near-prefix").expect("id");
    let guard = crate::game_mutation_lock::try_lock(&game_id).expect("guard");
    let target = root.path().join("renodx-dlssfix.addon64");
    fs::write(&target, b"before").expect("seed");
    let mutation = DurableFileTransaction::prepare(
        &context,
        &guard,
        &scope(root.path()),
        "renodx_dlss_fix_install_extra",
        None,
        [target.clone()],
    )
    .expect("legacy prepared");
    fs::write(&target, b"after").expect("mutate");
    drop(mutation);

    recover_pending(&context, &guard).expect("ordinary v1 recovery");
    assert_eq!(fs::read(target).expect("restored"), b"before");
}

#[test]
fn unknown_manifest_version_fails_closed_before_resolution() {
    let root = tempfile::tempdir().expect("game");
    let context = Context::open_at(root.path().join("catalog.db")).expect("context");
    let game_id = GameId::new("manual:unknown-file-mutation-version").expect("id");
    store_game(&context, game_id.clone(), root.path());
    let guard = crate::game_mutation_lock::try_lock(&game_id).expect("guard");
    let manifest = r#"{"format_version":99,"snapshots":[]}"#;
    context
        .storage()
        .begin_file_mutation_preparation(&BeginFileMutationPreparation {
            id: "unknown-version".to_owned(),
            game_id: game_id.clone(),
            feature: "test".to_owned(),
            subject_id: None,
            initial_manifest_json: manifest.to_owned(),
        })
        .expect("begin");
    context
        .storage()
        .finish_preparing_file_mutation("unknown-version", manifest)
        .expect("finish");

    let error = recover_pending(&context, &guard).expect_err("unsupported version must stop");
    assert!(error.to_string().contains("unsupported"));
    assert_eq!(
        context
            .storage()
            .get_pending_file_mutation("unknown-version")
            .expect("row lookup")
            .expect("row retained")
            .state,
        renderpilot_storage_sqlite::PendingFileMutationState::Prepared
    );
    assert!(matches!(
        context
            .storage()
            .catalog_readiness(&game_id)
            .expect("readiness"),
        CatalogReadiness::Invalidated { .. }
    ));
}

#[cfg(windows)]
#[test]
fn v1_recovery_cleanup_uses_the_validated_directory_not_an_external_alias() {
    use std::os::windows::fs::symlink_dir;

    let root = tempfile::tempdir().expect("game");
    let aliases = tempfile::tempdir().expect("aliases");
    let context = Context::open_at(root.path().join("catalog.db")).expect("context");
    let game_id = GameId::new("manual:v1-cleanup-alias").expect("id");
    let guard = crate::game_mutation_lock::try_lock(&game_id).expect("guard");
    let row_id = "owned-v1-directory";
    let owned = context.file_mutation_root().join(row_id);
    fs::create_dir_all(&owned).expect("owned transaction directory");
    fs::write(owned.join("snapshot"), b"owned").expect("owned artifact");
    let alias = aliases.path().join("external-alias");
    if symlink_dir(&owned, &alias).is_err() {
        // Windows may deny symlink creation outside Developer Mode. The
        // canonical-path plumbing is still covered on every platform.
        return;
    }
    let manifest = serde_json::json!({
        "format_version": 1,
        "roots": [root.path().to_string_lossy().into_owned()],
        "transaction_dir": alias.to_string_lossy().into_owned(),
        "snapshots": [],
    })
    .to_string();
    context
        .storage()
        .begin_file_mutation_preparation(&BeginFileMutationPreparation {
            id: row_id.to_owned(),
            game_id,
            feature: renderpilot_domain::mutation_features::RENODX_DLSS_FIX_INSTALL.to_owned(),
            subject_id: None,
            initial_manifest_json: manifest.clone(),
        })
        .expect("begin v1 row");
    context
        .storage()
        .finish_preparing_file_mutation(row_id, &manifest)
        .expect("finish v1 row");

    recover_pending(&context, &guard).expect("recover through validated directory");
    assert!(!owned.exists(), "owned app-private directory is cleaned");
    assert!(
        fs::symlink_metadata(&alias)
            .expect("external alias remains")
            .file_type()
            .is_symlink(),
        "cleanup must not delete the untrusted alias path"
    );
}
