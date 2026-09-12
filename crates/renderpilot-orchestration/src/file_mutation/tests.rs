use std::fs;
use std::path::{Path, PathBuf};

use renderpilot_application::GameRepository;
pub(super) use renderpilot_domain::{
    AddonKind, FileReceipt, GameId, GameIdentity, GameInstallation, GameProxyTopology, GameRuntime,
    Launcher, OptiScalerAdoptionState, OptiScalerFileCleanup, OptiScalerFileReceipt,
    OptiScalerFileRole, PathRef, Platform, ProxyImplementation, ProxyLink, ProxyRootPrestate,
    Sha256Hash,
};
pub(super) use renderpilot_storage_sqlite::{
    BeginFileMutationPreparation, CatalogReadiness, GameMutationCommit, InstalledAddonMutation,
    OptiScalerAggregateMutation,
};

pub(super) use super::manifest::{
    FileBeforeSnapshot, FileMutationManifest, MANIFEST_FORMAT_VERSION, serialize_manifest,
};
use super::*;
use crate::Context;
use crate::ServiceError;

mod boundary;
mod legacy_recovery;
mod peer_recovery;
mod v2_recovery;

fn scope(root: &Path) -> MutationScope {
    MutationScope::single(root).expect("scope")
}

fn commit_empty_mutation(
    context: &Context,
    game_id: &GameId,
    mutation_id: &str,
) -> Result<(), ServiceError> {
    context
        .storage()
        .commit_game_mutation(GameMutationCommit {
            game_id,
            component_set: None,
            baseline_mutations: &[],
            addon: InstalledAddonMutation::Keep,
            mutation_id: Some(mutation_id),
        })
        .map_err(ServiceError::from)
}

#[test]
fn explicit_snapshot_override_retains_destructive_digest() {
    let root = tempfile::tempdir().expect("root");
    let path = root.path().join("obsolete.dll");
    let expected = renderpilot_domain::Sha256Hash::new("a".repeat(64)).expect("hash");

    let targets = apply_snapshot_overrides(
        [path.clone()],
        [MutationTarget::quarantine(&path, Some(expected.clone()))],
        [path.clone()],
        &[root.path().to_path_buf()],
    )
    .expect("targets");
    let target = targets
        .iter()
        .find(|target| target.path == path)
        .expect("target");

    assert_eq!(target.expected_sha256(), Some(&expected));
}

#[test]
fn explicit_snapshot_override_retains_expected_absence() {
    let root = tempfile::tempdir().expect("root");
    let path = root.path().join("new-runtime.dll");

    let targets = apply_snapshot_overrides(
        [path.clone()],
        [MutationTarget::absent_file(&path)],
        [path.clone()],
        &[root.path().to_path_buf()],
    )
    .expect("targets");
    let target = targets
        .iter()
        .find(|target| target.path == path)
        .expect("target");

    assert!(target.expects_absence());
}

#[test]
fn publication_creates_missing_ancestors_but_verification_does_not() {
    let root = tempfile::tempdir().expect("root");
    let publication = root.path().join("D3D12_Optiscaler").join("D3D12Core.dll");

    let publication_targets = apply_snapshot_overrides(
        [publication.clone()],
        std::iter::empty::<MutationTarget>(),
        [publication.clone()],
        &[root.path().to_path_buf()],
    )
    .expect("publication targets");
    assert_eq!(
        publication_targets
            .iter()
            .map(|target| target.path.clone())
            .collect::<Vec<_>>(),
        vec![root.path().join("D3D12_Optiscaler"), publication.clone(),]
    );
    assert!(publication_targets[0].is_absent_directory());

    let verification_targets = apply_snapshot_overrides(
        [publication.clone()],
        std::iter::empty::<MutationTarget>(),
        std::iter::empty::<PathBuf>(),
        &[root.path().to_path_buf()],
    )
    .expect("verification targets");
    assert_eq!(verification_targets.len(), 1);
    assert_eq!(verification_targets[0].path, publication);
    assert!(!verification_targets[0].is_absent_directory());
}

#[test]
fn publication_orders_shared_and_sibling_missing_ancestors_before_endpoints() {
    let root = tempfile::tempdir().expect("root");
    let left = root.path().join("a").join("b").join("left.dll");
    let right = root.path().join("a").join("c").join("right.dll");

    let targets = apply_snapshot_overrides(
        [left.clone(), right.clone()],
        std::iter::empty::<MutationTarget>(),
        [left.clone(), right.clone()],
        &[root.path().to_path_buf()],
    )
    .expect("publication targets");

    assert_eq!(
        targets
            .iter()
            .map(|target| target.path.clone())
            .collect::<Vec<_>>(),
        vec![
            root.path().join("a"),
            root.path().join("a").join("b"),
            root.path().join("a").join("c"),
            left,
            right,
        ]
    );
    assert!(targets[..3].iter().all(MutationTarget::is_absent_directory));
}

#[test]
fn publication_rejects_a_target_outside_declared_roots_before_parent_inspection() {
    let root = tempfile::tempdir().expect("root");
    let outside = tempfile::tempdir().expect("outside");
    let publication = outside.path().join("runtime").join("file.dll");

    let error = apply_snapshot_overrides(
        [publication.clone()],
        std::iter::empty::<MutationTarget>(),
        [publication],
        &[root.path().to_path_buf()],
    )
    .expect_err("out-of-root publication must fail");

    assert!(
        error
            .to_string()
            .contains("not a strict descendant of a declared root")
    );
}

#[test]
fn publication_rejects_a_declared_root_as_its_target() {
    let root = tempfile::tempdir().expect("root");
    let target = root.path().to_path_buf();

    let error = apply_snapshot_overrides(
        [target.clone()],
        std::iter::empty::<MutationTarget>(),
        [target],
        &[root.path().to_path_buf()],
    )
    .expect_err("a publication root is not an endpoint");

    assert!(
        error
            .to_string()
            .contains("not a strict descendant of a declared root")
    );
}

#[test]
fn publication_rejects_a_missing_declared_root() {
    let parent = tempfile::tempdir().expect("parent");
    let missing_root = parent.path().join("missing-root");
    let publication = missing_root.join("runtime").join("file.dll");

    let error = apply_snapshot_overrides(
        [publication.clone()],
        std::iter::empty::<MutationTarget>(),
        [publication],
        &[missing_root],
    )
    .expect_err("a missing authority root cannot authorize publication");

    assert!(
        error
            .to_string()
            .contains("declared publication root is missing")
    );
}

fn store_game(context: &Context, game_id: GameId, root: &Path) {
    let game = GameInstallation::new(
        GameIdentity::new(game_id, "File mutation test", Launcher::Manual).expect("identity"),
        Platform::Windows,
        GameRuntime::NativeWindows,
        PathRef::new(root.to_string_lossy().replace('\\', "/")).expect("root"),
    );
    context.storage().upsert_game(&game).expect("store game");
}

#[cfg(windows)]
fn unreachable_game_root() -> PathBuf {
    for drive in (b'D'..=b'Z').map(char::from) {
        let volume = PathBuf::from(format!("{drive}:\\"));
        if !volume.exists() {
            return volume.join("renderpilot-unreachable-game");
        }
    }
    panic!("the test host has no unavailable drive letter")
}

#[test]
fn prepared_recovery_restores_existing_and_removes_created_files() {
    let root = tempfile::tempdir().expect("game");
    let context = Context::open_at(root.path().join("catalog.db")).expect("context");
    let game_id = GameId::new("manual:test").expect("id");
    store_game(&context, game_id.clone(), root.path());
    let guard = crate::game_mutation_lock::try_lock(&game_id).expect("guard");
    let existing = root.path().join("original.dll");
    let created = root.path().join("created.dll");
    fs::write(&existing, b"before").expect("seed");

    let mutation = DurableFileTransaction::prepare(
        &context,
        &guard,
        &scope(root.path()),
        "test",
        None,
        [existing.clone(), created.clone()],
    )
    .expect("prepare");
    assert_eq!(
        context
            .storage()
            .catalog_readiness(&game_id)
            .expect("readiness"),
        CatalogReadiness::Invalidated {
            authority_epoch: 1,
            reason: "prepared_file_mutation".to_owned(),
            mutation_token: Some(mutation.id().to_owned()),
        },
        "the prepared durable marker must invalidate before the caller's first filesystem write"
    );
    fs::write(&existing, b"after").expect("mutate");
    fs::write(&created, b"new").expect("create");
    drop(mutation);

    recover_pending(&context, &guard).expect("recover");
    assert_eq!(fs::read(existing).expect("read"), b"before");
    assert!(!created.exists());
    recover_pending(&context, &guard).expect("idempotent recovery");
}

#[test]
fn durable_prepare_rechecks_no_follow_preimages_before_forward_work() {
    let root = tempfile::tempdir().expect("game");
    let context = Context::open_at(root.path().join("catalog.db")).expect("context");
    let game_id = GameId::new("manual:o3-preimage").expect("id");
    store_game(&context, game_id.clone(), root.path());
    let guard = crate::game_mutation_lock::try_lock(&game_id).expect("guard");
    let target = root.path().join("payload.dll");
    fs::write(&target, b"before").expect("seed");

    let mutation = DurableFileTransaction::prepare(
        &context,
        &guard,
        &scope(root.path()),
        "test-o3",
        None,
        [target.clone()],
    )
    .expect("prepare");
    fs::write(&target, b"foreign").expect("drift");

    let error = mutation
        .verify_before_apply()
        .expect_err("preimage drift must fail before work");
    assert!(error.to_string().contains("changed before apply"));
    assert_eq!(fs::read(&target).expect("foreign file"), b"foreign");
}

#[test]
fn transaction_accepts_an_explicit_external_addon_root() {
    let game = tempfile::tempdir().expect("game");
    let addon = tempfile::tempdir().expect("addon");
    let context = Context::open_at(game.path().join("catalog.db")).expect("context");
    let game_id = GameId::new("manual:split").expect("id");
    let guard = crate::game_mutation_lock::try_lock(&game_id).expect("guard");
    let game_file = game.path().join("dxgi.dll");
    let addon_file = addon.path().join("nvngx_dlss.dll");

    let mutation = DurableFileTransaction::prepare(
        &context,
        &guard,
        &MutationScope::new([game.path().to_path_buf(), addon.path().to_path_buf()])
            .expect("scope"),
        "test",
        None,
        [game_file.clone(), addon_file.clone()],
    )
    .expect("prepare split roots");
    fs::write(&game_file, b"host").expect("host");
    fs::write(&addon_file, b"payload").expect("payload");
    mutation.rollback(context.storage()).expect("rollback");

    assert!(!game_file.exists());
    assert!(!addon_file.exists());
}

#[test]
fn pre_catalog_prepared_recovery_restores_without_creating_catalog_authority() {
    let root = tempfile::tempdir().expect("game");
    let context = Context::open_at(root.path().join("catalog.db")).expect("context");
    let game_id = GameId::new("manual:pre-catalog-recovery").expect("id");
    let guard = crate::game_mutation_lock::try_lock(&game_id).expect("guard");
    let live = root.path().join("addon.dll");
    fs::write(&live, b"before").expect("seed");

    let mutation = DurableFileTransaction::prepare(
        &context,
        &guard,
        &scope(root.path()),
        "test",
        None,
        [live.clone()],
    )
    .expect("prepare without catalog");
    fs::write(&live, b"after").expect("mutate");
    drop(mutation);

    recover_pending(&context, &guard).expect("recover without catalog");
    assert_eq!(fs::read(live).expect("read"), b"before");
    assert!(context.storage().catalog_readiness(&game_id).is_err());
    assert!(
        context
            .storage()
            .pending_file_mutations_for_game(&game_id)
            .expect("pending rows")
            .is_empty()
    );
}

#[test]
fn preparing_recovery_never_restores_partial_snapshots() {
    let root = tempfile::tempdir().expect("game");
    let context = Context::open_at(root.path().join("catalog.db")).expect("context");
    let game_id = GameId::new("manual:preparing").expect("id");
    let guard = crate::game_mutation_lock::try_lock(&game_id).expect("guard");
    let id = "preparing-crash";
    let transaction_dir = context.file_mutation_root().join(id);
    fs::create_dir_all(&transaction_dir).expect("transaction dir");
    context
        .storage()
        .begin_file_mutation_preparation(&BeginFileMutationPreparation {
            id: id.to_owned(),
            game_id: game_id.clone(),
            feature: "test".to_owned(),
            subject_id: None,
            initial_manifest_json: serialize_manifest(&FileMutationManifest {
                format_version: MANIFEST_FORMAT_VERSION,
                roots: vec![
                    crate::paths::canonicalize_existing(root.path())
                        .unwrap()
                        .to_string_lossy()
                        .into_owned(),
                ],
                transaction_dir: transaction_dir.to_string_lossy().into_owned(),
                snapshots: Vec::new(),
                peer_ancestors: Vec::new(),
            })
            .unwrap(),
        })
        .expect("row");

    recover_pending(&context, &guard).expect("recover");

    assert!(!transaction_dir.exists());
    assert!(
        context
            .storage()
            .pending_file_mutations_for_game(&game_id)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn committed_recovery_only_cleans_snapshots() {
    let root = tempfile::tempdir().expect("game");
    let context = Context::open_at(root.path().join("catalog.db")).expect("context");
    let game_id = GameId::new("manual:committed-cleanup").expect("id");
    let guard = crate::game_mutation_lock::try_lock(&game_id).expect("guard");
    let live = root.path().join("nvngx_dlss.dll");
    fs::write(&live, b"before").expect("seed");

    let mutation = DurableFileTransaction::prepare(
        &context,
        &guard,
        &scope(root.path()),
        "test",
        None,
        [live.clone()],
    )
    .expect("prepare");
    fs::write(&live, b"committed").expect("mutate");
    context
        .storage()
        .commit_game_mutation(GameMutationCommit {
            game_id: &game_id,
            component_set: None,
            baseline_mutations: &[],
            addon: InstalledAddonMutation::Keep,
            mutation_id: Some(mutation.id()),
        })
        .expect("commit row");
    drop(mutation);

    recover_pending(&context, &guard).expect("recover");
    assert_eq!(fs::read(live).unwrap(), b"committed");
    assert!(
        context
            .storage()
            .pending_file_mutations_for_game(&game_id)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn missing_snapshot_stops_before_row_cleanup_and_retains_prepared_invalidation() {
    let root = tempfile::tempdir().expect("game");
    let context = Context::open_at(root.path().join("catalog.db")).expect("context");
    let game_id = GameId::new("manual:missing-snapshot").expect("id");
    store_game(&context, game_id.clone(), root.path());
    let guard = crate::game_mutation_lock::try_lock(&game_id).expect("guard");
    let live = root.path().join("nvngx_dlss.dll");
    fs::write(&live, b"before").expect("seed");

    let mutation = DurableFileTransaction::prepare(
        &context,
        &guard,
        &scope(root.path()),
        "test",
        None,
        [live.clone()],
    )
    .expect("prepare");
    let mutation_id = mutation.id().to_owned();
    let row = context
        .storage()
        .get_pending_file_mutation(&mutation_id)
        .expect("row")
        .expect("prepared row");
    let manifest = super::manifest::deserialize_manifest(&row).expect("manifest");
    let snapshot = manifest.snapshots[0]
        .snapshot
        .as_ref()
        .expect("before snapshot");
    fs::remove_file(snapshot).expect("remove snapshot");
    fs::write(&live, b"after").expect("mutate");
    drop(mutation);

    recover_pending(&context, &guard).expect_err("missing snapshot must stop recovery");
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
    assert_eq!(fs::read(live).expect("live bytes"), b"after");
}

#[test]
fn commit_or_rollback_cleans_on_success_and_restores_on_failure() {
    let root = tempfile::tempdir().expect("game");
    let context = Context::open_at(root.path().join("catalog.db")).expect("context");
    let game_id = GameId::new("manual:commit-or-rollback").expect("id");
    let guard = crate::game_mutation_lock::try_lock(&game_id).expect("guard");
    let live = root.path().join("payload.dll");
    fs::write(&live, b"before").expect("seed");

    let mutation = DurableFileTransaction::prepare(
        &context,
        &guard,
        &scope(root.path()),
        "test_ok",
        Some(game_id.as_str()),
        [live.clone()],
    )
    .expect("prepare");
    let mutation_id = mutation.id().to_owned();
    fs::write(&live, b"after").expect("mutate");
    mutation
        .commit_or_rollback(
            context.storage(),
            || {
                context
                    .storage()
                    .commit_game_mutation(GameMutationCommit {
                        game_id: &game_id,
                        component_set: None,
                        baseline_mutations: &[],
                        addon: InstalledAddonMutation::Keep,
                        mutation_id: Some(&mutation_id),
                    })
                    .map_err(ServiceError::from)?;
                Ok::<(), ServiceError>(())
            },
            |()| {},
            || {},
        )
        .expect("commit");
    assert_eq!(fs::read(&live).unwrap(), b"after");
    assert!(
        context
            .storage()
            .pending_file_mutations_for_game(&game_id)
            .unwrap()
            .is_empty()
    );

    let mutation = DurableFileTransaction::prepare(
        &context,
        &guard,
        &scope(root.path()),
        "test_err",
        Some(game_id.as_str()),
        [live.clone()],
    )
    .expect("prepare err path");
    fs::write(&live, b"broken").expect("mutate");
    let error = mutation
        .commit_or_rollback(
            context.storage(),
            || Err::<(), _>(crate::failed("apply failed")),
            |()| {},
            || {},
        )
        .expect_err("work failure");
    assert!(error.to_string().contains("apply failed"));
    assert_eq!(fs::read(&live).unwrap(), b"after");
    assert!(
        context
            .storage()
            .pending_file_mutations_for_game(&game_id)
            .unwrap()
            .is_empty()
    );
}
