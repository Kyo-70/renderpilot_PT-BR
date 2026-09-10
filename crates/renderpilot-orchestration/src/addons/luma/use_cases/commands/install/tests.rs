#[cfg(windows)]
use std::path::Path;

#[cfg(windows)]
use super::*;
#[cfg(windows)]
use crate::addons::engine;
#[cfg(windows)]
use crate::addons::luma::test_support::{
    MACHINE_AMD64, PE32_PLUS_MAGIC, build_pe_with_exports, manifest,
};
#[cfg(windows)]
use crate::addons::records;
#[cfg(windows)]
use renderpilot_application::{GameRepository, InstalledAddonRepository};
#[cfg(windows)]
use renderpilot_domain::{
    AddonKind, GameId, GameIdentity, GameInstallation, GameRuntime, InstalledAddon, Launcher,
    PathRef, Platform,
};
#[cfg(windows)]
use tempfile::tempdir;

#[cfg(windows)]
fn seed_game(context: &Context, game_id: &GameId, appid: &str, game_dir: &Path, exe_path: &Path) {
    let identity = GameIdentity::new(game_id.clone(), "Dishonored 2", Launcher::Steam)
        .expect("identity")
        .with_external_id(appid)
        .expect("external id");
    let game = GameInstallation::new(
        identity,
        Platform::Windows,
        GameRuntime::NativeWindows,
        PathRef::new(game_dir.to_string_lossy().replace('\\', "/")).expect("install path"),
    )
    .with_executable_candidate(
        PathRef::new(exe_path.to_string_lossy().replace('\\', "/")).expect("exe path"),
    );
    context.storage().upsert_game(&game).expect("seed game");
}

#[cfg(windows)]
fn write_stub_exe(path: &Path) {
    std::fs::write(
        path,
        build_pe_with_exports(MACHINE_AMD64, PE32_PLUS_MAGIC, &[]),
    )
    .expect("write exe");
}

#[cfg(windows)]
fn game_safety(context: &Context, game_id: &GameId) -> crate::GameSafetyPermit {
    let authority = crate::FileSafetyAuthority::new();
    let assessment = authority
        .issue_game_assessment(context, game_id)
        .expect("assessment");
    authority
        .game_permit(game_id.clone(), Some(&assessment.context_token))
        .expect("permit")
}

#[test]
#[cfg(windows)]
fn install_safety_boundary_rejects_missing_stale_and_scope_mismatched_permits_before_writes() {
    let db_dir = tempdir().expect("db root");
    let game_dir = tempdir().expect("game root");
    let context = Context::open_at(db_dir.path().join("catalog.sqlite")).expect("context");
    let game_id = GameId::new("steam:luma-install-safety").expect("game id");
    let exe_path = game_dir.path().join("Dishonored2.exe");
    write_stub_exe(&exe_path);
    seed_game(
        &context,
        &game_id,
        "luma-install-safety",
        game_dir.path(),
        &exe_path,
    );
    let target = game_dir.path().join("Luma-Game.addon");

    let authority = crate::FileSafetyAuthority::new();
    let missing = authority
        .game_permit(game_id.clone(), None)
        .expect_err("missing permit must reject before install writes");
    assert!(matches!(missing, ServiceError::SafetyContextMissing { .. }));
    assert!(!target.exists());
    assert!(
        context
            .storage()
            .pending_file_mutations_for_game(&game_id)
            .expect("pending rows")
            .is_empty()
    );

    let assessment = authority
        .issue_game_assessment(&context, &game_id)
        .expect("assessment");
    let permit = authority
        .game_permit(game_id.clone(), Some(&assessment.context_token))
        .expect("permit");
    std::fs::write(game_dir.path().join("EasyAntiCheat"), b"detected marker").expect("marker");
    let guard = crate::game_mutation_lock::try_lock(&game_id).expect("lock");
    let stale = authority
        .authorize_game_commit(
            &context,
            crate::addons::mutation_features::LUMA_INSTALL,
            &guard,
            &permit,
            || -> Result<(), ServiceError> { panic!("stale permit entered commit") },
        )
        .expect_err("stale permit must reject before install writes");
    assert!(matches!(stale, ServiceError::SafetyContextStale { .. }));
    assert!(!target.exists());
    drop(guard);

    let other_db_game = tempdir().expect("other game root");
    let other_id = GameId::new("steam:luma-install-other-safety").expect("other game id");
    let other_exe = other_db_game.path().join("Dishonored2.exe");
    write_stub_exe(&other_exe);
    seed_game(
        &context,
        &other_id,
        "luma-install-other-safety",
        other_db_game.path(),
        &other_exe,
    );
    let other_assessment = authority
        .issue_game_assessment(&context, &other_id)
        .expect("other assessment");
    let mismatched = authority
        .game_permit(game_id.clone(), Some(&other_assessment.context_token))
        .expect("well-formed permit");
    let guard = crate::game_mutation_lock::try_lock(&game_id).expect("lock");
    let scope = authority
        .authorize_game_commit(
            &context,
            crate::addons::mutation_features::LUMA_INSTALL,
            &guard,
            &mismatched,
            || -> Result<(), ServiceError> { panic!("mismatched permit entered commit") },
        )
        .expect_err("scope-mismatched permit must reject before install writes");
    assert!(matches!(
        scope,
        ServiceError::SafetyContextScopeMismatch { .. }
    ));
    assert!(!target.exists());
}

#[tokio::test]
#[cfg(windows)]
async fn install_refuses_before_any_network_when_renodx_is_installed() {
    let db_dir = tempdir().expect("db dir");
    let game_dir = tempdir().expect("game dir");
    let context = Context::open_at(db_dir.path().join("catalog.sqlite")).expect("context");
    let game_id = GameId::new("steam:403640").expect("game id");
    let exe_path = game_dir.path().join("Dishonored2.exe");
    write_stub_exe(&exe_path);
    seed_game(&context, &game_id, "403640", game_dir.path(), &exe_path);
    let renodx_record = InstalledAddon::new(
        game_id.clone(),
        AddonKind::RenoDx,
        PathRef::new(
            game_dir
                .path()
                .join("renodx-test.addon64")
                .to_string_lossy()
                .replace('\\', "/"),
        )
        .expect("path"),
    );
    context
        .storage()
        .upsert_installed_addon(&renodx_record)
        .expect("seed renodx record");

    let error = install(InstallRequest {
        context: &context,
        manifest: &manifest(Vec::new()),
        reshade_sources: &crate::addons::luma::test_support::reshade_sources(),
        game_id: &game_id,
        safety: game_safety(&context, &game_id),
        progress: None,
    })
    .await
    .expect_err("must refuse while RenoDX is installed");
    assert!(matches!(error, ServiceError::InvalidInput(_)));
    assert!(
        records::record_of_kind(&context, &game_id, AddonKind::Luma)
            .expect("query")
            .is_none()
    );
}

#[tokio::test]
#[cfg(windows)]
async fn install_refuses_before_any_network_when_luma_files_are_unmanaged_on_disk() {
    let db_dir = tempdir().expect("db dir");
    let game_dir = tempdir().expect("game dir");
    let context = Context::open_at(db_dir.path().join("catalog.sqlite")).expect("context");
    let game_id = GameId::new("steam:403641").expect("game id");
    let exe_path = game_dir.path().join("Dishonored2.exe");
    write_stub_exe(&exe_path);
    seed_game(&context, &game_id, "403641", game_dir.path(), &exe_path);
    std::fs::write(game_dir.path().join("Luma-Dishonored_2.addon"), b"x")
        .expect("write unmanaged addon");

    let error = install(InstallRequest {
        context: &context,
        manifest: &manifest(Vec::new()),
        reshade_sources: &crate::addons::luma::test_support::reshade_sources(),
        game_id: &game_id,
        safety: game_safety(&context, &game_id),
        progress: None,
    })
    .await
    .expect_err("must refuse over unmanaged Luma files");
    assert!(matches!(error, ServiceError::InvalidInput(_)));
    assert!(
        records::record_of_kind(&context, &game_id, AddonKind::Luma)
            .expect("query")
            .is_none()
    );
}

#[tokio::test]
#[cfg(windows)]
async fn install_recovers_from_a_torn_install_and_proceeds_past_the_unmanaged_gate() {
    let db_dir = tempdir().expect("db dir");
    let game_dir = tempdir().expect("game dir");
    let context = Context::open_at(db_dir.path().join("catalog.sqlite")).expect("context");
    let game_id = GameId::new("steam:403642").expect("game id");
    let exe_path = game_dir.path().join("Dishonored2.exe");
    write_stub_exe(&exe_path);
    seed_game(&context, &game_id, "403642", game_dir.path(), &exe_path);
    std::fs::write(
        game_dir.path().join("Luma-Dishonored_2.addon"),
        b"half-written",
    )
    .expect("write torn debris");
    std::fs::write(game_dir.path().join("renderpilot-luma-install.lock"), b"")
        .expect("write sentinel");

    let error = install(InstallRequest {
        context: &context,
        manifest: &manifest(Vec::new()),
        reshade_sources: &crate::addons::luma::test_support::reshade_sources(),
        game_id: &game_id,
        safety: game_safety(&context, &game_id),
        progress: None,
    })
    .await
    .expect_err("empty manifest has no matching profile");
    match error {
        ServiceError::InvalidInput(message) => assert!(!message.contains("found on disk")),
        other => panic!("expected InvalidInput, got {other:?}"),
    }
    assert!(!game_dir.path().join("Luma-Dishonored_2.addon").exists());
    assert!(!engine::is_install_torn(game_dir.path(), AddonKind::Luma));
}

#[tokio::test]
#[cfg(windows)]
async fn install_still_refuses_when_renodx_blocks_a_torn_install() {
    let db_dir = tempdir().expect("db dir");
    let game_dir = tempdir().expect("game dir");
    let context = Context::open_at(db_dir.path().join("catalog.sqlite")).expect("context");
    let game_id = GameId::new("steam:403643").expect("game id");
    let exe_path = game_dir.path().join("Dishonored2.exe");
    write_stub_exe(&exe_path);
    seed_game(&context, &game_id, "403643", game_dir.path(), &exe_path);
    std::fs::write(game_dir.path().join("renderpilot-luma-install.lock"), b"")
        .expect("write sentinel");
    let renodx_record = InstalledAddon::new(
        game_id.clone(),
        AddonKind::RenoDx,
        PathRef::new(
            game_dir
                .path()
                .join("renodx-test.addon64")
                .to_string_lossy()
                .replace('\\', "/"),
        )
        .expect("path"),
    );
    context
        .storage()
        .upsert_installed_addon(&renodx_record)
        .expect("seed renodx record");

    let error = install(InstallRequest {
        context: &context,
        manifest: &manifest(Vec::new()),
        reshade_sources: &crate::addons::luma::test_support::reshade_sources(),
        game_id: &game_id,
        safety: game_safety(&context, &game_id),
        progress: None,
    })
    .await
    .expect_err("must refuse while RenoDX is installed");
    assert!(matches!(error, ServiceError::InvalidInput(_)));
    assert!(engine::is_install_torn(game_dir.path(), AddonKind::Luma));
}
