use renderpilot_domain::PathRef;
use serde_json::{Value, json};

use super::super::super::shared_roots::bind_persisted_json;
use super::path;

const GAME_ROOT: &str = "C:/game";
const SHARED_ROOT: &str = "C:/shared";

fn roots_json(entries: &[(&str, &str, &str)]) -> String {
    json!({
        "version": 1,
        "roots": entries.iter().map(|(id, kind, canonical_path)| json!({
            "id": id,
            "kind": kind,
            "canonical_path": canonical_path,
        })).collect::<Vec<_>>(),
    })
    .to_string()
}

fn valid_roots() -> Value {
    json!({
        "version": 1,
        "roots": [
            {"id": "game-0", "kind": "game", "canonical_path": GAME_ROOT},
            {"id": "shared", "kind": "shared_vulkan", "canonical_path": SHARED_ROOT}
        ]
    })
}

fn bind(value: &Value, program_roots: &[&str]) -> renderpilot_application::AppResult<()> {
    bind_persisted_json(
        &value.to_string(),
        &program_roots
            .iter()
            .map(|root| (*root).to_owned())
            .collect::<Vec<_>>(),
    )
    .map(|_| ())
}

#[test]
fn shared_root_binder_accepts_game_zero_and_shared_in_exact_order() {
    let roots = bind_persisted_json(
        &roots_json(&[
            ("game-0", "game", GAME_ROOT),
            ("shared", "shared_vulkan", SHARED_ROOT),
        ]),
        &[GAME_ROOT.to_owned(), SHARED_ROOT.to_owned()],
    )
    .expect("roots");
    assert_eq!(roots.canonical_game_root(), Some(&path(GAME_ROOT)));
}

#[test]
fn shared_root_binder_accepts_shared_only_legacy_shape_without_game_authority() {
    let roots = bind_persisted_json(
        &roots_json(&[("shared", "shared_vulkan", SHARED_ROOT)]),
        &[SHARED_ROOT.to_owned()],
    )
    .expect("shared-only roots");
    assert_eq!(roots.canonical_game_root(), None);
}

#[test]
fn shared_root_binder_rejects_shape_order_path_and_program_mismatches() {
    let valid = valid_roots();
    let cases = [
        ("missing-version", json!({"roots": valid["roots"].clone()})),
        ("missing-roots", json!({"version": 1})),
        (
            "version",
            json!({"version": 2, "roots": valid["roots"].clone()}),
        ),
        (
            "unknown-top-level-key",
            json!({"version": 1, "roots": valid["roots"].clone(), "extra": true}),
        ),
        (
            "missing-root-key",
            json!({"version": 1, "roots": [{"id": "game-0", "kind": "game"}, valid["roots"][1].clone()]}),
        ),
        (
            "unknown-root-key",
            json!({"version": 1, "roots": [{"id": "game-0", "kind": "game", "canonical_path": GAME_ROOT, "extra": true}, valid["roots"][1].clone()]}),
        ),
        (
            "unknown-kind",
            json!({"version": 1, "roots": [{"id": "game-0", "kind": "other", "canonical_path": GAME_ROOT}, valid["roots"][1].clone()]}),
        ),
        (
            "nonabsolute",
            json!({"version": 1, "roots": [{"id": "game-0", "kind": "game", "canonical_path": "game"}, valid["roots"][1].clone()]}),
        ),
        (
            "dot",
            json!({"version": 1, "roots": [{"id": "game-0", "kind": "game", "canonical_path": "C:/game/./nested"}, valid["roots"][1].clone()]}),
        ),
        (
            "duplicate-separator",
            json!({"version": 1, "roots": [{"id": "game-0", "kind": "game", "canonical_path": "C:/game//nested"}, valid["roots"][1].clone()]}),
        ),
        (
            "game-id",
            json!({"version": 1, "roots": [{"id": "game-1", "kind": "game", "canonical_path": GAME_ROOT}, valid["roots"][1].clone()]}),
        ),
        (
            "noncontiguous-game-id",
            json!({"version": 1, "roots": [{"id": "game-0", "kind": "game", "canonical_path": GAME_ROOT}, {"id": "game-2", "kind": "game", "canonical_path": "C:/other"}, valid["roots"][1].clone()]}),
        ),
        (
            "missing-shared-root",
            json!({"version": 1, "roots": [{"id": "game-0", "kind": "game", "canonical_path": GAME_ROOT}]}),
        ),
        (
            "duplicate-shared-root",
            json!({"version": 1, "roots": [valid["roots"][0].clone(), valid["roots"][1].clone(), {"id": "shared", "kind": "shared_vulkan", "canonical_path": "C:/other"}]}),
        ),
        (
            "shared-order",
            json!({"version": 1, "roots": [valid["roots"][1].clone(), valid["roots"][0].clone()]}),
        ),
        (
            "overlap",
            json!({"version": 1, "roots": [{"id": "game-0", "kind": "game", "canonical_path": GAME_ROOT}, {"id": "shared", "kind": "shared_vulkan", "canonical_path": "C:/game/shared"}]}),
        ),
    ];
    for (name, value) in cases {
        assert!(
            bind(&value, &[GAME_ROOT, SHARED_ROOT]).is_err(),
            "case {name} must reject"
        );
    }
    assert!(bind(&valid, &[SHARED_ROOT, GAME_ROOT]).is_err());
    assert!(bind(&valid, &[GAME_ROOT, SHARED_ROOT, "C:/extra"]).is_err());
}

#[test]
fn shared_root_binder_keeps_exact_path_identity() {
    let roots = bind_persisted_json(
        &roots_json(&[
            ("game-0", "game", "C:/game"),
            ("shared", "shared_vulkan", "C:/other"),
        ]),
        &["C:/game".to_owned(), "C:/other".to_owned()],
    )
    .expect("disjoint roots");
    assert_eq!(
        roots.canonical_game_root(),
        Some(&PathRef::new("C:/game").unwrap())
    );
}
