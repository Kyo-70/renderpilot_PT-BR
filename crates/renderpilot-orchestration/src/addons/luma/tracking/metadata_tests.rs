use std::sync::Arc;
use std::time::Duration;

use renderpilot_application::{GameRepository, InstalledAddonRepository};
use renderpilot_domain::{
    AddonKind, GameId, GameIdentity, GameInstallation, GameRuntime, InstalledAddon, Launcher,
    PathRef, Platform, TrackedSource, TrackedSourceRole,
};
use tempfile::tempdir;

use crate::Context;
use crate::addons::luma::fetch::types::LumaPayload;

fn source_record(game_id: GameId) -> InstalledAddon {
    InstalledAddon::new(
        game_id,
        AddonKind::Luma,
        PathRef::new("C:/Games/Luma.addon").expect("path"),
    )
    .with_tracked_source(TrackedSource::new(
        TrackedSourceRole::AddonPayload,
        "https://example.invalid/luma.zip",
        Some("old-etag".to_owned()),
        "zip-digest",
    ))
}

fn payload() -> LumaPayload {
    LumaPayload {
        files: Vec::new(),
        main_addon_rel: "Luma.addon".to_owned(),
        zip_digest: "zip-digest".to_owned(),
        etag: Some("new-etag".to_owned()),
        last_modified: Some("Wed, 01 Jan 2025 00:00:00 GMT".to_owned()),
        build_number: Some(2),
    }
}

#[tokio::test]
async fn metadata_consumer_waits_for_the_same_game_guard_before_persisting() {
    let database = tempdir().expect("database");
    let context =
        Arc::new(Context::open_at(database.path().join("catalog.sqlite")).expect("context"));
    let game_id =
        GameId::new(format!("manual:luma-lock:{}", ulid::Ulid::generate())).expect("game id");
    let record = source_record(game_id.clone());
    context
        .storage()
        .upsert_game(&GameInstallation::new(
            GameIdentity::new(game_id.clone(), "Luma tracking", Launcher::Manual)
                .expect("identity"),
            Platform::Windows,
            GameRuntime::NativeWindows,
            PathRef::new("C:/Games").expect("game root"),
        ))
        .expect("seed game");
    context
        .storage()
        .upsert_installed_addon(&record)
        .expect("seed record");

    let held = crate::game_mutation_lock::try_lock(&game_id).expect("held game guard");
    let (attempt_tx, attempt_rx) = std::sync::mpsc::channel();
    crate::game_mutation_lock::set_lock_attempt_hook(&game_id, attempt_tx);

    let worker_context = Arc::clone(&context);
    let worker_game = game_id.clone();
    let worker = tokio::spawn(async move {
        super::try_refresh_payload_validators(
            &worker_context,
            &worker_game,
            "zip-digest",
            &payload(),
        )
        .await;
    });
    tokio::task::spawn_blocking(move || {
        attempt_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("refresh attempted to acquire the game guard");
    })
    .await
    .expect("lock-attempt observer");
    assert!(
        !worker.is_finished(),
        "consumer must remain behind the guard"
    );

    drop(held);
    worker.await.expect("refresh task");

    let stored = context
        .storage()
        .get_installed_addon(&game_id)
        .expect("load record")
        .expect("record exists");
    let source = stored.tracked_sources().first().expect("payload source");
    assert_eq!(source.etag(), Some("new-etag"));
    assert_eq!(stored.addon_version(), Some("Build 2"));
}
