use renderpilot_domain::{AddonKind, GameId, InstalledAddon, PathRef};

use super::ensure_guard_matches_record;
use crate::game_mutation_lock::try_lock;

fn record(id: &str) -> InstalledAddon {
    InstalledAddon::new(
        GameId::new(id).expect("game id"),
        AddonKind::Luma,
        PathRef::new(format!("C:/Games/{id}/Luma.addon64")).expect("path"),
    )
}

#[test]
fn guard_image_mismatch_is_rejected_before_execution() {
    let expected = record("execute-expected");
    let guard = try_lock(&GameId::new("execute-other").expect("game id")).expect("lock");

    assert!(ensure_guard_matches_record(&guard, &expected).is_err());
}

#[test]
fn matching_guard_image_is_accepted() {
    let expected = record("execute-matching");
    let guard = try_lock(expected.game_id()).expect("lock");

    ensure_guard_matches_record(&guard, &expected).expect("matching image");
}
