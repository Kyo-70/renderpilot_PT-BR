use renderpilot_domain::{ManagedAddonFile, ManagedFileBaseline, PathRef, Sha256Hash};

use super::fixtures::{path, payload, record, run, run_invalid, source, temp};

#[test]
fn dependency_paths_are_unique_authorized_and_disjoint_from_payload_endpoints() {
    let root = temp();
    let before = record(root.path(), "Game.addon", &[], &[]);
    let dependency = path(root.path(), "dependency.dll");
    let alias = PathRef::new(dependency.as_str().replace('/', "\\")).expect("alias");
    let game_payload = payload("Game.addon", &[("Game.addon", b"new")]);

    assert!(
        run_invalid(
            root.path(),
            &before,
            game_payload.clone(),
            &[dependency, alias],
            &[source(false)],
        )
        .is_err()
    );
    assert!(
        run_invalid(
            root.path(),
            &before,
            game_payload,
            &[path(root.path(), "Game.addon")],
            &[source(false)],
        )
        .is_err()
    );

    let dependency_parent = path(root.path(), "Luma");
    assert!(
        run_invalid(
            root.path(),
            &before,
            payload("Luma/Game.addon", &[("Luma/Game.addon", b"new")]),
            &[dependency_parent],
            &[source(false)],
        )
        .is_err()
    );
}

#[test]
fn malformed_old_claim_maps_fail_closed_before_observation() {
    let root = temp();
    let duplicate = record(root.path(), "Game.addon", &["game.addon"], &[]);
    assert!(
        run_invalid(
            root.path(),
            &duplicate,
            payload("Game.addon", &[("Game.addon", b"new")]),
            &[],
            &[source(false)],
        )
        .is_err()
    );

    let root = temp();
    let backed_without_created = record(root.path(), "Game.addon", &[], &["Old.hlsl"]);
    assert!(
        run_invalid(
            root.path(),
            &backed_without_created,
            payload("Game.addon", &[("Game.addon", b"new")]),
            &[],
            &[source(false)],
        )
        .is_err()
    );
}

#[test]
fn managed_aliases_are_rejected_without_effects() {
    let root = temp();
    let digest = Sha256Hash::new("a".repeat(64)).expect("digest");
    let managed = path(root.path(), "Managed.dll");
    let before = record(root.path(), "Game.addon", &[], &[])
        .try_with_managed_files(vec![ManagedAddonFile::owned(
            managed,
            ManagedFileBaseline::Absent,
            digest,
        )])
        .expect("managed record");
    assert!(
        run_invalid(
            root.path(),
            &before,
            payload(
                "Game.addon",
                &[("Game.addon", b"new"), ("Managed.dll", b"managed")]
            ),
            &[],
            &[source(false)],
        )
        .is_err()
    );
}

#[test]
fn split_effective_root_rejects_remaining_game_root_claims() {
    let game = temp();
    let payload_root = temp();
    std::fs::write(
        game.path().join("ReShade.ini"),
        format!("[ADDON]\r\nAddonPath={}\r\n", payload_root.path().display()),
    )
    .expect("config");
    let before = record(game.path(), "Game.addon", &[], &[]);
    assert!(
        run_invalid(
            game.path(),
            &before,
            payload("Game.addon", &[("Game.addon", b"new")]),
            &[],
            &[source(false)],
        )
        .is_err()
    );

    let dependency = path(game.path(), "Game.addon");
    let before = record(game.path(), "Game.addon", &[], &[]);
    // A dependency is retained when it is outside the effective payload root;
    // it is not treated as a removed payload claim.
    let (projection, _) = run(
        game.path(),
        &before,
        payload("Game.addon", &[("Game.addon", b"new")]),
        &[dependency],
        &[source(false)],
    )
    .expect("split dependency");
    let (_, delta, _, _) = super::scenarios::projection_parts(projection);
    assert_eq!(delta.0.len(), 1);
    assert!(delta.1.is_empty());
}
