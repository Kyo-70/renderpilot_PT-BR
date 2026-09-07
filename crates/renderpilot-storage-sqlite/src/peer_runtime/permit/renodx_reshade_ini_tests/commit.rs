use renderpilot_application::{GameRepository, InstalledAddonRepository, ProxyTopologyRepository};
use renderpilot_domain::{
    GameIdentity, GameInstallation, GameRuntime, Launcher, PeerEndpointEvidence, PeerFileImage,
    PeerReadGuardEvidence, PeerReadGuardExpectation, PeerReadGuardRequirement,
    PlannedGameProxyTopology, Platform, ProxyPeerRoute,
    required_read_guards_with_renodx_reshade_ini,
};
use serde_json::json;

use super::super::{PeerCommitPreparation, PeerStorageRuntime};
use super::{AFTER_DIGEST, begin, finish_install, hash, install_manifest, path};
use crate::{PendingFileMutationState, SqliteStorage};

#[test]
fn final_read_guard_rederivation_uses_the_sealed_authority() {
    let storage = SqliteStorage::in_memory().expect("storage");
    let game_id = renderpilot_domain::GameId::new("game:renodx-commit").expect("game id");
    let id = "renodx-commit";
    begin(&storage, id, &game_id, renderpilot_domain::RENODX_INSTALL);
    let runtime = PeerStorageRuntime::new(storage);
    let after = super::main_after(&game_id);
    let manifest = install_manifest(id);
    let permit =
        finish_install(&runtime, id, &game_id, &after, &manifest, None).expect("typed preparation");
    let program = super::super::super::manifest::parse_peer_program(
        &super::super::super::manifest::parse_manifest(&manifest, "test").expect("manifest"),
        "test",
    )
    .expect("program");
    let evidence = vec![
        PeerEndpointEvidence::new(
            program.intents()[0].clone(),
            None,
            Some(PeerFileImage::new("addon-after", hash(super::BEFORE_DIGEST), 8).expect("image")),
        ),
        PeerEndpointEvidence::new(
            program.intents()[1].clone(),
            None,
            Some(PeerFileImage::new("ini-after", hash(AFTER_DIGEST), 4).expect("image")),
        ),
    ];

    runtime
        .seal_and_commit_ordinary_peer(permit, evidence, Vec::new())
        .expect("typed commit");
    assert_eq!(
        runtime
            .repositories()
            .get_pending_file_mutation(id)
            .expect("row")
            .expect("row exists")
            .state,
        PendingFileMutationState::Committed
    );
    let persisted = runtime
        .repositories()
        .get_installed_addon(&game_id)
        .expect("record")
        .expect("record exists");
    assert_eq!(persisted.kind(), after.kind());
    assert_eq!(persisted.addon_file(), after.addon_file());
    assert_eq!(persisted.created_files(), after.created_files());
}

#[test]
fn final_read_guard_evidence_cannot_be_added_to_typed_endpoint() {
    let storage = SqliteStorage::in_memory().expect("storage");
    let game_id = renderpilot_domain::GameId::new("game:renodx-guard-drift").expect("game id");
    let id = "renodx-guard-drift";
    begin(&storage, id, &game_id, renderpilot_domain::RENODX_INSTALL);
    let runtime = PeerStorageRuntime::new(storage);
    let after = super::main_after(&game_id);
    let manifest = install_manifest(id);
    let permit =
        finish_install(&runtime, id, &game_id, &after, &manifest, None).expect("typed preparation");
    let program = super::super::super::manifest::parse_peer_program(
        &super::super::super::manifest::parse_manifest(&manifest, "test").expect("manifest"),
        "test",
    )
    .expect("program");
    let evidence = vec![
        PeerEndpointEvidence::new(
            program.intents()[0].clone(),
            None,
            Some(PeerFileImage::new("addon-after", hash(super::BEFORE_DIGEST), 8).expect("image")),
        ),
        PeerEndpointEvidence::new(
            program.intents()[1].clone(),
            None,
            Some(PeerFileImage::new("ini-after", hash(AFTER_DIGEST), 4).expect("image")),
        ),
    ];
    let extra_guard = vec![PeerReadGuardEvidence::new(path("C:/game/extra"), None)];

    assert!(
        runtime
            .seal_and_commit_ordinary_peer(permit, evidence, extra_guard)
            .is_err()
    );
    assert_eq!(
        runtime
            .repositories()
            .get_pending_file_mutation(id)
            .expect("row")
            .expect("row exists")
            .state,
        PendingFileMutationState::Prepared
    );
}

#[test]
fn prepared_typed_manifest_role_and_path_tamper_changes_cas_and_blocks_commit() {
    for (suffix, tampered_role, tampered_path) in [
        ("role", Some("disjoint"), None),
        ("path", None, Some("C:/game/Other.ini")),
    ] {
        let storage = SqliteStorage::in_memory().expect("storage");
        let game_id =
            renderpilot_domain::GameId::new(format!("game:renodx-cas-{suffix}")).expect("game id");
        let id = format!("renodx-cas-{suffix}");
        begin(&storage, &id, &game_id, renderpilot_domain::RENODX_INSTALL);
        let runtime = PeerStorageRuntime::new(storage);
        let after = super::main_after(&game_id);
        let manifest = install_manifest(&id);
        let permit = finish_install(&runtime, &id, &game_id, &after, &manifest, None)
            .expect("typed preparation");
        let before = runtime
            .repositories()
            .with_transaction(|transaction| {
                super::super::super::permit::fingerprint::read_file_fingerprint(transaction, &id)
            })
            .expect("read original fingerprint")
            .expect("original fingerprint");

        let mut tampered: serde_json::Value = serde_json::from_str(&manifest).expect("manifest");
        if let Some(role) = tampered_role {
            tampered["peer_program"]["endpoints"][1]["role"] = json!(role);
        }
        if let Some(path) = tampered_path {
            tampered["peer_program"]["endpoints"][1]["path"] = json!(path);
        }
        let tampered = serde_json::to_string(&tampered).expect("tampered manifest");
        runtime
            .repositories()
            .with_transaction(|transaction| {
                transaction
                    .execute(
                        "UPDATE pending_file_mutations
                         SET manifest_json = ?1
                         WHERE id = ?2",
                        rusqlite::params![tampered, id],
                    )
                    .map_err(crate::error::storage_error)?;
                Ok(())
            })
            .expect("tamper manifest");
        let after_tamper = runtime
            .repositories()
            .with_transaction(|transaction| {
                super::super::super::permit::fingerprint::read_file_fingerprint(transaction, &id)
            })
            .expect("read tampered fingerprint")
            .expect("tampered fingerprint");
        assert_ne!(before.manifest_sha256, after_tamper.manifest_sha256);

        assert!(
            runtime
                .seal_and_commit_ordinary_peer(permit, typed_evidence(&manifest), Vec::new())
                .is_err()
        );
        assert_eq!(
            runtime
                .repositories()
                .get_pending_file_mutation(&id)
                .expect("row")
                .expect("row exists")
                .state,
            PendingFileMutationState::Prepared
        );
    }
}

#[test]
fn typed_commit_materializes_and_persists_the_planned_topology() {
    let storage = SqliteStorage::in_memory().expect("storage");
    let game_id = renderpilot_domain::GameId::new("game:renodx-topology").expect("game id");
    let id = "renodx-topology";
    begin(&storage, id, &game_id, renderpilot_domain::RENODX_INSTALL);
    storage
        .upsert_game(&GameInstallation::new(
            GameIdentity::new(game_id.clone(), "RenoDX topology", Launcher::Steam)
                .expect("game identity"),
            Platform::Windows,
            GameRuntime::NativeWindows,
            super::path(super::ROOT),
        ))
        .expect("game projection");
    let runtime = PeerStorageRuntime::new(storage);
    let after = super::main_after(&game_id);
    let manifest = install_manifest(id);
    let authority = super::authority(renderpilot_domain::RENODX_INSTALL);
    let topology = super::topology(&game_id);
    runtime
        .repositories()
        .with_transaction(|transaction| {
            transaction
                .execute(
                    "INSERT INTO game_proxy_topologies
                     (id, game_id, topology_json, created_at, updated_at)
                     VALUES (?1, ?2, ?3, 1, 1)",
                    rusqlite::params![
                        topology.id,
                        game_id.as_str(),
                        serde_json::to_string(&topology).expect("topology json"),
                    ],
                )
                .map_err(crate::error::storage_error)?;
            Ok(())
        })
        .expect("topology projection");

    let program = super::super::super::manifest::parse_peer_program(
        &super::super::super::manifest::parse_manifest(&manifest, "test").expect("manifest"),
        "test",
    )
    .expect("program");
    let planned = PlannedGameProxyTopology::Exact(topology.clone());
    let requirements = required_read_guards_with_renodx_reshade_ini(
        None,
        Some(&after),
        Some(&topology),
        Some(&planned),
        ProxyPeerRoute::DurableDisjoint,
        program.intents(),
        &authority,
    )
    .expect("topology guard requirements");
    let initial_read_guards = guard_evidence(&requirements);
    let permit = runtime
        .finish_file_peer_preparation(PeerCommitPreparation {
            mutation_id: id,
            game_id: &game_id,
            feature: renderpilot_domain::RENODX_INSTALL,
            subject_id: None,
            manifest_json: &manifest,
            canonical_game_root: super::ROOT,
            initial_read_guards: &initial_read_guards,
            before_peer: None,
            after_peer: Some(&after),
            before_topology: Some(&topology),
            planned_after_topology: Some(&planned),
            route: ProxyPeerRoute::DurableDisjoint,
            component_set: None,
            baseline_mutations: &[],
            catalog_claim: None,
            renodx_reshade_ini: Some(&authority),
        })
        .expect("typed topology preparation");
    let evidence = vec![
        PeerEndpointEvidence::new(
            program.intents()[0].clone(),
            None,
            Some(
                PeerFileImage::new("addon-after", super::hash(super::BEFORE_DIGEST), 8)
                    .expect("image"),
            ),
        ),
        PeerEndpointEvidence::new(
            program.intents()[1].clone(),
            None,
            Some(
                PeerFileImage::new("ini-after", super::hash(super::AFTER_DIGEST), 4)
                    .expect("image"),
            ),
        ),
    ];

    runtime
        .seal_and_commit_ordinary_peer(permit, evidence, initial_read_guards)
        .expect("typed topology commit");
    assert_eq!(
        runtime
            .repositories()
            .get_proxy_topology(&game_id)
            .expect("topology")
            .expect("topology exists"),
        topology
    );
}

fn guard_evidence(requirements: &[PeerReadGuardRequirement]) -> Vec<PeerReadGuardEvidence> {
    requirements
        .iter()
        .map(|requirement| {
            let observed = match requirement.expectation() {
                PeerReadGuardExpectation::Absent => None,
                PeerReadGuardExpectation::Digest { sha256 } => {
                    Some(PeerFileImage::new("guard-digest", sha256.clone(), 4).expect("image"))
                }
                PeerReadGuardExpectation::Receipt { identity, sha256 } => {
                    Some(PeerFileImage::new(identity.clone(), sha256.clone(), 4).expect("image"))
                }
            };
            PeerReadGuardEvidence::new(requirement.path().clone(), observed)
        })
        .collect()
}

fn typed_evidence(manifest: &str) -> Vec<PeerEndpointEvidence> {
    let program = super::super::super::manifest::parse_peer_program(
        &super::super::super::manifest::parse_manifest(manifest, "test").expect("manifest"),
        "test",
    )
    .expect("program");
    vec![
        PeerEndpointEvidence::new(
            program.intents()[0].clone(),
            None,
            Some(
                PeerFileImage::new("addon-after", super::hash(super::BEFORE_DIGEST), 8)
                    .expect("image"),
            ),
        ),
        PeerEndpointEvidence::new(
            program.intents()[1].clone(),
            None,
            Some(
                PeerFileImage::new("ini-after", super::hash(super::AFTER_DIGEST), 4)
                    .expect("image"),
            ),
        ),
    ]
}
