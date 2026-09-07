use renderpilot_domain::{PeerEndpointRole, RENODX_INSTALL};
use serde_json::{Value, json};

use super::validate_shared_peer_recovery_program;

const OWNER: &str = "shared-mutation-1";
const AFTER: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn roots_json(entries: &[(&str, &str, &str)]) -> String {
    json!({
        "version": 1,
        "roots": entries.iter().map(|(id, kind, path)| json!({
            "id": id,
            "kind": kind,
            "canonical_path": path,
        })).collect::<Vec<_>>(),
    })
    .to_string()
}

fn manifest(
    owner: &str,
    feature: &str,
    roots: &[&str],
    role: PeerEndpointRole,
    path: &str,
    guard: &str,
    execution_class: &str,
) -> Value {
    let role = match role {
        PeerEndpointRole::Disjoint => "disjoint",
        PeerEndpointRole::TopologyDownstream => "topology_downstream",
        PeerEndpointRole::RenoDxReshadeIni => "renodx_reshade_ini",
        PeerEndpointRole::OptiScalerConfig => "optiscaler_config",
        PeerEndpointRole::DlssFix => "dlss_fix",
    };
    json!({
        "version": 1,
        "scope": "game_shared",
        "game_id": "game:shared-recovery",
        "feature": feature,
        "files": [],
        "registry": [],
        "directories": [],
        "peer_program": {
            "format": 1,
            "transaction_owner": owner,
            "execution_class": execution_class,
            "roots": roots,
            "stage": [],
            "custody": [],
            "created_ancestors": [],
            "endpoints": [{
                "ordinal": 0,
                "path": path,
                "role": role,
                "operation": "create",
                "planned_sha256": AFTER,
                "planned_length": 4,
                "before": null,
                "read_guards": [guard],
                "subtree_publishes": []
            }]
        }
    })
}

fn encode(value: &Value) -> String {
    serde_json::to_string(value).expect("manifest JSON")
}

fn game_shared_roots() -> String {
    roots_json(&[
        ("game-0", "game", "C:/game"),
        ("shared", "shared_vulkan", "C:/shared"),
    ])
}

#[test]
fn full_shared_typed_game_zero_manifest_is_accepted() {
    let manifest = manifest(
        OWNER,
        RENODX_INSTALL,
        &["C:/game", "C:/shared"],
        PeerEndpointRole::RenoDxReshadeIni,
        "C:/game/ReShade.ini",
        "C:/game:reshade.ini",
        "shared",
    );
    validate_shared_peer_recovery_program(
        OWNER,
        RENODX_INSTALL,
        &encode(&manifest),
        &game_shared_roots(),
    )
    .expect("typed game-0 shared recovery");
}

#[test]
fn full_shared_typed_manifest_rejects_independent_tampering() {
    let base = manifest(
        OWNER,
        RENODX_INSTALL,
        &["C:/game", "C:/shared"],
        PeerEndpointRole::RenoDxReshadeIni,
        "C:/game/ReShade.ini",
        "C:/game:reshade.ini",
        "shared",
    );

    let mut feature = base.clone();
    feature["feature"] = json!("renodx.uninstall");
    assert!(
        validate_shared_peer_recovery_program(
            OWNER,
            RENODX_INSTALL,
            &encode(&feature),
            &game_shared_roots(),
        )
        .is_err()
    );

    let mut root_id = base.clone();
    root_id["peer_program"]["roots"] = json!(["game-1", "shared"]);
    assert!(
        validate_shared_peer_recovery_program(
            OWNER,
            RENODX_INSTALL,
            &encode(&root_id),
            &game_shared_roots(),
        )
        .is_err()
    );

    let mut root_path = game_shared_roots();
    root_path = root_path.replace("C:/game", "C:/other");
    assert!(
        validate_shared_peer_recovery_program(OWNER, RENODX_INSTALL, &encode(&base), &root_path,)
            .is_err()
    );

    let mut game_one_path = base.clone();
    game_one_path["peer_program"]["endpoints"][0]["path"] = json!("C:/game-1/ReShade.ini");
    assert!(
        validate_shared_peer_recovery_program(
            OWNER,
            RENODX_INSTALL,
            &encode(&game_one_path),
            &game_shared_roots(),
        )
        .is_err()
    );

    let mut shared_guard = base.clone();
    shared_guard["peer_program"]["endpoints"][0]["read_guards"] = json!(["shared:ReShade.ini"]);
    assert!(
        validate_shared_peer_recovery_program(
            OWNER,
            RENODX_INSTALL,
            &encode(&shared_guard),
            &game_shared_roots(),
        )
        .is_err()
    );

    let mut role = base.clone();
    role["peer_program"]["endpoints"][0]["role"] = json!("topology_downstream");
    assert!(
        validate_shared_peer_recovery_program(
            OWNER,
            RENODX_INSTALL,
            &encode(&role),
            &game_shared_roots(),
        )
        .is_err()
    );

    let mut owner = base.clone();
    owner["peer_program"]["transaction_owner"] = json!("other");
    assert!(
        validate_shared_peer_recovery_program(
            OWNER,
            RENODX_INSTALL,
            &encode(&owner),
            &game_shared_roots(),
        )
        .is_err()
    );

    let mut class = base;
    class["peer_program"]["execution_class"] = json!("ordinary");
    assert!(
        validate_shared_peer_recovery_program(
            OWNER,
            RENODX_INSTALL,
            &encode(&class),
            &game_shared_roots(),
        )
        .is_err()
    );
}

#[test]
fn shared_only_untyped_recovery_accepts_and_typed_recovery_rejects() {
    let roots = roots_json(&[("shared", "shared_vulkan", "C:/shared")]);
    let untyped = manifest(
        OWNER,
        RENODX_INSTALL,
        &["C:/shared"],
        PeerEndpointRole::Disjoint,
        "C:/shared/RenoDx.addon64",
        "C:/shared:renodx.addon64",
        "shared",
    );
    validate_shared_peer_recovery_program(OWNER, RENODX_INSTALL, &encode(&untyped), &roots)
        .expect("shared-only untyped recovery");

    let typed = manifest(
        OWNER,
        RENODX_INSTALL,
        &["C:/shared"],
        PeerEndpointRole::RenoDxReshadeIni,
        "C:/shared/ReShade.ini",
        "C:/shared:reshade.ini",
        "shared",
    );
    assert!(
        validate_shared_peer_recovery_program(OWNER, RENODX_INSTALL, &encode(&typed), &roots,)
            .is_err()
    );
}
