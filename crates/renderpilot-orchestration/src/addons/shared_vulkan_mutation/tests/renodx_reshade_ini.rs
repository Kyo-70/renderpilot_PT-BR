use renderpilot_domain::{GameId, PathRef, RenoDxReshadeIniAuthority, RenoDxReshadeIniFeature};

use super::super::io;
use super::super::plan::{FileIntent, MutationPlan, Request as PlanRequest};
use super::super::transaction::{
    build_shared_peer_evidence, build_shared_peer_program, capture_shared_endpoint_snapshots,
    validate_renodx_authority,
};
use super::super::{Scope, TrustedRoots};

#[test]
fn shared_peer_snapshot_retains_typed_renodx_role_for_exact_game_zero_ini() {
    let temp = tempfile::tempdir().expect("tempdir");
    let game_root = temp.path().join("game");
    let shared_root = temp.path().join("shared");
    std::fs::create_dir_all(&game_root).expect("game root");
    std::fs::create_dir_all(&shared_root).expect("shared root");
    let ini = game_root.join("ReShade.ini");
    let shared_file = shared_root.join("layer.dll");
    std::fs::write(&ini, b"before").expect("ini");
    std::fs::write(&shared_file, b"shared-before").expect("shared file");
    let scope = crate::file_mutation::MutationScope::single(&game_root).expect("scope");
    let roots = TrustedRoots::game_shared(&scope, &shared_root).expect("roots");
    let transaction_root = temp.path().join("transaction");
    let plan = MutationPlan::build(PlanRequest {
        transaction_root: transaction_root.clone(),
        mutation_id: "peer-typed-role".to_owned(),
        roots,
        scope: Scope::GameShared,
        game_id: Some("steam:typed-role".to_owned()),
        feature: renderpilot_domain::RENODX_INSTALL.to_owned(),
        intents: vec![
            FileIntent {
                live_path: ini,
                before: Some(b"before".to_vec()),
                after: Some(b"after".to_vec()),
            },
            FileIntent {
                live_path: shared_file,
                before: Some(b"shared-before".to_vec()),
                after: Some(b"shared-after".to_vec()),
            },
        ],
        registry: Vec::new(),
        registry_authority: None,
        created_dirs: Vec::new(),
    })
    .expect("plan");
    let authority = RenoDxReshadeIniAuthority::new(
        RenoDxReshadeIniFeature::Install,
        PathRef::new(game_root.to_string_lossy().replace('\\', "/")).expect("root path"),
    )
    .expect("authority");

    let before =
        capture_shared_endpoint_snapshots(&plan, &[], Some(&authority)).expect("typed snapshots");
    assert_eq!(before.len(), 2);
    assert_eq!(
        before[0].role,
        renderpilot_domain::PeerEndpointRole::RenoDxReshadeIni
    );
    assert_eq!(
        before[1].role,
        renderpilot_domain::PeerEndpointRole::Disjoint
    );
    let program =
        build_shared_peer_program(&plan, &before, "peer-typed-role").expect("typed peer program");
    assert_eq!(
        program["endpoints"][0]["role"],
        serde_json::Value::String("renodx_reshade_ini".to_owned())
    );

    io::materialize_stages(&plan).expect("stages");
    io::apply_files(
        &transaction_root,
        &plan.manifest,
        &plan.payloads,
        &plan.roots,
    )
    .expect("apply");
    let after =
        capture_shared_endpoint_snapshots(&plan, &[], Some(&authority)).expect("after snapshots");
    let evidence = build_shared_peer_evidence(&before, &after).expect("typed evidence");
    assert_eq!(evidence.len(), 2);
    for (index, endpoint) in evidence.iter().enumerate() {
        assert_eq!(endpoint.intent().path(), &before[index].path);
        assert_eq!(endpoint.intent().role(), before[index].role);
    }
}

#[test]
fn shared_peer_rejects_renodx_authority_feature_drift_before_reservation() {
    let temp = tempfile::tempdir().expect("tempdir");
    let game_root = temp.path().join("game");
    let shared_root = temp.path().join("shared");
    std::fs::create_dir_all(&game_root).expect("game root");
    std::fs::create_dir_all(&shared_root).expect("shared root");
    let authority = RenoDxReshadeIniAuthority::new(
        RenoDxReshadeIniFeature::Install,
        PathRef::new(game_root.to_string_lossy().replace('\\', "/")).expect("root"),
    )
    .expect("authority");
    let roots = TrustedRoots::game_shared(
        &crate::file_mutation::MutationScope::single(&game_root).expect("scope"),
        &shared_root,
    )
    .expect("roots");

    let error = validate_renodx_authority(
        Some(&authority),
        &roots,
        Some(&GameId::new("steam:renodx-feature-drift").expect("game id")),
        renderpilot_domain::mutation_features::RENODX_UPDATE,
    )
    .expect_err("feature drift must fail closed");

    assert!(error.to_string().contains("game-scoped feature"));
}

#[test]
fn shared_peer_rejects_wrong_renodx_root_without_typed_capture() {
    let temp = tempfile::tempdir().expect("tempdir");
    let game_root = temp.path().join("game");
    let other_root = temp.path().join("other");
    let shared_root = temp.path().join("shared");
    std::fs::create_dir_all(&game_root).expect("game root");
    std::fs::create_dir_all(&other_root).expect("other root");
    std::fs::create_dir_all(&shared_root).expect("shared root");
    let ini = game_root.join("ReShade.ini");
    let shared_file = shared_root.join("layer.dll");
    std::fs::write(&ini, b"before").expect("ini");
    std::fs::write(&shared_file, b"before").expect("shared file");
    let scope = crate::file_mutation::MutationScope::single(&game_root).expect("scope");
    let roots = TrustedRoots::game_shared(&scope, &shared_root).expect("roots");
    let plan = MutationPlan::build(PlanRequest {
        transaction_root: temp.path().join("transaction"),
        mutation_id: "peer-wrong-root".to_owned(),
        roots,
        scope: Scope::GameShared,
        game_id: Some("steam:wrong-root".to_owned()),
        feature: renderpilot_domain::RENODX_INSTALL.to_owned(),
        intents: vec![
            FileIntent {
                live_path: ini,
                before: Some(b"before".to_vec()),
                after: Some(b"after".to_vec()),
            },
            FileIntent {
                live_path: shared_file,
                before: Some(b"before".to_vec()),
                after: Some(b"after".to_vec()),
            },
        ],
        registry: Vec::new(),
        registry_authority: None,
        created_dirs: Vec::new(),
    })
    .expect("plan");
    let wrong_authority = RenoDxReshadeIniAuthority::new(
        RenoDxReshadeIniFeature::Install,
        PathRef::new(other_root.to_string_lossy().replace('\\', "/")).expect("other root"),
    )
    .expect("wrong authority");

    let error = capture_shared_endpoint_snapshots(&plan, &[], Some(&wrong_authority))
        .expect_err("wrong root must not classify a typed endpoint");

    assert!(error.to_string().contains("exactly one changed endpoint"));
}

#[test]
fn shared_peer_rejects_game_one_renodx_endpoint() {
    let temp = tempfile::tempdir().expect("tempdir");
    let first = temp.path().join("a-game");
    let second = temp.path().join("b-game");
    let shared_root = temp.path().join("shared");
    std::fs::create_dir_all(&first).expect("first root");
    std::fs::create_dir_all(&second).expect("second root");
    std::fs::create_dir_all(&shared_root).expect("shared root");
    let scope =
        crate::file_mutation::MutationScope::new([first.clone(), second.clone()]).expect("scope");
    let roots = TrustedRoots::game_shared(&scope, &shared_root).expect("roots");
    let game_one = [first, second]
        .into_iter()
        .find(|root| {
            roots
                .authorize(&root.join("ReShade.ini"))
                .expect("authorized path")
                .root_id()
                == "game-1"
        })
        .expect("game-1 root");
    let ini = game_one.join("ReShade.ini");
    std::fs::write(&ini, b"before").expect("ini");
    let authority = RenoDxReshadeIniAuthority::new(
        RenoDxReshadeIniFeature::Install,
        PathRef::new(game_one.to_string_lossy().replace('\\', "/")).expect("root"),
    )
    .expect("authority");
    let plan = MutationPlan::build(PlanRequest {
        transaction_root: temp.path().join("transaction"),
        mutation_id: "peer-game-one".to_owned(),
        roots,
        scope: Scope::GameShared,
        game_id: Some("steam:game-one".to_owned()),
        feature: renderpilot_domain::RENODX_INSTALL.to_owned(),
        intents: vec![FileIntent {
            live_path: ini,
            before: Some(b"before".to_vec()),
            after: Some(b"after".to_vec()),
        }],
        registry: Vec::new(),
        registry_authority: None,
        created_dirs: Vec::new(),
    })
    .expect("plan");

    let error = capture_shared_endpoint_snapshots(&plan, &[], Some(&authority))
        .expect_err("game-1 typed endpoint must be rejected");

    assert!(error.to_string().contains("game-0"));
}

#[test]
fn shared_peer_rejects_missing_and_duplicate_typed_renodx_endpoints() {
    let temp = tempfile::tempdir().expect("tempdir");
    let game_root = temp.path().join("game");
    let shared_root = temp.path().join("shared");
    std::fs::create_dir_all(&game_root).expect("game root");
    std::fs::create_dir_all(&shared_root).expect("shared root");
    let ini = game_root.join("ReShade.ini");
    std::fs::write(&ini, b"before").expect("ini");
    let scope = crate::file_mutation::MutationScope::single(&game_root).expect("scope");
    let roots = TrustedRoots::game_shared(&scope, &shared_root).expect("roots");
    let authority = RenoDxReshadeIniAuthority::new(
        RenoDxReshadeIniFeature::Install,
        PathRef::new(game_root.to_string_lossy().replace('\\', "/")).expect("root"),
    )
    .expect("authority");

    let missing = MutationPlan::build(PlanRequest {
        transaction_root: temp.path().join("missing-transaction"),
        mutation_id: "peer-missing-ini".to_owned(),
        roots: roots.clone(),
        scope: Scope::GameShared,
        game_id: Some("steam:missing-ini".to_owned()),
        feature: renderpilot_domain::RENODX_INSTALL.to_owned(),
        intents: Vec::new(),
        registry: Vec::new(),
        registry_authority: None,
        created_dirs: Vec::new(),
    })
    .expect("missing plan");
    let missing_error = capture_shared_endpoint_snapshots(&missing, &[], Some(&authority))
        .expect_err("missing typed endpoint must be rejected");
    assert!(
        missing_error
            .to_string()
            .contains("exactly one changed endpoint")
    );

    let mut duplicate = MutationPlan::build(PlanRequest {
        transaction_root: temp.path().join("duplicate-transaction"),
        mutation_id: "peer-duplicate-ini".to_owned(),
        roots,
        scope: Scope::GameShared,
        game_id: Some("steam:duplicate-ini".to_owned()),
        feature: renderpilot_domain::RENODX_INSTALL.to_owned(),
        intents: vec![FileIntent {
            live_path: ini,
            before: Some(b"before".to_vec()),
            after: Some(b"after-one".to_vec()),
        }],
        registry: Vec::new(),
        registry_authority: None,
        created_dirs: Vec::new(),
    })
    .expect("duplicate plan");
    duplicate
        .manifest
        .files
        .push(duplicate.manifest.files[0].clone());
    let duplicate_error = capture_shared_endpoint_snapshots(&duplicate, &[], Some(&authority))
        .expect_err("duplicate typed endpoints must be rejected");
    assert!(
        duplicate_error
            .to_string()
            .contains("exactly one changed endpoint")
    );
}
