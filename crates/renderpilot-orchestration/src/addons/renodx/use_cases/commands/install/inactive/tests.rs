use std::assert_matches;
use std::fs;

use renderpilot_application::GameRepository;
use renderpilot_domain::{
    Architecture, GameId, GameIdentity, GameInstallation, GameRuntime, Launcher, PathRef, Platform,
};
use tempfile::{TempDir, tempdir};

use crate::addons::reshade::types::ReshadeChannel;
use crate::{Context, ServiceError};

#[test]
fn addon_arch_invariant_rejects_a_bitness_mismatch() {
    assert!(super::local_file::ensure_addon_arch(Architecture::X64, Architecture::X64).is_ok());
    let error = super::local_file::ensure_addon_arch(Architecture::X86, Architecture::X64)
        .expect_err("a 32-bit add-on for a 64-bit host must be rejected");
    assert_matches!(error, ServiceError::InvalidInput(_));
}

#[test]
fn every_install_path_rejects_an_explicit_unavailable_stable_channel() {
    let mut reshade_sources = crate::addons::renodx::test_support::reshade_sources();
    reshade_sources.stable = None;

    let error = super::phase::ensure_requested_channel(&reshade_sources, ReshadeChannel::Stable)
        .expect_err("Stable must not silently remap to Nightly");

    assert_matches!(error, ServiceError::InvalidInput(_));
}

struct SafetyFixture {
    _db_dir: TempDir,
    game_dir: TempDir,
    context: Context,
    game_id: GameId,
}

fn safety_fixture(suffix: &str) -> SafetyFixture {
    let db_dir = tempdir().expect("db dir");
    let game_dir = tempdir().expect("game dir");
    let context = Context::open_at(db_dir.path().join("catalog.sqlite")).expect("context");
    let game_id = GameId::new(format!("manual:install-safety-{suffix}")).expect("game id");
    let game = GameInstallation::new(
        GameIdentity::new(game_id.clone(), "Safety Test Game", Launcher::Manual).expect("identity"),
        Platform::Windows,
        GameRuntime::NativeWindows,
        PathRef::new(game_dir.path().to_string_lossy()).expect("game path"),
    );
    context.storage().upsert_game(&game).expect("game");

    SafetyFixture {
        _db_dir: db_dir,
        game_dir,
        context,
        game_id,
    }
}

async fn assert_install_barrier_rejects(
    fixture: &SafetyFixture,
    safety: crate::GameMutationSafetyPermits,
    expected: fn(&ServiceError) -> bool,
) {
    let guards = crate::mutation_boundary::enter_mutation_boundary_async(
        &fixture.context,
        &fixture.game_id,
        false,
    )
    .await
    .expect("game boundary");
    let mut commit_called = false;
    let error = super::commit::authorize_install_commit(
        &fixture.context,
        crate::addons::mutation_features::RENODX_INSTALL,
        guards,
        &safety,
        |_| {
            commit_called = true;
            Ok(())
        },
    )
    .expect_err("invalid safety must reject the install commit");

    assert!(expected(&error), "unexpected error: {error:?}");
    assert!(!commit_called, "safety rejection must precede first write");
    assert!(
        fixture
            .context
            .storage()
            .pending_file_mutations_for_game(&fixture.game_id)
            .expect("pending mutations")
            .is_empty()
    );
}

#[tokio::test]
async fn install_commit_barrier_rejects_stale_game_context_before_first_write() {
    let fixture = safety_fixture("stale");
    let authority = crate::FileSafetyAuthority::new();
    let assessment = authority
        .issue_game_assessment(&fixture.context, &fixture.game_id)
        .expect("assessment");
    let safety = authority
        .game_mutation_permits(
            fixture.game_id.clone(),
            Some(&assessment.context_token),
            None,
        )
        .expect("permits");
    fs::create_dir(fixture.game_dir.path().join("EasyAntiCheat")).expect("anti-cheat marker");

    assert_install_barrier_rejects(&fixture, safety, |error| {
        matches!(error, ServiceError::SafetyContextStale { .. })
    })
    .await;
}

#[tokio::test]
async fn install_commit_barrier_rejects_another_game_scope_before_first_write() {
    let fixture = safety_fixture("scope");
    let other = safety_fixture("other");
    fixture
        .context
        .storage()
        .upsert_game(
            &other
                .context
                .storage()
                .require_game(&other.game_id)
                .expect("other game"),
        )
        .expect("copy other game");
    let authority = crate::FileSafetyAuthority::new();
    let assessment = authority
        .issue_game_assessment(&fixture.context, &other.game_id)
        .expect("assessment");
    let safety = authority
        .game_mutation_permits(
            fixture.game_id.clone(),
            Some(&assessment.context_token),
            None,
        )
        .expect("well-formed permits");

    assert_install_barrier_rejects(&fixture, safety, |error| {
        matches!(error, ServiceError::SafetyContextScopeMismatch { .. })
    })
    .await;
}
