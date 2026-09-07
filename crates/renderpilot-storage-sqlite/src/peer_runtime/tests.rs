use std::sync::Arc;

use serde_json::json;

use super::manifest::{parse_manifest, parse_peer_program};
use super::permit::{RuntimeInstance, ensure_runtime, read_file_fingerprint};
use super::validation::validate_file_manifest_binding;
use crate::{BeginFileMutationPreparation, PendingFileMutationState, SqliteStorage};
use renderpilot_application::InstalledAddonRepository;
use renderpilot_domain::{
    AddonKind, GameId, InstalledAddon, PathRef, PeerEndpointEvidence, PeerEndpointIntent,
    PeerEndpointRole, PeerFileImage, ProxyPeerRoute, Sha256Hash,
};

const DIGEST: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn valid_program() -> serde_json::Value {
    json!({
        "peer_program": {
            "format": 1,
            "transaction_owner": "mutation-1",
            "execution_class": "ordinary",
            "roots": ["C:/game"],
            "stage": ["C:/game:stage"],
            "custody": ["C:/game:custody"],
            "created_ancestors": ["C:/game:game"],
            "endpoints": [{
                "ordinal": 0,
                "path": "C:/game/dxgi.dll",
                "role": "disjoint",
                "operation": "replace",
                "planned_sha256": DIGEST,
                "planned_length": 3,
                "before": {
                    "identity": "file-1",
                    "sha256": DIGEST,
                    "length": 3
                },
                "read_guards": ["C:/game:dxgi"],
                "subtree_publishes": []
            }]
        }
    })
}

fn bound_ordinary_manifest() -> serde_json::Value {
    let mut value = valid_program();
    value["format_version"] = json!(1);
    value["roots"] = json!(["C:/game"]);
    value["peer_program"]["roots"] = json!(["C:/game"]);
    value["peer_program"]["stage"] = json!([]);
    value["peer_program"]["created_ancestors"] = json!([]);
    value["peer_program"]["endpoints"][0]["planned_length"] = json!(4);
    value["peer_program"]["custody"] = json!(["C:/game:dxgi.dll"]);
    value["peer_program"]["endpoints"][0]["read_guards"] = json!(["C:/game:dxgi.dll"]);
    value["snapshots"] = json!([{
        "path": "C:/game/dxgi.dll",
        "snapshot": "C:/transaction/0.before"
    }]);
    value
}

fn bound_nested_manifest() -> serde_json::Value {
    json!({
        "format_version": 1,
        "roots": ["C:/game"],
        "snapshots": [
            {"path": "C:/game/config/dlss/profile.json", "snapshot": null},
            {"path": "C:/game/config/other.json", "snapshot": null}
        ],
        "peer_ancestors": [
            {"path": "C:/game/config", "consumer_ordinals": [0, 1]},
            {"path": "C:/game/config/dlss", "consumer_ordinals": [0]}
        ],
        "peer_program": {
            "format": 1,
            "transaction_owner": "mutation-1",
            "execution_class": "ordinary",
            "roots": ["C:/game"],
            "stage": [],
            "custody": [],
            "created_ancestors": ["C:/game:config", "C:/game:config/dlss"],
            "endpoints": [
                {
                    "ordinal": 0,
                    "path": "C:/game/config/dlss/profile.json",
                    "role": "disjoint",
                    "operation": "create",
                    "planned_sha256": DIGEST,
                    "planned_length": 3,
                    "before": null,
                    "read_guards": ["C:/game:config/dlss/profile.json"],
                    "subtree_publishes": ["C:/game:config", "C:/game:config/dlss"]
                },
                {
                    "ordinal": 1,
                    "path": "C:/game/config/other.json",
                    "role": "disjoint",
                    "operation": "create",
                    "planned_sha256": DIGEST,
                    "planned_length": 3,
                    "before": null,
                    "read_guards": ["C:/game:config/other.json"],
                    "subtree_publishes": ["C:/game:config"]
                }
            ]
        }
    })
}

fn parse_and_validate(value: &serde_json::Value) -> bool {
    let manifest = serde_json::to_string(value).expect("manifest");
    let value = parse_manifest(&manifest, "test").expect("manifest");
    let program = parse_peer_program(&value, "test").expect("program");
    validate_file_manifest_binding(&value, &program).is_ok()
}

#[test]
fn strict_program_requires_format_one() {
    let mut value = valid_program();
    value["peer_program"]["format"] = json!(2);
    assert!(parse_peer_program(&value, "test").is_err());
}

#[test]
fn strict_program_rejects_unknown_fields_and_execution_classes() {
    let mut unknown = valid_program();
    unknown["peer_program"]["unexpected"] = json!(true);
    assert!(parse_peer_program(&unknown, "test").is_err());

    let mut class = valid_program();
    class["peer_program"]["execution_class"] = json!("ordinary_v2");
    assert!(parse_peer_program(&class, "test").is_err());
}

#[test]
fn scope_validators_reject_the_wrong_execution_class() {
    let ordinary = serde_json::to_string(&valid_program()).expect("ordinary program");
    assert!(super::super::validate_file_peer_program_manifest(&ordinary).is_ok());
    assert!(super::super::validate_shared_peer_program_manifest(&ordinary).is_err());

    let mut shared = valid_program();
    shared["peer_program"]["execution_class"] = json!("shared");
    let shared = serde_json::to_string(&shared).expect("shared program");
    assert!(super::super::validate_file_peer_program_manifest(&shared).is_err());
    assert!(super::super::validate_shared_peer_program_manifest(&shared).is_ok());
}

#[test]
fn strict_program_rejects_duplicate_or_unscoped_auxiliary_paths() {
    let mut duplicate = valid_program();
    duplicate["peer_program"]["stage"] = json!(["game:root:stage", "game:root:stage"]);
    assert!(parse_peer_program(&duplicate, "test").is_err());

    let mut unscoped = valid_program();
    unscoped["peer_program"]["custody"] = json!(["other:root:custody"]);
    assert!(parse_peer_program(&unscoped, "test").is_err());
}

#[test]
fn strict_program_rejects_after_identity_and_invalid_endpoint_order() {
    let mut after = valid_program();
    after["peer_program"]["endpoints"][0]["after"] = json!({
        "identity": "post",
        "sha256": DIGEST,
        "length": 3
    });
    assert!(parse_peer_program(&after, "test").is_err());

    let mut ordinal = valid_program();
    ordinal["peer_program"]["endpoints"][0]["ordinal"] = json!(1);
    assert!(parse_peer_program(&ordinal, "test").is_err());
}

#[test]
fn file_binding_rejects_root_divergence() {
    let mut value = bound_ordinary_manifest();
    value["roots"] = json!(["C:/other"]);
    let manifest = serde_json::to_string(&value).expect("manifest");
    let value = parse_manifest(&manifest, "test").expect("manifest");
    let program = parse_peer_program(&value, "test").expect("program");
    assert!(validate_file_manifest_binding(&value, &program).is_err());
}

#[test]
fn file_binding_rejects_orphan_auxiliary_or_read_guard_capability() {
    let mut stage = bound_ordinary_manifest();
    stage["peer_program"]["stage"] = json!(["C:/game:orphan"]);
    let stage = serde_json::to_string(&stage).expect("stage manifest");
    let value = parse_manifest(&stage, "test").expect("stage manifest");
    let program = parse_peer_program(&value, "test").expect("stage program");
    assert!(validate_file_manifest_binding(&value, &program).is_err());

    let mut guard = bound_ordinary_manifest();
    guard["peer_program"]["endpoints"][0]["read_guards"] = json!(["C:/game:sibling.dll"]);
    let guard = serde_json::to_string(&guard).expect("guard manifest");
    let value = parse_manifest(&guard, "test").expect("guard manifest");
    let program = parse_peer_program(&value, "test").expect("guard program");
    assert!(validate_file_manifest_binding(&value, &program).is_err());
}

#[test]
fn file_binding_accepts_manifest_without_ancestor_projection() {
    assert!(parse_and_validate(&bound_ordinary_manifest()));
}

#[test]
fn file_binding_accepts_nested_shared_ancestors_and_rejects_projection_drift() {
    let valid = bound_nested_manifest();
    assert!(parse_and_validate(&valid));

    for mutation in [
        |value: &mut serde_json::Value| {
            value["peer_ancestors"]
                .as_array_mut()
                .expect("ancestors")
                .pop();
        },
        |value: &mut serde_json::Value| {
            value["peer_ancestors"] = json!([
                {"path": "C:/game/config/dlss", "consumer_ordinals": [0]},
                {"path": "C:/game/config", "consumer_ordinals": [0, 1]}
            ]);
        },
        |value: &mut serde_json::Value| {
            value["peer_ancestors"] = json!([
                {"path": "C:/game/config", "consumer_ordinals": [0, 1]},
                {"path": "C:/game/config/dlss", "consumer_ordinals": [0]},
                {"path": "C:/game/config/extra", "consumer_ordinals": [0]}
            ]);
        },
    ] {
        let mut value = valid.clone();
        mutation(&mut value);
        assert!(!parse_and_validate(&value));
    }
}

#[test]
fn file_binding_rejects_invalid_ancestor_paths_and_consumers() {
    for ancestors in [
        json!([
            {"path": "C:/game/config", "consumer_ordinals": [0, 1]},
            {"path": "C:/game/config", "consumer_ordinals": [0]}
        ]),
        json!([
            {"path": "C:/other/config", "consumer_ordinals": [0, 1]},
            {"path": "C:/game/config/dlss", "consumer_ordinals": [0]}
        ]),
        json!([
            {"path": "C:/game", "consumer_ordinals": [0, 1]},
            {"path": "C:/game/config/dlss", "consumer_ordinals": [0]}
        ]),
        json!([
            {"path": "C:/game/config", "consumer_ordinals": [1, 0]},
            {"path": "C:/game/config/dlss", "consumer_ordinals": [0]}
        ]),
        json!([
            {"path": "C:/game/config", "consumer_ordinals": [0, 0]},
            {"path": "C:/game/config/dlss", "consumer_ordinals": [0]}
        ]),
        json!([
            {"path": "C:/game/config", "consumer_ordinals": [0, 2]},
            {"path": "C:/game/config/dlss", "consumer_ordinals": [0]}
        ]),
    ] {
        let mut value = bound_nested_manifest();
        value["peer_ancestors"] = ancestors;
        assert!(!parse_and_validate(&value));
    }

    let mut non_create = bound_nested_manifest();
    non_create["peer_program"]["endpoints"][1]["operation"] = json!("replace");
    non_create["peer_program"]["endpoints"][1]["before"] = json!({
        "identity": "file-2",
        "sha256": DIGEST,
        "length": 3
    });
    non_create["peer_program"]["custody"] = json!(["C:/game:config/other.json"]);
    non_create["peer_ancestors"][0]["consumer_ordinals"] = json!([0, 1]);
    assert!(!parse_and_validate(&non_create));
}

#[test]
fn file_binding_rejects_subtree_projection_drift_and_nonempty_stage() {
    for subtree in [
        json!([]),
        json!(["C:/game:config", "C:/game:config/dlss", "C:/game:extra"]),
        json!(["C:/game:config/dlss", "C:/game:config"]),
    ] {
        let mut value = bound_nested_manifest();
        value["peer_program"]["endpoints"][0]["subtree_publishes"] = subtree;
        assert!(!parse_and_validate(&value));
    }

    let mut stage = bound_nested_manifest();
    stage["peer_program"]["stage"] = json!(["C:/game:stage"]);
    assert!(!parse_and_validate(&stage));
}

#[test]
fn permit_runtime_identity_is_not_interchangeable() {
    let first = Arc::new(RuntimeInstance);
    let second = Arc::new(RuntimeInstance);
    assert!(ensure_runtime(&first, &first).is_ok());
    assert!(ensure_runtime(&first, &second).is_err());
}

#[test]
fn prepared_row_fingerprint_changes_when_manifest_changes() {
    let storage = SqliteStorage::in_memory().expect("storage");
    let game_id = GameId::new("game:peer-runtime").expect("game id");
    storage
        .begin_file_mutation_preparation(&BeginFileMutationPreparation {
            id: "mutation-1".to_owned(),
            game_id,
            feature: "luma_install".to_owned(),
            subject_id: None,
            initial_manifest_json: "{}".to_owned(),
        })
        .expect("reserve row");
    let before = storage
        .with_transaction(|transaction| read_file_fingerprint(transaction, "mutation-1"))
        .expect("read fingerprint")
        .expect("fingerprint exists");
    storage
        .with_transaction(|transaction| {
            transaction
                .execute(
                    "UPDATE pending_file_mutations SET manifest_json = '{\"changed\":true}' WHERE id = 'mutation-1'",
                    [],
                )
                .map_err(crate::error::storage_error)?;
            Ok(())
        })
        .expect("change manifest");
    let after = storage
        .with_transaction(|transaction| read_file_fingerprint(transaction, "mutation-1"))
        .expect("read changed fingerprint")
        .expect("changed fingerprint exists");
    assert_ne!(before.manifest_sha256, after.manifest_sha256);
    assert_eq!(before.rowid, after.rowid);
    assert_eq!(before.created_at, after.created_at);
}

const GENERIC_ROOT: &str = "C:/game";
const GENERIC_ADDON: &str = "C:/game/peer.addon64";
const GENERIC_REPAIR: &str = "C:/game/repaired.dll";

fn generic_hash(byte: char) -> Sha256Hash {
    Sha256Hash::new(byte.to_string().repeat(64)).expect("hash")
}

fn generic_path(value: &str) -> PathRef {
    PathRef::new(value).expect("path")
}

fn generic_peer(game_id: &GameId, extra_claim: Option<&str>) -> InstalledAddon {
    let peer = InstalledAddon::new(
        game_id.clone(),
        AddonKind::Luma,
        generic_path(GENERIC_ADDON),
    )
    .with_created_file(generic_path(GENERIC_REPAIR));
    match extra_claim {
        Some(path) => peer.with_created_file(generic_path(path)),
        None => peer,
    }
}

fn generic_create_intent(role: PeerEndpointRole, typed: bool) -> PeerEndpointIntent {
    PeerEndpointIntent::create(
        generic_path(GENERIC_REPAIR),
        role,
        typed.then(|| generic_hash('d')),
        typed.then_some(9),
    )
    .expect("create intent")
}

fn generic_manifest(
    mutation_id: &str,
    intent: &PeerEndpointIntent,
    before: Option<(&str, char, u64)>,
) -> String {
    let before = before.map(|(identity, digest, length)| {
        json!({
            "identity": identity,
            "sha256": generic_hash(digest).as_str(),
            "length": length,
        })
    });
    let role = match intent.role() {
        PeerEndpointRole::Disjoint => "disjoint",
        PeerEndpointRole::TopologyDownstream => "topology_downstream",
        PeerEndpointRole::RenoDxReshadeIni => "renodx_reshade_ini",
        PeerEndpointRole::OptiScalerConfig => "optiscaler_config",
        PeerEndpointRole::DlssFix => "dlss_fix",
    };
    let custody = if before.is_some() {
        vec![format!("{GENERIC_ROOT}:repaired.dll")]
    } else {
        Vec::new()
    };
    json!({
        "format_version": 1,
        "roots": [GENERIC_ROOT],
        "transaction_dir": format!("C:/transactions/{mutation_id}"),
        "snapshots": [{
            "path": GENERIC_REPAIR,
            "snapshot": before.as_ref().map(|_| format!("C:/transactions/{mutation_id}/before.bin")),
        }],
        "peer_program": {
            "format": 1,
            "transaction_owner": mutation_id,
            "execution_class": "ordinary",
            "roots": [GENERIC_ROOT],
            "stage": [],
            "custody": custody,
            "created_ancestors": [],
            "endpoints": [{
                "ordinal": 0,
                "path": GENERIC_REPAIR,
                "role": role,
                "operation": "create",
                "planned_sha256": intent.planned_sha256().map(renderpilot_domain::Sha256Hash::as_str),
                "planned_length": intent.planned_length(),
                "before": before,
                "read_guards": [format!("{GENERIC_ROOT}:repaired.dll")],
                "subtree_publishes": [],
            }]
        }
    })
    .to_string()
}

struct GenericClaimFixture {
    runtime: super::permit::PeerStorageRuntime,
    game_id: GameId,
    before_peer: InstalledAddon,
    after_peer: InstalledAddon,
    intent: PeerEndpointIntent,
    manifest_json: String,
    mutation_id: String,
}

fn generic_claim_fixture(
    mutation_id: &str,
    intent: PeerEndpointIntent,
    before_image: Option<(&str, char, u64)>,
    after_peer: InstalledAddon,
) -> GenericClaimFixture {
    let storage = SqliteStorage::in_memory().expect("storage");
    let game_id = GameId::new(format!("game:generic-claim-{mutation_id}")).expect("game id");
    let before_peer = generic_peer(&game_id, None);
    storage
        .upsert_installed_addon(&before_peer)
        .expect("seed peer");
    let before_peer = storage
        .get_installed_addon(&game_id)
        .expect("read peer")
        .expect("peer exists");
    storage
        .begin_file_mutation_preparation(&BeginFileMutationPreparation {
            id: mutation_id.to_owned(),
            game_id: game_id.clone(),
            feature: "luma_install".to_owned(),
            subject_id: None,
            initial_manifest_json: "{}".to_owned(),
        })
        .expect("reserve row");
    let manifest_json = generic_manifest(mutation_id, &intent, before_image);
    GenericClaimFixture {
        runtime: super::permit::PeerStorageRuntime::new(storage),
        game_id,
        before_peer,
        after_peer,
        intent,
        manifest_json,
        mutation_id: mutation_id.to_owned(),
    }
}

impl GenericClaimFixture {
    fn prepare(
        &self,
    ) -> renderpilot_application::AppResult<super::permit::PreparedPeerCommitPermit> {
        self.runtime
            .finish_file_peer_preparation(super::permit::PeerCommitPreparation {
                mutation_id: &self.mutation_id,
                game_id: &self.game_id,
                feature: "luma_install",
                subject_id: None,
                manifest_json: &self.manifest_json,
                canonical_game_root: GENERIC_ROOT,
                initial_read_guards: &[],
                before_peer: Some(&self.before_peer),
                after_peer: Some(&self.after_peer),
                before_topology: None,
                planned_after_topology: None,
                route: ProxyPeerRoute::DurableDisjoint,
                component_set: None,
                baseline_mutations: &[],
                catalog_claim: None,
                renodx_reshade_ini: None,
            })
    }

    fn row_state(&self) -> PendingFileMutationState {
        self.runtime
            .repositories()
            .get_pending_file_mutation(&self.mutation_id)
            .expect("row")
            .expect("row exists")
            .state
    }

    fn evidence(&self, digest: char, length: u64) -> PeerEndpointEvidence {
        PeerEndpointEvidence::new(
            self.intent.clone(),
            None,
            Some(PeerFileImage::new("post", generic_hash(digest), length).expect("postimage")),
        )
    }
}

#[test]
fn storage_rederives_stable_generic_create_and_seals_exact_postimage() {
    let game_id = GameId::new("game:generic-claim-generic-create-accept").expect("game id");
    let fixture = generic_claim_fixture(
        "generic-create-accept",
        generic_create_intent(PeerEndpointRole::Disjoint, true),
        None,
        generic_peer(&game_id, None),
    );
    let permit = fixture.prepare().expect("stable generic repair permit");
    fixture
        .runtime
        .seal_and_commit_ordinary_peer(permit, vec![fixture.evidence('d', 9)], vec![])
        .expect("exact generic repair commit");
    assert_eq!(fixture.row_state(), PendingFileMutationState::Committed);
}

#[test]
fn storage_rejects_generic_create_with_present_preimage() {
    let game_id = GameId::new("game:generic-claim-generic-create-present").expect("game id");
    let fixture = generic_claim_fixture(
        "generic-create-present",
        generic_create_intent(PeerEndpointRole::Disjoint, true),
        Some(("foreign", 'd', 9)),
        generic_peer(&game_id, None),
    );
    assert!(fixture.prepare().is_err());
    assert_eq!(fixture.row_state(), PendingFileMutationState::Preparing);
}

#[test]
fn storage_rejects_generic_create_without_digest_and_length() {
    let game_id = GameId::new("game:generic-claim-generic-create-untyped").expect("game id");
    let fixture = generic_claim_fixture(
        "generic-create-untyped",
        generic_create_intent(PeerEndpointRole::Disjoint, false),
        None,
        generic_peer(&game_id, None),
    );
    assert!(fixture.prepare().is_err());
    assert_eq!(fixture.row_state(), PendingFileMutationState::Preparing);
}

#[test]
fn storage_rejects_generic_create_with_wrong_role() {
    let game_id = GameId::new("game:generic-claim-generic-create-role").expect("game id");
    let fixture = generic_claim_fixture(
        "generic-create-role",
        generic_create_intent(PeerEndpointRole::TopologyDownstream, true),
        None,
        generic_peer(&game_id, None),
    );
    assert!(fixture.prepare().is_err());
    assert_eq!(fixture.row_state(), PendingFileMutationState::Preparing);
}

#[test]
fn storage_rejects_generic_create_with_wrong_game_identity() {
    let other_game = GameId::new("game:generic-create-other").expect("other game");
    let fixture = generic_claim_fixture(
        "generic-create-identity",
        generic_create_intent(PeerEndpointRole::Disjoint, true),
        None,
        generic_peer(&other_game, None),
    );
    assert!(fixture.prepare().is_err());
    assert_eq!(fixture.row_state(), PendingFileMutationState::Preparing);
}

#[test]
fn storage_rejects_generic_create_for_a_changed_peer_claim() {
    let game_id = GameId::new("game:generic-claim-generic-create-claim").expect("game id");
    let fixture = generic_claim_fixture(
        "generic-create-claim",
        generic_create_intent(PeerEndpointRole::Disjoint, true),
        None,
        generic_peer(&game_id, Some("C:/game/other.dll")),
    );
    assert!(fixture.prepare().is_err());
    assert_eq!(fixture.row_state(), PendingFileMutationState::Preparing);
}

#[test]
fn storage_rejects_generic_create_postimage_digest_mismatch_without_commit() {
    let game_id = GameId::new("game:generic-claim-generic-create-bad-digest").expect("game id");
    let fixture = generic_claim_fixture(
        "generic-create-bad-digest",
        generic_create_intent(PeerEndpointRole::Disjoint, true),
        None,
        generic_peer(&game_id, None),
    );
    let permit = fixture.prepare().expect("prepare permit");
    let error = fixture
        .runtime
        .seal_and_commit_ordinary_peer(permit, vec![fixture.evidence('e', 9)], vec![])
        .expect_err("digest mismatch must reject");
    assert!(error.to_string().contains("digest"));
    assert_eq!(fixture.row_state(), PendingFileMutationState::Prepared);
}

#[test]
fn storage_rejects_generic_create_postimage_length_mismatch_without_commit() {
    let game_id = GameId::new("game:generic-claim-generic-create-bad-length").expect("game id");
    let fixture = generic_claim_fixture(
        "generic-create-bad-length",
        generic_create_intent(PeerEndpointRole::Disjoint, true),
        None,
        generic_peer(&game_id, None),
    );
    let permit = fixture.prepare().expect("prepare permit");
    let error = fixture
        .runtime
        .seal_and_commit_ordinary_peer(permit, vec![fixture.evidence('d', 8)], vec![])
        .expect_err("length mismatch must reject");
    assert!(error.to_string().contains("length"));
    assert_eq!(fixture.row_state(), PendingFileMutationState::Prepared);
}
