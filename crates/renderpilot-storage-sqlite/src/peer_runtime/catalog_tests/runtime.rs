use renderpilot_application::{
    ComponentRepository, InstalledAddonRepository, ProxyTopologyRepository,
};
use renderpilot_domain::{
    ComponentFile, ComponentRollbackBaseline, PeerFileImage, PeerReadGuardEvidence, Sha256Hash,
};

use super::fixtures::{FullFixture, OTHER, addon_without_timestamps};
use crate::PendingFileMutationState;

#[test]
fn peer_storage_runtime_commits_catalog_claim_as_one_exact_projection() {
    let fixture = FullFixture::new("catalog-success");
    let permit = fixture.prepare().expect("prepare permit");
    let before_row = fixture
        .runtime
        .repositories()
        .get_pending_file_mutation(&fixture.mutation_id)
        .expect("prepared row")
        .expect("prepared row exists");
    assert_eq!(before_row.state, PendingFileMutationState::Prepared);
    fixture
        .runtime
        .seal_and_commit_ordinary_peer(
            permit,
            fixture.endpoint_evidence(),
            fixture.read_guards.clone(),
        )
        .expect("commit");

    let row = fixture
        .runtime
        .repositories()
        .get_pending_file_mutation(&fixture.mutation_id)
        .expect("committed row")
        .expect("committed row exists");
    assert_eq!(row.state, PendingFileMutationState::Committed);
    assert_eq!(
        fixture
            .runtime
            .repositories()
            .list_components_for_game(&fixture.game_id)
            .expect("components"),
        fixture.claim.after_components()
    );
    assert_eq!(
        fixture
            .runtime
            .repositories()
            .get_component_backup(&fixture.changed_id)
            .expect("changed baseline"),
        None
    );
    assert_eq!(
        fixture
            .runtime
            .repositories()
            .get_component_backup(&fixture.stable_id)
            .expect("stable baseline"),
        None
    );
    assert_eq!(
        fixture
            .runtime
            .repositories()
            .get_proxy_topology(&fixture.game_id)
            .expect("topology"),
        Some(fixture.topology.clone())
    );
    let peer = fixture
        .runtime
        .repositories()
        .get_installed_addon(&fixture.game_id)
        .expect("peer")
        .expect("peer exists");
    assert_eq!(
        addon_without_timestamps(&peer),
        addon_without_timestamps(&fixture.peer)
    );
}

#[test]
fn peer_storage_runtime_rejects_catalog_baseline_drift_without_partial_commit() {
    let fixture = FullFixture::new("catalog-drift");
    let permit = fixture.prepare().expect("prepare permit");
    let changed = ComponentRollbackBaseline::new(vec![
        ComponentFile::new(renderpilot_domain::PathRef::new("C:/game/catalog.dll").expect("path"))
            .with_sha256(Sha256Hash::new(OTHER).expect("drift digest")),
    ]);
    let changed_json = serde_json::to_string(changed.files()).expect("changed baseline");
    fixture
        .runtime
        .repositories()
        .with_transaction(|transaction| {
            transaction
                .execute(
                    "UPDATE component_backups SET files_json = :files
                     WHERE component_id = :component_id",
                    rusqlite::named_params! {
                        ":files": changed_json,
                        ":component_id": fixture.changed_id.as_str(),
                    },
                )
                .map_err(crate::error::storage_error)?;
            Ok(())
        })
        .expect("drift baseline");
    let error = fixture
        .runtime
        .seal_and_commit_ordinary_peer(
            permit,
            fixture.endpoint_evidence(),
            fixture.read_guards.clone(),
        )
        .expect_err("baseline drift must fail closed");
    assert!(error.to_string().contains("changed"));
    let row = fixture
        .runtime
        .repositories()
        .get_pending_file_mutation(&fixture.mutation_id)
        .expect("row")
        .expect("row exists");
    assert_eq!(row.state, PendingFileMutationState::Prepared);
    assert_eq!(
        fixture
            .runtime
            .repositories()
            .list_components_for_game(&fixture.game_id)
            .expect("components"),
        fixture.claim.before_components()
    );
    assert_eq!(
        fixture
            .runtime
            .repositories()
            .get_component_backup(&fixture.changed_id)
            .expect("drifted baseline"),
        Some(changed)
    );
    assert_eq!(
        fixture
            .runtime
            .repositories()
            .get_proxy_topology(&fixture.game_id)
            .expect("topology"),
        Some(fixture.topology.clone())
    );
    let peer = fixture
        .runtime
        .repositories()
        .get_installed_addon(&fixture.game_id)
        .expect("peer")
        .expect("peer exists");
    assert_eq!(
        addon_without_timestamps(&peer),
        addon_without_timestamps(&fixture.before_peer)
    );
}

#[test]
fn peer_storage_runtime_rejects_catalog_guard_tampering_at_prepare() {
    for (index, mutation) in ["omitted", "extra", "reordered", "changed"]
        .into_iter()
        .enumerate()
    {
        let fixture = FullFixture::new(&format!("catalog-guards-{index}"));
        let mut guards = fixture.read_guards.clone();
        match mutation {
            "omitted" => {
                guards.pop();
            }
            "extra" => {
                guards.push(guards[0].clone());
            }
            "reordered" => {
                guards.swap(0, 1);
            }
            "changed" => {
                let position = guards
                    .iter()
                    .position(|guard| guard.path().as_str() == "C:/game/stable.dll")
                    .expect("guard");
                let requirement = fixture
                    .read_guards
                    .get(position)
                    .expect("guard")
                    .path()
                    .clone();
                guards[position] = PeerReadGuardEvidence::new(
                    requirement,
                    Some(
                        PeerFileImage::new(
                            "tampered",
                            Sha256Hash::new(OTHER).expect("tampered digest"),
                            8,
                        )
                        .expect("tampered image"),
                    ),
                );
            }
            _ => unreachable!(),
        }
        let error = fixture
            .prepare_with_guards(&guards)
            .expect_err("catalog guard tampering must fail");
        assert!(error.to_string().contains("peer"));
        let row = fixture
            .runtime
            .repositories()
            .get_pending_file_mutation(&fixture.mutation_id)
            .expect("row")
            .expect("row exists");
        assert_eq!(row.state, PendingFileMutationState::Preparing);
    }
}
