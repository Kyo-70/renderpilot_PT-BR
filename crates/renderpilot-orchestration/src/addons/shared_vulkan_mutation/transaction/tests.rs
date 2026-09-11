use renderpilot_domain::{AddonKind, GameId, normalized_path_key};
use renderpilot_storage_sqlite::{InstalledAddonMutation, SharedArtifactMutation};

use super::*;

#[test]
fn shared_only_noop_projects_without_reserving_the_singleton() {
    let temp = tempfile::tempdir().expect("tempdir");
    let context = crate::Context::open_at(temp.path().join("catalog.sqlite")).expect("context");
    let live = temp.path().join("ReShade64.dll");
    std::fs::write(&live, b"unchanged").expect("seed shared file");
    let roots = TrustedRoots::shared_only(temp.path()).expect("roots");

    execute(Request::new(
        &context,
        MutationIdentity::new("shared-noop", ScopeSpec::shared_only(), "test"),
        PhysicalParticipants::new(
            roots,
            super::super::composer::ComposedParticipants {
                files: vec![FileIntent {
                    live_path: live,
                    before: Some(b"unchanged".to_vec()),
                    after: Some(b"unchanged".to_vec()),
                }],
                registry: Vec::new(),
                created_dirs: Vec::new(),
            },
            None,
        ),
        CatalogProjection::new(SharedArtifactMutation::Keep),
    ))
    .expect("no-op projection");

    assert!(
        context
            .storage()
            .pending_shared_vulkan_mutation()
            .expect("pending row")
            .is_none()
    );
}

#[test]
fn peer_route_rejects_shared_only_scope_before_projection_or_reservation() {
    let temp = tempfile::tempdir().expect("tempdir");
    let context = crate::Context::open_at(temp.path().join("catalog.sqlite")).expect("context");
    let live = temp.path().join("ReShade64.dll");
    std::fs::write(&live, b"unchanged").expect("seed shared file");
    let roots = TrustedRoots::shared_only(temp.path()).expect("roots");

    let request = Request::new(
        &context,
        MutationIdentity::new("peer-shared-only", ScopeSpec::shared_only(), "test"),
        PhysicalParticipants::new(
            roots,
            super::super::composer::ComposedParticipants {
                files: vec![FileIntent {
                    live_path: live,
                    before: Some(b"unchanged".to_vec()),
                    after: Some(b"changed".to_vec()),
                }],
                registry: Vec::new(),
                created_dirs: Vec::new(),
            },
            None,
        ),
        CatalogProjection::new(SharedArtifactMutation::Keep),
    );
    let game = GameId::new("steam:peer-shared-only").expect("game id");
    let error = execute_peer_unchanged(PeerUnchangedRequest::new(
        request, &game, None, None, None, None,
    ))
    .expect_err("shared-only peer route must be excluded");
    assert!(error.to_string().contains("game-scoped owner"));
    assert!(
        context
            .storage()
            .pending_shared_vulkan_mutation()
            .expect("pending row")
            .is_none()
    );
}

#[test]
fn game_shared_scope_always_carries_an_owner_and_projection() {
    let game = GameId::new("steam:123").expect("game id");
    let (scope, owner, addon) = ScopeSpec::game_delete(&game, AddonKind::RenoDx).storage_parts();
    assert_eq!(scope, Scope::GameShared);
    assert_eq!(owner, Some(&game));
    assert!(matches!(
        addon,
        InstalledAddonMutation::Delete(AddonKind::RenoDx)
    ));
}

#[test]
fn shared_peer_evidence_covers_changed_game_and_shared_files_only() {
    let temp = tempfile::tempdir().expect("tempdir");
    let game_root = temp.path().join("game");
    let shared_root = temp.path().join("shared");
    std::fs::create_dir_all(&game_root).expect("game root");
    std::fs::create_dir_all(&shared_root).expect("shared root");
    let game_file = game_root.join("game.dll");
    let shared_file = shared_root.join("shared.dll");
    let noop_file = game_root.join("noop.dll");
    std::fs::write(&game_file, b"game-before").expect("game file");
    std::fs::write(&shared_file, b"shared-before").expect("shared file");
    std::fs::write(&noop_file, b"same").expect("noop file");
    let scope = crate::file_mutation::MutationScope::single(&game_root).expect("scope");
    let roots = TrustedRoots::game_shared(&scope, &shared_root).expect("roots");
    let plan = MutationPlan::build(PlanRequest {
        transaction_root: temp.path().join("transaction"),
        mutation_id: "peer-evidence".to_owned(),
        roots,
        scope: Scope::GameShared,
        game_id: Some("steam:evidence".to_owned()),
        feature: "test".to_owned(),
        intents: vec![
            FileIntent {
                live_path: game_file.clone(),
                before: Some(b"game-before".to_vec()),
                after: Some(b"game-after".to_vec()),
            },
            FileIntent {
                live_path: shared_file.clone(),
                before: Some(b"shared-before".to_vec()),
                after: Some(b"shared-after".to_vec()),
            },
            FileIntent {
                live_path: noop_file,
                before: Some(b"same".to_vec()),
                after: Some(b"same".to_vec()),
            },
        ],
        registry: Vec::new(),
        registry_authority: None,
        created_dirs: Vec::new(),
    })
    .expect("plan");

    let evidence = capture_shared_endpoint_snapshots(&plan, &[], None).expect("evidence");
    assert_eq!(evidence.len(), 2);
    assert_eq!(
        normalized_path_key(evidence[0].path.as_str()),
        normalized_path_key(&game_file.to_string_lossy())
    );
    assert_eq!(
        normalized_path_key(evidence[1].path.as_str()),
        normalized_path_key(&shared_file.to_string_lossy())
    );
}
