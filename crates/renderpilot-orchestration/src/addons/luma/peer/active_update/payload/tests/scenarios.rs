use std::path::Path;

use renderpilot_domain::{PathRef, TrackedSource, TrackedSourceRole};

use crate::addons::luma::peer::active_update::model::LumaActiveUpdateDlssInput;

use super::fixtures::{
    authority, path, payload, record, run, run_invalid, run_optional, run_preserve, source, temp,
};

type PayloadProjectionParts = (
    PathRef,
    (Vec<PathRef>, Vec<PathRef>, Vec<PathRef>, Vec<PathRef>),
    LumaActiveUpdateDlssInput,
    Option<super::super::super::model::LumaActiveUpdateMtime>,
);

pub(super) fn projection_parts(
    projection: super::super::super::model::PayloadProjection,
) -> PayloadProjectionParts {
    let (record, dlss, mtime) = projection.into_parts();
    let (main, delta) = record.into_parts();
    (main, delta.into_parts(), dlss, mtime)
}

fn assert_path_key(actual: &PathRef, expected: &PathRef) {
    assert_eq!(
        renderpilot_domain::normalized_path_key(actual.as_str()),
        renderpilot_domain::normalized_path_key(expected.as_str())
    );
}

fn assert_path_keys(actual: &[PathRef], expected: &[PathRef]) {
    assert_eq!(actual.len(), expected.len());
    for (actual, expected) in actual.iter().zip(expected) {
        assert_path_key(actual, expected);
    }
}

#[test]
fn preserve_is_a_zero_observation_zero_effect_projection() {
    let root = temp();
    let before = record(root.path(), "Game.addon", &[], &[]);
    let (projection, effects) = run_preserve(root.path(), &before).expect("preserve");
    let (main, delta, dlss, mtime) = projection_parts(projection);

    assert_eq!(main, *before.addon_file());
    assert!(delta.0.is_empty() && delta.1.is_empty() && delta.2.is_empty() && delta.3.is_empty());
    assert!(matches!(dlss, LumaActiveUpdateDlssInput::Preserve));
    assert!(mtime.is_none());
    assert!(effects.is_none());
}

#[test]
fn retained_created_only_equal_change_and_missing_follow_the_owned_matrix() {
    let equal = temp();
    std::fs::write(equal.path().join("Game.addon"), b"same").expect("live");
    let before = record(equal.path(), "Game.addon", &[], &[]);
    let (projection, effects) = run_optional(
        equal.path(),
        &before,
        payload("Game.addon", &[("Game.addon", b"same")]),
        &[],
        &[source(false)],
    )
    .expect("equal");
    let (_, delta, _, mtime) = projection_parts(projection);
    assert!(effects.is_none());
    assert!(delta.0.is_empty() && delta.1.is_empty());
    assert!(mtime.is_none());

    let changed = temp();
    std::fs::write(changed.path().join("Game.addon"), b"old").expect("live");
    let before = record(changed.path(), "Game.addon", &[], &[]);
    let (projection, effects) = run(
        changed.path(),
        &before,
        payload("Game.addon", &[("Game.addon", b"new")]),
        &[],
        &[source(false)],
    )
    .expect("changed");
    let (_, delta, _, mtime) = projection_parts(projection);
    assert!(delta.0.is_empty() && delta.1.is_empty());
    assert_eq!(mtime.expect("main mtime").path(), before.addon_file());
    assert_eq!(effects.program().endpoints().len(), 1);
    assert!(matches!(effects.payloads(), [Some(bytes)] if bytes == b"new"));

    let missing = temp();
    let before = record(missing.path(), "Game.addon", &[], &[]);
    let (projection, effects) = run(
        missing.path(),
        &before,
        payload("Game.addon", &[("Game.addon", b"repair")]),
        &[],
        &[source(false)],
    )
    .expect("missing");
    let (_, delta, _, mtime) = projection_parts(projection);
    assert!(delta.0.is_empty() && delta.1.is_empty());
    assert_eq!(mtime.expect("main mtime").path(), before.addon_file());
    assert_eq!(effects.program().endpoints().len(), 1);
}

#[test]
fn retained_created_and_backed_equal_change_and_missing_require_the_sidecar() {
    let equal = temp();
    std::fs::write(equal.path().join("Game.addon"), b"same").expect("live");
    std::fs::write(equal.path().join("Game.addon.bak"), b"original").expect("sidecar");
    let before = record(equal.path(), "Game.addon", &[], &["Game.addon"]);
    let (projection, effects) = run_optional(
        equal.path(),
        &before,
        payload("Game.addon", &[("Game.addon", b"same")]),
        &[],
        &[source(false)],
    )
    .expect("equal backed");
    let (_, delta, _, mtime) = projection_parts(projection);
    assert!(effects.is_none() && mtime.is_none());
    assert!(delta.0.is_empty() && delta.1.is_empty());

    let changed = temp();
    std::fs::write(changed.path().join("Game.addon"), b"old").expect("live");
    std::fs::write(changed.path().join("Game.addon.bak"), b"original").expect("sidecar");
    let before = record(changed.path(), "Game.addon", &[], &["Game.addon"]);
    let (projection, effects) = run(
        changed.path(),
        &before,
        payload("Game.addon", &[("Game.addon", b"new")]),
        &[],
        &[source(false)],
    )
    .expect("changed backed");
    let (_, delta, _, mtime) = projection_parts(projection);
    assert!(delta.0.is_empty() && delta.1.is_empty());
    assert!(mtime.is_some());
    assert_eq!(effects.program().endpoints().len(), 1);
    assert!(matches!(effects.payloads(), [Some(bytes)] if bytes == b"new"));

    let missing = temp();
    std::fs::write(missing.path().join("Game.addon.bak"), b"original").expect("sidecar");
    let before = record(missing.path(), "Game.addon", &[], &["Game.addon"]);
    let (projection, effects) = run(
        missing.path(),
        &before,
        payload("Game.addon", &[("Game.addon", b"repair")]),
        &[],
        &[source(false)],
    )
    .expect("missing backed");
    let (_, delta, _, mtime) = projection_parts(projection);
    assert!(delta.0.is_empty() && delta.1.is_empty());
    assert!(mtime.is_some());
    assert_eq!(effects.program().endpoints().len(), 1);
}

#[test]
fn new_absent_is_created_and_new_present_is_acquired() {
    let absent = temp();
    let before = record(absent.path(), "Old.addon", &[], &[]);
    std::fs::write(absent.path().join("Old.addon"), b"old").expect("old");
    let (projection, effects) = run(
        absent.path(),
        &before,
        payload("New.addon", &[("New.addon", b"new")]),
        &[],
        &[source(false)],
    )
    .expect("new absent");
    let (_, delta, _, _) = projection_parts(projection);
    assert_path_keys(&delta.0, &[path(absent.path(), "New.addon")]);
    assert_path_keys(&delta.1, &[path(absent.path(), "Old.addon")]);
    assert_eq!(effects.program().endpoints().len(), 2);

    let present = temp();
    let before = record(present.path(), "Old.addon", &[], &[]);
    std::fs::write(present.path().join("Old.addon"), b"old").expect("old");
    std::fs::write(present.path().join("New.addon"), b"foreign").expect("foreign");
    let (projection, effects) = run(
        present.path(),
        &before,
        payload("New.addon", &[("New.addon", b"new")]),
        &[],
        &[source(false)],
    )
    .expect("new present");
    let (_, delta, _, _) = projection_parts(projection);
    let new = path(present.path(), "New.addon");
    assert_path_keys(&delta.0, std::slice::from_ref(&new));
    assert_path_keys(&delta.2, std::slice::from_ref(&new));
    assert_eq!(effects.program().endpoints().len(), 3);
    assert_eq!(effects.payloads()[0], Some(b"foreign".to_vec()));
    assert_eq!(effects.payloads()[1], Some(b"new".to_vec()));
    assert_eq!(effects.payloads()[2], None);
}

#[test]
fn removed_created_only_and_backed_require_expected_live_images() {
    let created_only = temp();
    let before = record(created_only.path(), "Game.addon", &["Old.hlsl"], &[]);
    std::fs::write(created_only.path().join("Game.addon"), b"main").expect("main");
    std::fs::write(created_only.path().join("Old.hlsl"), b"old").expect("old");
    let (projection, effects) = run(
        created_only.path(),
        &before,
        payload("Game.addon", &[("Game.addon", b"main")]),
        &[],
        &[source(false)],
    )
    .expect("remove created");
    let (_, delta, _, _) = projection_parts(projection);
    assert_eq!(delta.1, vec![path(created_only.path(), "Old.hlsl")]);
    assert!(delta.3.is_empty());
    assert_eq!(effects.program().endpoints().len(), 1);

    let backed = temp();
    let before = record(backed.path(), "Game.addon", &["Old.hlsl"], &["Old.hlsl"]);
    std::fs::write(backed.path().join("Game.addon"), b"main").expect("main");
    std::fs::write(backed.path().join("Old.hlsl"), b"current").expect("old");
    std::fs::write(backed.path().join("Old.hlsl.bak"), b"baseline").expect("sidecar");
    let (projection, effects) = run(
        backed.path(),
        &before,
        payload("Game.addon", &[("Game.addon", b"main")]),
        &[],
        &[source(false)],
    )
    .expect("release backed");
    let (_, delta, _, _) = projection_parts(projection);
    assert_eq!(delta.1, vec![path(backed.path(), "Old.hlsl")]);
    assert_eq!(delta.3, vec![path(backed.path(), "Old.hlsl")]);
    assert_eq!(effects.program().endpoints().len(), 2);
    assert_eq!(effects.payloads()[0], Some(b"baseline".to_vec()));
    assert_eq!(effects.payloads()[1], None);
}

#[test]
fn rename_is_removal_plus_new_acquisition_and_preserves_claim_spelling() {
    let root = temp();
    let before = record(root.path(), "Old.addon", &[], &[]);
    std::fs::write(root.path().join("Old.addon"), b"old").expect("old");
    std::fs::write(root.path().join("New.addon"), b"foreign").expect("foreign");
    let (projection, effects) = run(
        root.path(),
        &before,
        payload("New.addon", &[("New.addon", b"new")]),
        &[],
        &[source(false)],
    )
    .expect("rename");
    let (main, delta, _, mtime) = projection_parts(projection);
    assert_path_key(&main, &path(root.path(), "New.addon"));
    assert_path_keys(&delta.0, &[path(root.path(), "New.addon")]);
    assert_path_keys(&delta.1, &[path(root.path(), "Old.addon")]);
    assert_path_keys(&delta.2, &[path(root.path(), "New.addon")]);
    assert_path_key(
        mtime.expect("new main mtime").path(),
        &path(root.path(), "New.addon"),
    );
    assert_eq!(effects.program().endpoints().len(), 3);
}

#[test]
fn full_payload_requires_one_non_advisory_exact_source_and_no_advisory_extra() {
    let root = temp();
    let before = record(root.path(), "Game.addon", &[], &[]);
    let good_payload = payload("Game.addon", &[("Game.addon", b"new")]);

    let cases = [
        (Vec::<TrackedSource>::new(), "zero sources"),
        (vec![source(true)], "advisory source"),
        (
            vec![source(false), source(false)],
            "duplicate authoritative sources",
        ),
        (vec![source(false), source(true)], "advisory extra"),
        (
            vec![TrackedSource::new(
                TrackedSourceRole::HostBinary,
                "https://example.invalid/host.dll",
                None,
                "zip-digest",
            )],
            "missing payload role",
        ),
    ];
    for (sources, label) in cases {
        let result = run_invalid(root.path(), &before, good_payload.clone(), &[], &sources);
        assert!(result.is_err(), "{label} must fail");
    }

    let mismatch = TrackedSource::new(
        TrackedSourceRole::AddonPayload,
        "https://example.invalid/luma.zip",
        Some("wrong-etag".to_owned()),
        "zip-digest",
    )
    .with_last_modified(Some("last-modified".to_owned()));
    assert!(run_invalid(root.path(), &before, good_payload, &[], &[mismatch]).is_err());

    let exact_payload = payload("Game.addon", &[("Game.addon", b"new")]);
    let invalid_identities = [
        TrackedSource::new(
            TrackedSourceRole::AddonPayload,
            "   ",
            Some("etag".to_owned()),
            "zip-digest",
        )
        .with_last_modified(Some("last-modified".to_owned())),
        TrackedSource::new(
            TrackedSourceRole::AddonPayload,
            "https://example.invalid/luma.zip",
            Some("etag".to_owned()),
            "wrong-digest",
        )
        .with_last_modified(Some("last-modified".to_owned())),
        TrackedSource::new(
            TrackedSourceRole::AddonPayload,
            "https://example.invalid/luma.zip",
            Some("etag".to_owned()),
            "zip-digest",
        )
        .with_last_modified(Some("wrong-last-modified".to_owned())),
    ];
    for identity in invalid_identities {
        assert!(
            run_invalid(
                root.path(),
                &before,
                exact_payload.clone(),
                &[],
                &[identity],
            )
            .is_err()
        );
    }
}

#[test]
fn missing_or_occupied_retained_and_removed_sidecars_fail_closed() {
    let retained = temp();
    std::fs::write(retained.path().join("Game.addon"), b"old").expect("live");
    let before = record(retained.path(), "Game.addon", &[], &["Game.addon"]);
    assert!(
        run_invalid(
            retained.path(),
            &before,
            payload("Game.addon", &[("Game.addon", b"new")]),
            &[],
            &[source(false)],
        )
        .is_err()
    );

    let removed_missing = temp();
    std::fs::write(removed_missing.path().join("Game.addon"), b"main").expect("main");
    let before = record(removed_missing.path(), "Game.addon", &["Old.hlsl"], &[]);
    assert!(
        run_invalid(
            removed_missing.path(),
            &before,
            payload("Game.addon", &[("Game.addon", b"main")]),
            &[],
            &[source(false)],
        )
        .is_err()
    );

    let removed_occupied = temp();
    std::fs::write(removed_occupied.path().join("Game.addon"), b"main").expect("main");
    std::fs::write(removed_occupied.path().join("Old.hlsl"), b"old").expect("old");
    std::fs::write(removed_occupied.path().join("Old.hlsl.bak"), b"orphan").expect("sidecar");
    let before = record(removed_occupied.path(), "Game.addon", &["Old.hlsl"], &[]);
    assert!(
        run_invalid(
            removed_occupied.path(),
            &before,
            payload("Game.addon", &[("Game.addon", b"main")]),
            &[],
            &[source(false)],
        )
        .is_err()
    );
}

#[test]
fn mtime_only_tracks_a_physical_main_change_and_dlss_is_typed() {
    let root = temp();
    let before = record(root.path(), "Game.addon", &[], &[]);
    std::fs::write(root.path().join("Game.addon"), b"old").expect("main");
    let mut full = payload(
        "Game.addon",
        &[("Game.addon", b"new"), ("nvngx_dlss.dll", b"dlss")],
    );
    full.last_modified = Some("exact-last-modified".to_owned());
    let source = TrackedSource::new(
        TrackedSourceRole::AddonPayload,
        "https://example.invalid/luma.zip",
        Some("etag".to_owned()),
        "zip-digest",
    )
    .with_last_modified(Some("exact-last-modified".to_owned()));
    let (projection, effects) = run(root.path(), &before, full, &[], &[source]).expect("full");
    let (_, _, dlss, mtime) = projection_parts(projection);
    assert!(
        matches!(dlss, LumaActiveUpdateDlssInput::Full { bundled_bytes: Some(bytes) } if bytes == b"dlss")
    );
    assert_eq!(
        mtime.expect("physical mtime").last_modified(),
        Some("exact-last-modified")
    );
    assert_eq!(effects.program().endpoints().len(), 1);
}

#[test]
fn classification_failure_after_an_earlier_valid_candidate_adds_no_effects() {
    let root = temp();
    let before = record(root.path(), "Game.addon", &[], &[]);
    std::fs::create_dir_all(root.path().join("Luma")).expect("payload parent");
    std::fs::write(root.path().join("Luma/Bad.hlsl.bak"), b"occupied").expect("sidecar");
    let mut accumulator = crate::addons::luma::peer::effects::LumaPeerEffectAccumulator::new(
        crate::addons::luma::peer::effects::LumaPeerOperationOrder::InstallOrUpdate,
    );
    let result = super::super::project_payload(
        &before,
        &authority(root.path()),
        super::super::super::model::LumaActiveUpdatePayloadInput::Full(payload(
            "Game.addon",
            &[("Game.addon", b"main"), ("Luma/Bad.hlsl", b"bad")],
        )),
        &[],
        &[source(false)],
        &mut accumulator,
    );
    assert!(result.is_err());
    assert!(accumulator.finalize().expect("effects").is_none());
}

#[test]
fn endpoint_order_and_payload_alignment_are_deterministic() {
    let first = temp();
    let second = temp();
    for root in [first.path(), second.path()] {
        std::fs::write(root.join("Game.addon"), b"old").expect("main");
        std::fs::create_dir_all(root.join("Luma")).expect("parent");
        std::fs::write(root.join("Luma/Z.hlsl"), b"foreign-z").expect("z");
        std::fs::write(root.join("Luma/A.hlsl"), b"foreign-a").expect("a");
    }
    // Recreate the first root's parent order-independent setup without relying
    // on any traversal order from the filesystem.
    let before_first = record(first.path(), "Game.addon", &[], &[]);
    let before_second = record(second.path(), "Game.addon", &[], &[]);
    let forward = [
        ("Luma/Z.hlsl", b"z-new".as_slice()),
        ("Game.addon", b"main-new".as_slice()),
        ("Luma/A.hlsl", b"a-new".as_slice()),
    ];
    let reverse = [forward[2], forward[1], forward[0]];
    let (_, first_effects) = run(
        first.path(),
        &before_first,
        payload("Game.addon", &forward),
        &[],
        &[source(false)],
    )
    .expect("first");
    let (_, second_effects) = run(
        second.path(),
        &before_second,
        payload("Game.addon", &reverse),
        &[],
        &[source(false)],
    )
    .expect("second");
    let relative = |root: &Path, endpoint_path: &PathRef| {
        let root_key = renderpilot_domain::normalized_path_key(path(root, "").as_str());
        renderpilot_domain::normalized_path_key(endpoint_path.as_str())
            .strip_prefix(&format!("{root_key}/"))
            .expect("root")
            .to_owned()
    };
    let first_names: Vec<_> = first_effects
        .program()
        .endpoints()
        .iter()
        .map(|endpoint| relative(first.path(), endpoint.path()))
        .collect();
    let second_names: Vec<_> = second_effects
        .program()
        .endpoints()
        .iter()
        .map(|endpoint| relative(second.path(), endpoint.path()))
        .collect();
    assert_eq!(first_names, second_names);
    assert_eq!(
        first_effects.payloads().len(),
        first_effects.program().endpoints().len()
    );
}
