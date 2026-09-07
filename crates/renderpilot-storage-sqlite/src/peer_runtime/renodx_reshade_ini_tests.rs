use renderpilot_application::AppResult;
use renderpilot_domain::mutation_features::RENODX_UPDATE;
use renderpilot_domain::{PathRef, PeerEndpointRole, RENODX_INSTALL, RenoDxReshadeIniAuthority};
use serde_json::{Value, json};

use super::manifest::parse_peer_program;
use super::renodx_reshade_ini::{bind_preparation, bind_recovery};

const DIGEST: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const SECOND_DIGEST: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const ROOT: &str = "C:/game";
const INI: &str = "C:/game/ReShade.ini";

fn authority(root: &str) -> RenoDxReshadeIniAuthority {
    RenoDxReshadeIniAuthority::try_from_feature(RENODX_INSTALL, PathRef::new(root).expect("root"))
        .expect("authority")
}

fn manifest_with_role(role: &str, endpoint_path: &str) -> Value {
    json!({
        "format_version": 1,
        "roots": [ROOT],
        "snapshots": [{"path": endpoint_path, "snapshot": null}],
        "peer_program": {
            "format": 1,
            "transaction_owner": "mutation-1",
            "execution_class": "ordinary",
            "roots": [ROOT],
            "stage": [],
            "custody": [],
            "created_ancestors": [],
            "endpoints": [{
                "ordinal": 0,
                "path": endpoint_path,
                "role": role,
                "operation": "create",
                "planned_sha256": DIGEST,
                "planned_length": 4,
                "before": null,
                "read_guards": ["C:/game:reshade.ini"],
                "subtree_publishes": []
            }]
        }
    })
}

fn parsed(value: &Value) -> super::ParsedPeerProgram {
    parse_peer_program(value, "RenoDX test").expect("peer program")
}

fn json_manifest(value: &Value) -> String {
    serde_json::to_string(value).expect("manifest JSON")
}

fn assert_err<T>(result: AppResult<T>) {
    assert!(result.err().is_some(), "expected validation to fail");
}

#[test]
fn parser_accepts_typed_wire_and_seals_role_changes() {
    let disjoint = parsed(&manifest_with_role("disjoint", INI));
    let typed_wire = json_manifest(&manifest_with_role("renodx_reshade_ini", INI));
    let typed_value: Value = serde_json::from_str(&typed_wire).expect("typed wire");
    assert_eq!(
        typed_value["peer_program"]["endpoints"][0]["role"],
        "renodx_reshade_ini"
    );
    let typed = parsed(&typed_value);
    assert!(disjoint.renodx_reshade_ini_intent().is_none());
    assert_eq!(
        typed
            .renodx_reshade_ini_intent()
            .map(renderpilot_domain::PeerEndpointIntent::role),
        Some(PeerEndpointRole::RenoDxReshadeIni)
    );
    assert_ne!(disjoint.seal(), typed.seal());
}

#[test]
fn file_preflight_rejects_typed_snapshot_cardinality_mismatch() {
    let mut manifest = manifest_with_role("renodx_reshade_ini", INI);
    manifest["snapshots"] = json!([]);
    assert_err(
        super::validate_file_peer_program_manifest_with_renodx_reshade_ini(
            RENODX_INSTALL,
            ROOT,
            &authority(ROOT),
            &json_manifest(&manifest),
        ),
    );
}

#[test]
fn parser_rejects_unknown_and_duplicate_typed_roles() {
    assert!(parse_peer_program(&manifest_with_role("renodx", INI), "test").is_err());

    let mut duplicate = manifest_with_role("renodx_reshade_ini", INI);
    let endpoint = duplicate["peer_program"]["endpoints"][0].clone();
    let mut second = endpoint;
    second["ordinal"] = json!(1);
    second["path"] = json!("C:/game/other.ini");
    duplicate["peer_program"]["endpoints"]
        .as_array_mut()
        .expect("endpoints")
        .push(second);
    assert!(parse_peer_program(&duplicate, "test").is_err());
}

#[test]
fn unqualified_validators_reject_typed_programs() {
    let manifest = json_manifest(&manifest_with_role("renodx_reshade_ini", INI));
    assert_err(super::validate_peer_program_manifest(&manifest));
    assert_err(super::validate_file_peer_program_manifest(&manifest));
    assert_err(super::validate_shared_peer_program_manifest(&manifest));
}

fn optiscaler_config_recovery_manifest() -> Value {
    let mut value = manifest_with_role("optiscaler_config", "C:/game/OptiScaler.ini");
    value["transaction_dir"] = json!("C:/transaction/mutation-1");
    value["snapshots"] = json!([
        {"path": "C:/game/ReShade64.dll", "snapshot": null},
        {"path": "C:/game/OptiScaler.ini", "snapshot": "C:/transaction/mutation-1/1.before"}
    ]);
    value["peer_program"]["custody"] = json!(["C:/game:optiscaler.ini"]);
    let mut config = value["peer_program"]["endpoints"][0].clone();
    config["ordinal"] = json!(1);
    config["operation"] = json!("replace");
    config["planned_sha256"] = json!(SECOND_DIGEST);
    config["before"] = json!({
        "identity": "config-id",
        "sha256": DIGEST,
        "length": 4,
    });
    config["read_guards"] = json!(["C:/game:optiscaler.ini"]);
    let downstream = json!({
        "ordinal": 0,
        "path": "C:/game/ReShade64.dll",
        "role": "topology_downstream",
        "operation": "create",
        "planned_sha256": SECOND_DIGEST,
        "planned_length": 4,
        "before": null,
        "read_guards": ["C:/game:reshade64.dll"],
        "subtree_publishes": []
    });
    value["peer_program"]["endpoints"] = json!([downstream, config]);
    value
}

#[test]
fn optiscaler_config_is_preflight_closed_but_uses_ordinary_feature_bound_recovery() {
    let manifest = json_manifest(&optiscaler_config_recovery_manifest());

    assert_err(super::validate_file_peer_program_manifest(&manifest));
    assert!(
        super::validated_file_peer_recovery_program_for_feature(
            "mutation-1",
            RENODX_INSTALL,
            &manifest,
        )
        .expect("feature-bound ordinary recovery")
        .is_some()
    );
}

#[test]
fn optiscaler_config_recovery_rejects_unbound_feature_path_role_cardinality_and_order() {
    let value = optiscaler_config_recovery_manifest();
    let encoded = json_manifest(&value);
    assert!(super::validated_file_peer_recovery_program("mutation-1", &encoded).is_err());

    let wrong_feature = value.clone();
    assert!(
        super::validated_file_peer_recovery_program_for_feature(
            "mutation-1",
            RENODX_UPDATE,
            &json_manifest(&wrong_feature),
        )
        .is_err()
    );

    let mut wrong_path = value.clone();
    wrong_path["peer_program"]["endpoints"][1]["path"] = json!("C:/game/other.ini");
    wrong_path["snapshots"][1]["path"] = json!("C:/game/other.ini");
    assert!(
        super::validated_file_peer_recovery_program_for_feature(
            "mutation-1",
            RENODX_INSTALL,
            &json_manifest(&wrong_path),
        )
        .is_err()
    );

    let mut wrong_role = value.clone();
    wrong_role["peer_program"]["endpoints"][1]["role"] = json!("disjoint");
    assert!(
        super::validated_file_peer_recovery_program_for_feature(
            "mutation-1",
            RENODX_INSTALL,
            &json_manifest(&wrong_role),
        )
        .is_err()
    );

    let mut duplicate = value.clone();
    let mut second = duplicate["peer_program"]["endpoints"][1].clone();
    second["ordinal"] = json!(2);
    second["path"] = json!("C:/game/Other.ini");
    duplicate["peer_program"]["endpoints"]
        .as_array_mut()
        .expect("endpoints")
        .push(second);
    duplicate["snapshots"]
        .as_array_mut()
        .expect("snapshots")
        .push(
            json!({"path": "C:/game/Other.ini", "snapshot": "C:/transaction/mutation-1/2.before"}),
        );
    assert!(
        super::validated_file_peer_recovery_program_for_feature(
            "mutation-1",
            RENODX_INSTALL,
            &json_manifest(&duplicate),
        )
        .is_err()
    );

    let mut missing_downstream = value.clone();
    missing_downstream["peer_program"]["endpoints"] =
        json!([missing_downstream["peer_program"]["endpoints"][1].clone(),]);
    missing_downstream["peer_program"]["endpoints"][0]["ordinal"] = json!(0);
    missing_downstream["snapshots"] = json!([missing_downstream["snapshots"][1].clone(),]);
    assert!(
        super::validated_file_peer_recovery_program_for_feature(
            "mutation-1",
            RENODX_INSTALL,
            &json_manifest(&missing_downstream),
        )
        .is_err()
    );

    let mut wrong_order = value;
    wrong_order["peer_program"]["endpoints"] = json!([
        wrong_order["peer_program"]["endpoints"][1].clone(),
        wrong_order["peer_program"]["endpoints"][0].clone(),
    ]);
    wrong_order["peer_program"]["endpoints"][0]["ordinal"] = json!(0);
    wrong_order["peer_program"]["endpoints"][1]["ordinal"] = json!(1);
    wrong_order["snapshots"] = json!([
        wrong_order["snapshots"][1].clone(),
        wrong_order["snapshots"][0].clone(),
    ]);
    assert!(
        super::validated_file_peer_recovery_program_for_feature(
            "mutation-1",
            RENODX_INSTALL,
            &json_manifest(&wrong_order),
        )
        .is_err()
    );
}

#[test]
fn binder_enforces_none_and_authority_matrix() {
    let root = PathRef::new(ROOT).expect("root");
    let plain = parsed(&manifest_with_role("disjoint", INI));
    let typed = parsed(&manifest_with_role("renodx_reshade_ini", INI));
    let authority = authority(ROOT);

    assert_eq!(
        bind_preparation(RENODX_INSTALL, &root, &plain, None).expect("plain binding"),
        None
    );
    assert_err(bind_preparation(
        RENODX_INSTALL,
        &root,
        &plain,
        Some(&authority),
    ));
    assert_err(bind_preparation(RENODX_INSTALL, &root, &typed, None));
    assert_eq!(
        bind_preparation(RENODX_INSTALL, &root, &typed, Some(&authority)).expect("typed binding"),
        Some(authority.clone())
    );

    assert_eq!(
        bind_recovery(RENODX_INSTALL, &root, &plain).expect("plain recovery"),
        None
    );
    assert_eq!(
        bind_recovery(RENODX_INSTALL, &root, &typed).expect("typed recovery"),
        Some(authority)
    );
}

#[test]
fn file_preflight_accepts_exact_typed_authority() {
    let manifest = json_manifest(&manifest_with_role("renodx_reshade_ini", INI));
    super::validate_file_peer_program_manifest_with_renodx_reshade_ini(
        RENODX_INSTALL,
        ROOT,
        &authority(ROOT),
        &manifest,
    )
    .expect("typed file preflight");
}

#[test]
fn file_preflight_rejects_feature_root_path_and_authority_drift() {
    let manifest = json_manifest(&manifest_with_role("renodx_reshade_ini", INI));
    for (feature, root, supplied) in [
        (RENODX_UPDATE, ROOT, authority(ROOT)),
        (RENODX_INSTALL, "C:/other", authority(ROOT)),
        (RENODX_INSTALL, ROOT, authority("C:/other")),
    ] {
        assert_err(
            super::validate_file_peer_program_manifest_with_renodx_reshade_ini(
                feature, root, &supplied, &manifest,
            ),
        );
    }

    let missing = json_manifest(&manifest_with_role("disjoint", INI));
    assert_err(
        super::validate_file_peer_program_manifest_with_renodx_reshade_ini(
            RENODX_INSTALL,
            ROOT,
            &authority(ROOT),
            &missing,
        ),
    );

    let wrong_leaf = json_manifest(&manifest_with_role(
        "renodx_reshade_ini",
        "C:/game/ReShade.ini.bak",
    ));
    assert_err(
        super::validate_file_peer_program_manifest_with_renodx_reshade_ini(
            RENODX_INSTALL,
            ROOT,
            &authority(ROOT),
            &wrong_leaf,
        ),
    );
}

#[test]
fn file_preflight_rejects_nonabsolute_dot_duplicate_and_external_roots() {
    let manifest = manifest_with_role("renodx_reshade_ini", INI);
    for root in ["game", "C:/game/../game", "C:/other"] {
        assert_err(
            super::validate_file_peer_program_manifest_with_renodx_reshade_ini(
                RENODX_INSTALL,
                root,
                &authority(ROOT),
                &json_manifest(&manifest),
            ),
        );
    }

    let mut duplicate_roots = manifest.clone();
    duplicate_roots["roots"] = json!([ROOT, "c:/GAME"]);
    duplicate_roots["peer_program"]["roots"] = json!([ROOT, "c:/GAME"]);
    assert_err(
        super::validate_file_peer_program_manifest_with_renodx_reshade_ini(
            RENODX_INSTALL,
            ROOT,
            &authority(ROOT),
            &json_manifest(&duplicate_roots),
        ),
    );

    let mut external = manifest;
    external["roots"] = json!(["C:/other"]);
    external["peer_program"]["roots"] = json!(["C:/other"]);
    assert_err(
        super::validate_file_peer_program_manifest_with_renodx_reshade_ini(
            RENODX_INSTALL,
            ROOT,
            &authority(ROOT),
            &json_manifest(&external),
        ),
    );
}
