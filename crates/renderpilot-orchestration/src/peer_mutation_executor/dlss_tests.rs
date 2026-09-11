use std::fs;

use renderpilot_application::InstalledAddonRepository;
use renderpilot_domain::{
    AddonKind, GameId, InstalledAddon, PeerEndpointRole, RenoDxDlssBeforeImage, RenoDxDlssClaim,
    RenoDxDlssProjection, TrackedSource, TrackedSourceRole,
};

use super::timing_tests::{path_ref, seed_game, seed_optiscaler};
use super::{
    EndpointExpectation, EndpointPostcondition, ExactEndpoint, ExactEndpointProgram,
    PeerPathSnapshot, observe_peer_path_snapshot,
};
use crate::addons::engine::InstallChanges;
use crate::addons::peer_lifecycle::package::{PeerMutationPackage, RenoDxDlssMutationRequest};
use crate::{Context, game_mutation_lock};

fn source(channel: &str) -> TrackedSource {
    TrackedSource::new(
        TrackedSourceRole::DlssFix,
        "https://example.test/renodx-dlss-fix",
        None,
        format!("digest-{channel}"),
    )
    .with_channel(channel)
}

fn digest(bytes: &[u8]) -> renderpilot_domain::Sha256Hash {
    renderpilot_detection::sha256_bytes(bytes).expect("digest")
}

#[test]
fn claim_only_dlss_package_commits_peer_atomically_and_cleans_pending_row() {
    let database = tempfile::tempdir().expect("database");
    let game = tempfile::tempdir().expect("game");
    let context = Context::open_at(database.path().join("catalog.sqlite")).expect("context");
    let game_id =
        GameId::new(format!("manual:dlss-claim-only:{}", ulid::Ulid::generate())).expect("game id");
    let game_root = fs::canonicalize(game.path()).expect("canonical game root");
    seed_game(&context, &game_id, &game_root);

    let addon = game_root.join("renodx.addon64");
    let companion = game_root.join("nvngx_dlss.dll");
    fs::write(&addon, b"renodx").expect("addon");
    fs::write(&companion, b"dlss").expect("companion");
    let old_source = source("old");
    let new_source = source("new");
    let before_peer = InstalledAddon::new(game_id.clone(), AddonKind::RenoDx, path_ref(&addon))
        .with_created_file(path_ref(&addon))
        .with_created_file(path_ref(&companion))
        .with_tracked_sources(vec![old_source.clone()]);
    context
        .storage()
        .upsert_installed_addon(&before_peer)
        .expect("peer preimage");
    let before_peer = context
        .storage()
        .get_installed_addon(&game_id)
        .expect("stored peer preimage")
        .expect("stored peer");
    let topology = seed_optiscaler(&context, &game_id, &game_root);
    let planned = renderpilot_domain::PlannedGameProxyTopology::Exact(topology.clone());

    let companion_ref = path_ref(&companion);
    let root_ref = path_ref(&game_root);
    let snapshot = observe_peer_path_snapshot(&companion_ref, &root_ref).expect("snapshot");
    let PeerPathSnapshot::File(_) = &snapshot else {
        panic!("companion snapshot must be present");
    };
    let file = snapshot.file().expect("verified companion");
    let before_image = RenoDxDlssBeforeImage::present(
        file.identity().to_owned(),
        file.digest().clone(),
        file.length(),
        snapshot.bytes().expect("retained bytes").to_vec(),
    )
    .expect("before image");
    let before_claim = RenoDxDlssClaim::new(true, Some(old_source)).expect("before claim");
    let after_claim = RenoDxDlssClaim::new(true, Some(new_source.clone())).expect("after claim");
    let projection =
        RenoDxDlssProjection::new(companion_ref, before_image, before_claim, after_claim);
    let after_peer = before_peer.clone().with_tracked_sources(vec![new_source]);
    let package = PeerMutationPackage::plan_active_with_renodx_dlss(RenoDxDlssMutationRequest {
        before_peer: &before_peer,
        after_peer: &after_peer,
        before_topology: &topology,
        planned_after_topology: &planned,
        program: None,
        payloads: Vec::new(),
        game_root,
        payload_root: None,
        renodx_reshade_ini: None,
        projection,
    })
    .expect("claim-only package");
    let guard = game_mutation_lock::try_lock(&game_id).expect("guard");
    let prepared = context
        .peer_mutation_executor()
        .prepare_ordinary_file_peer_with_renodx_dlss(
            &context,
            &guard,
            renderpilot_domain::mutation_features::RENODX_DLSS_FIX_UPDATE,
            Some(game_id.as_str()),
            package,
        )
        .expect("claim-only preparation");
    let mut changes = InstallChanges::default();
    let applied = prepared.apply(&mut changes).expect("claim-only apply");
    changes.sync_touched_dirs();
    applied.commit().expect("claim-only commit");

    let persisted = context
        .storage()
        .get_installed_addon(&game_id)
        .expect("persisted peer")
        .expect("persisted peer exists");
    assert_eq!(persisted.game_id(), after_peer.game_id());
    assert_eq!(persisted.kind(), after_peer.kind());
    assert_eq!(persisted.addon_file(), after_peer.addon_file());
    assert_eq!(persisted.created_files(), after_peer.created_files());
    assert_eq!(persisted.tracked_sources(), after_peer.tracked_sources());
    let rows = context
        .storage()
        .pending_file_mutations_for_game(&game_id)
        .expect("pending rows");
    assert!(rows.is_empty(), "committed durable row must be cleaned");
    assert_eq!(fs::read(companion).expect("companion bytes"), b"dlss");
}

#[test]
fn prepared_claim_only_dlss_row_recovers_without_restoring_live_files() {
    let database = tempfile::tempdir().expect("database");
    let game = tempfile::tempdir().expect("game");
    let payload = tempfile::tempdir().expect("payload");
    let context = Context::open_at(database.path().join("catalog.sqlite")).expect("context");
    let game_id =
        GameId::new(format!("manual:dlss-recovery:{}", ulid::Ulid::generate())).expect("game id");
    let game_root = fs::canonicalize(game.path()).expect("canonical game root");
    let payload_root = fs::canonicalize(payload.path()).expect("canonical payload root");
    seed_game(&context, &game_id, &game_root);

    let addon = payload_root.join("renodx.addon64");
    let companion = payload_root.join("nvngx_dlss.dll");
    fs::write(&addon, b"renodx").expect("addon");
    fs::write(&companion, b"dlss").expect("companion");
    let before_peer = InstalledAddon::new(game_id.clone(), AddonKind::RenoDx, path_ref(&addon))
        .with_created_file(path_ref(&addon))
        .with_created_file(path_ref(&companion))
        .with_tracked_sources(vec![source("old")]);
    context
        .storage()
        .upsert_installed_addon(&before_peer)
        .expect("peer preimage");
    let before_peer = context
        .storage()
        .get_installed_addon(&game_id)
        .expect("stored peer preimage")
        .expect("stored peer");
    let topology = seed_optiscaler(&context, &game_id, &game_root);
    let planned = renderpilot_domain::PlannedGameProxyTopology::Exact(topology.clone());

    let companion_ref = path_ref(&companion);
    let root_ref = path_ref(&payload_root);
    let snapshot = observe_peer_path_snapshot(&companion_ref, &root_ref).expect("snapshot");
    let file = snapshot.file().expect("verified companion");
    let before_image = RenoDxDlssBeforeImage::present(
        file.identity().to_owned(),
        file.digest().clone(),
        file.length(),
        snapshot.bytes().expect("retained bytes").to_vec(),
    )
    .expect("before image");
    let projection = RenoDxDlssProjection::new(
        companion_ref,
        before_image,
        RenoDxDlssClaim::new(true, Some(source("old"))).expect("before claim"),
        RenoDxDlssClaim::new(true, Some(source("new"))).expect("after claim"),
    );
    let after_peer = before_peer
        .clone()
        .with_tracked_sources(vec![source("new")]);
    let package = PeerMutationPackage::plan_active_with_renodx_dlss(RenoDxDlssMutationRequest {
        before_peer: &before_peer,
        after_peer: &after_peer,
        before_topology: &topology,
        planned_after_topology: &planned,
        program: None,
        payloads: Vec::new(),
        game_root,
        payload_root: Some(payload_root),
        renodx_reshade_ini: None,
        projection,
    })
    .expect("claim-only package");
    let guard = game_mutation_lock::try_lock(&game_id).expect("guard");
    let prepared = context
        .peer_mutation_executor()
        .prepare_ordinary_file_peer_with_renodx_dlss(
            &context,
            &guard,
            renderpilot_domain::mutation_features::RENODX_DLSS_FIX_UPDATE,
            Some(game_id.as_str()),
            package,
        )
        .expect("claim-only preparation");
    drop(prepared);

    let recovered = crate::file_mutation::recover_pending_matching(&context, &guard, |row| {
        row.feature == renderpilot_domain::mutation_features::RENODX_DLSS_FIX_UPDATE
    })
    .expect("recover prepared claim-only row");
    assert_eq!(recovered, 1);
    assert!(
        context
            .storage()
            .pending_file_mutations_for_game(&game_id)
            .expect("pending rows")
            .is_empty()
    );
    assert_eq!(
        context
            .storage()
            .get_installed_addon(&game_id)
            .expect("persisted peer")
            .expect("persisted peer exists")
            .tracked_sources(),
        before_peer.tracked_sources()
    );
    assert_eq!(fs::read(companion).expect("companion bytes"), b"dlss");
}

#[test]
fn applied_external_dlss_create_is_recovered_through_the_sealed_payload_root() {
    let database = tempfile::tempdir().expect("database");
    let game = tempfile::tempdir().expect("game");
    let payload = tempfile::tempdir().expect("payload");
    let context = Context::open_at(database.path().join("catalog.sqlite")).expect("context");
    let game_id = GameId::new(format!(
        "manual:dlss-external-recovery:{}",
        ulid::Ulid::generate()
    ))
    .expect("game id");
    let game_root = fs::canonicalize(game.path()).expect("canonical game root");
    let payload_root = fs::canonicalize(payload.path()).expect("canonical payload root");
    seed_game(&context, &game_id, &game_root);

    let addon = payload_root.join("renodx.addon64");
    let companion = payload_root.join("renodx-dlssfix.addon64");
    fs::write(&addon, b"renodx").expect("addon");
    let before_peer = InstalledAddon::new(game_id.clone(), AddonKind::RenoDx, path_ref(&addon))
        .with_created_file(path_ref(&addon));
    context
        .storage()
        .upsert_installed_addon(&before_peer)
        .expect("peer preimage");
    let before_peer = context
        .storage()
        .get_installed_addon(&game_id)
        .expect("stored peer preimage")
        .expect("stored peer");
    let topology = seed_optiscaler(&context, &game_id, &game_root);
    let planned = renderpilot_domain::PlannedGameProxyTopology::Exact(topology.clone());
    let companion_ref = path_ref(&companion);
    let next_source = source("external");
    let after_peer = before_peer
        .clone()
        .with_created_file(companion_ref.clone())
        .with_tracked_source(next_source.clone());
    let bytes = b"external companion";
    let program = ExactEndpointProgram::new(vec![ExactEndpoint::new(
        companion_ref.clone(),
        PeerEndpointRole::Disjoint,
        EndpointExpectation::Absent,
        EndpointPostcondition::File(digest(bytes)),
    )])
    .expect("program");
    let projection = RenoDxDlssProjection::new(
        companion_ref,
        RenoDxDlssBeforeImage::absent(),
        RenoDxDlssClaim::absent(),
        RenoDxDlssClaim::new(true, Some(next_source)).expect("after claim"),
    );
    let package = PeerMutationPackage::plan_active_with_renodx_dlss(RenoDxDlssMutationRequest {
        before_peer: &before_peer,
        after_peer: &after_peer,
        before_topology: &topology,
        planned_after_topology: &planned,
        program: Some(program),
        payloads: vec![Some(bytes.to_vec())],
        game_root,
        payload_root: Some(payload_root),
        renodx_reshade_ini: None,
        projection,
    })
    .expect("external physical package");
    let guard = game_mutation_lock::try_lock(&game_id).expect("guard");
    let prepared = context
        .peer_mutation_executor()
        .prepare_ordinary_file_peer_with_renodx_dlss(
            &context,
            &guard,
            renderpilot_domain::mutation_features::RENODX_DLSS_FIX_INSTALL,
            Some(game_id.as_str()),
            package,
        )
        .expect("external physical preparation");
    let mut changes = InstallChanges::default();
    let applied = prepared.apply(&mut changes).expect("external apply");
    changes.sync_touched_dirs();
    assert_eq!(fs::read(&companion).expect("applied companion"), bytes);
    drop(applied);

    let recovered = crate::file_mutation::recover_pending_matching(&context, &guard, |row| {
        row.feature == renderpilot_domain::mutation_features::RENODX_DLSS_FIX_INSTALL
    })
    .expect("recover external physical row");
    assert_eq!(recovered, 1);
    assert!(!companion.exists());
    assert_eq!(
        context
            .storage()
            .get_installed_addon(&game_id)
            .expect("persisted peer")
            .expect("persisted peer exists"),
        before_peer
    );
}
