use serde_json::{Value, json};

use super::{
    validated_file_peer_recovery_program, validated_file_peer_recovery_program_for_feature,
};
use renderpilot_domain::{PeerEndpointOperation, PeerEndpointRole, RENODX_INSTALL};

const BEFORE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const AFTER: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn image(identity: &str, digest: &str, length: u64) -> Value {
    json!({"identity": identity, "sha256": digest, "length": length})
}

fn manifest(operation: &str) -> Value {
    let before_digest = (operation != "create").then_some(BEFORE);
    let planned_digest = (operation != "remove").then_some(AFTER);
    let planned_length = (operation != "remove").then_some(4_u64);
    let path = "C:/game/bin/dxgi.dll";
    let snapshot = (operation != "create").then_some("C:/transaction/0.before");
    let endpoint = json!({
        "ordinal": 0,
        "path": path,
        "role": "disjoint",
        "operation": operation,
        "planned_sha256": planned_digest,
        "planned_length": planned_length,
        "before": before_digest.map(|digest| image("before", digest, 3)),
        "read_guards": ["C:/game:bin/dxgi.dll"],
        "subtree_publishes": []
    });
    let peer_program = json!({
        "format": 1,
        "transaction_owner": "mutation-1",
        "execution_class": "ordinary",
        "roots": ["C:/game"],
        "stage": [],
        "custody": if before_digest.is_some() { json!(["C:/game:bin/dxgi.dll"]) } else { json!([]) },
        "created_ancestors": [],
        "endpoints": [endpoint]
    });

    json!({
        "format_version": 1,
        "roots": ["C:/game"],
        "transaction_dir": "C:/transaction/mutation-1",
        "snapshots": [{"path": path, "snapshot": snapshot}],
        "peer_program": peer_program
    })
}

fn encode(value: &Value) -> String {
    serde_json::to_string(value).expect("manifest JSON")
}

#[test]
fn ordinary_projection_preserves_create_replace_remove_images_and_snapshots() {
    for (operation, before, after, expected_snapshot) in [
        ("create", (None, None), (Some(AFTER), Some(4)), None),
        (
            "replace",
            (Some(BEFORE), Some(3)),
            (Some(AFTER), Some(4)),
            Some("C:/transaction/0.before"),
        ),
        (
            "remove",
            (Some(BEFORE), Some(3)),
            (None, None),
            Some("C:/transaction/0.before"),
        ),
    ] {
        let program =
            validated_file_peer_recovery_program("mutation-1", &encode(&manifest(operation)))
                .expect("valid ordinary projection")
                .expect("peer program");
        assert!(program.execution_class().is_ordinary());
        assert_eq!(program.roots(), ["C:/game"]);
        assert_eq!(program.transaction_dir(), "C:/transaction/mutation-1");
        let endpoint = &program.endpoints()[0];
        assert_eq!(endpoint.ordinal(), 0);
        assert_eq!(endpoint.path().as_str(), "C:/game/bin/dxgi.dll");
        assert_eq!(endpoint.operation(), operation_name(operation));
        assert_image(endpoint.before(), before.0, before.1);
        assert_image(endpoint.after(), after.0, after.1);
        if operation == "replace" {
            assert_eq!(endpoint.before().identity(), Some("before"));
            assert_eq!(endpoint.after().identity(), None);
        }
        assert_eq!(endpoint.snapshot_path(), expected_snapshot);
    }
}

#[test]
fn projection_rejects_owner_and_shape_tampering() {
    let mut owner = manifest("replace");
    owner["peer_program"]["transaction_owner"] = json!("other");
    assert!(validated_file_peer_recovery_program("mutation-1", &encode(&owner)).is_err());

    let mut path = manifest("replace");
    path["snapshots"][0]["path"] = json!("C:/game/other.dll");
    assert!(validated_file_peer_recovery_program("mutation-1", &encode(&path)).is_err());

    let mut cardinality = manifest("replace");
    cardinality["snapshots"] = json!([]);
    assert!(validated_file_peer_recovery_program("mutation-1", &encode(&cardinality)).is_err());
    let mut presence = manifest("replace");
    presence["snapshots"][0]["snapshot"] = Value::Null;
    assert!(validated_file_peer_recovery_program("mutation-1", &encode(&presence)).is_err());
}

#[test]
fn ancestor_binding_returns_exact_order_paths_and_consumers() {
    let mut value = manifest("create");
    value["peer_program"]["endpoints"][0]["path"] = json!("C:/game/config/dxgi.dll");
    value["snapshots"][0]["path"] = json!("C:/game/config/dxgi.dll");
    value["peer_program"]["endpoints"][0]["read_guards"] = json!(["C:/game:config/dxgi.dll"]);
    value["peer_program"]["created_ancestors"] = json!(["C:/game:config"]);
    value["peer_program"]["endpoints"][0]["subtree_publishes"] = json!(["C:/game:config"]);
    value["peer_ancestors"] = json!([
        {"path": "C:/game/config", "consumer_ordinals": [0]}
    ]);
    let program = validated_file_peer_recovery_program("mutation-1", &encode(&value))
        .expect("valid ancestor projection")
        .expect("peer program");
    assert_eq!(program.declared_ancestors().len(), 1);
    assert_eq!(program.declared_ancestors()[0].path(), "C:/game/config");
    assert_eq!(program.declared_ancestors()[0].consumer_ordinals(), [0]);

    for tampered in [
        json!([{"path": "C:/game/config/other", "consumer_ordinals": [0]}]),
        json!([{"path": "C:/game/config", "consumer_ordinals": [1]}]),
    ] {
        let mut changed = value.clone();
        changed["peer_ancestors"] = tampered;
        assert!(validated_file_peer_recovery_program("mutation-1", &encode(&changed)).is_err());
    }
}

#[test]
fn recovery_boundary_distinguishes_absence_malformed_and_shared_programs() {
    let ordinary = json!({"format_version": 1, "roots": ["C:/game"]});
    assert!(
        validated_file_peer_recovery_program("mutation-1", &encode(&ordinary))
            .expect("valid object without peer program")
            .is_none()
    );

    let malformed = json!({"format_version": 1, "peer_program": {}});
    assert!(validated_file_peer_recovery_program("mutation-1", &encode(&malformed)).is_err());

    let mut shared = manifest("replace");
    shared["peer_program"]["execution_class"] = json!("shared");
    assert!(validated_file_peer_recovery_program("mutation-1", &encode(&shared)).is_err());

    assert!(validated_file_peer_recovery_program("mutation-1", "[]").is_err());
}

#[test]
fn byte_identical_replace_is_rejected_as_recovery_ambiguous() {
    let mut value = manifest("replace");
    value["peer_program"]["endpoints"][0]["planned_sha256"] = json!(BEFORE);
    value["peer_program"]["endpoints"][0]["planned_length"] = json!(3);
    assert!(validated_file_peer_recovery_program("mutation-1", &encode(&value)).is_err());
}

fn typed_manifest() -> Value {
    let endpoint = json!({
        "ordinal": 0,
        "path": "C:/game/ReShade.ini",
        "role": "renodx_reshade_ini",
        "operation": "create",
        "planned_sha256": AFTER,
        "planned_length": 4,
        "before": null,
        "read_guards": ["C:/game:reshade.ini"],
        "subtree_publishes": []
    });
    let peer_program = json!({
        "format": 1,
        "transaction_owner": "mutation-1",
        "execution_class": "ordinary",
        "roots": ["C:/game"],
        "stage": [],
        "custody": [],
        "created_ancestors": [],
        "endpoints": [endpoint]
    });
    json!({
        "format_version": 1,
        "roots": ["C:/game"],
        "transaction_dir": "C:/transaction/mutation-1",
        "snapshots": [{"path": "C:/game/ReShade.ini", "snapshot": null}],
        "peer_program": peer_program
    })
}

#[test]
fn feature_aware_projection_binds_typed_role_and_authority() {
    let program = validated_file_peer_recovery_program_for_feature(
        "mutation-1",
        RENODX_INSTALL,
        &encode(&typed_manifest()),
    )
    .expect("valid typed recovery")
    .expect("peer program");
    assert_eq!(
        program.endpoints()[0].role(),
        PeerEndpointRole::RenoDxReshadeIni
    );
    let authority = program
        .renodx_reshade_ini_authority()
        .expect("typed authority");
    assert_eq!(authority.feature().as_feature(), RENODX_INSTALL);
    assert_eq!(authority.canonical_game_root().as_str(), "C:/game");
    assert_eq!(authority.ini_path().as_str(), "C:/game/ReShade.ini");
}

#[test]
fn untyped_projection_rejects_typed_endpoint_without_a_feature() {
    assert!(
        validated_file_peer_recovery_program("mutation-1", &encode(&typed_manifest())).is_err()
    );
}

fn operation_name(value: &str) -> PeerEndpointOperation {
    match value {
        "create" => PeerEndpointOperation::Create,
        "replace" => PeerEndpointOperation::Replace,
        "remove" => PeerEndpointOperation::Remove,
        _ => panic!("unknown operation"),
    }
}

fn assert_image(image: &super::PeerRecoveryImage, digest: Option<&str>, length: Option<u64>) {
    assert_eq!(image.is_absent(), digest.is_none());
    assert_eq!(image.is_file(), digest.is_some());
    assert_eq!(
        image.sha256().map(renderpilot_domain::Sha256Hash::as_str),
        digest
    );
    assert_eq!(image.length(), length);
}
