use super::fixtures::{authority, path, prepared, record, run, source};
use crate::addons::luma::peer::{
    active_update::{dgvoodoo::project_dgvoodoo, model::LumaActiveUpdateDgVoodooInput},
    effects::{LumaPeerEffectAccumulator, LumaPeerOperationOrder},
};

#[test]
fn replace_source_must_be_exactly_one_non_advisory_wrapper() {
    let root = tempfile::tempdir().expect("root");
    let before = record(root.path(), &[], &[]);
    let prepared = prepared(&[]);
    let dependency = [path(root.path(), "dgVoodoo.conf")];

    for sources in [
        Vec::new(),
        vec![source(&prepared), source(&prepared)],
        vec![source(&prepared).with_advisory()],
        vec![source(&prepared).with_channel("dgvoodoo2@other")],
    ] {
        assert!(
            run(
                root.path(),
                &before,
                LumaActiveUpdateDgVoodooInput::Replace(Box::new(prepared.clone())),
                &dependency,
                &sources,
            )
            .is_err()
        );
    }
}

#[test]
fn remove_rejects_stale_wrapper_provenance() {
    let root = tempfile::tempdir().expect("root");
    let before = record(root.path(), &["D3D9.dll"], &[]);
    std::fs::write(root.path().join("D3D9.dll"), b"owned").expect("runtime");
    let prepared = prepared(&[("D3D9.dll", b"owned")]);
    assert!(
        run(
            root.path(),
            &before,
            LumaActiveUpdateDgVoodooInput::Remove,
            &[path(root.path(), "D3D9.dll")],
            &[source(&prepared)],
        )
        .is_err()
    );
}

#[test]
fn new_runtime_rejects_existing_live_or_sidecar() {
    for name in ["D3D9.dll", "D3D9.dll.bak"] {
        let root = tempfile::tempdir().expect("root");
        std::fs::write(root.path().join(name), b"foreign").expect("foreign endpoint");
        let before = record(root.path(), &[], &[]);
        let prepared = prepared(&[("D3D9.dll", b"owned")]);
        let dependencies = [
            path(root.path(), "D3D9.dll"),
            path(root.path(), "dgVoodoo.conf"),
        ];
        assert!(
            run(
                root.path(),
                &before,
                LumaActiveUpdateDgVoodooInput::Replace(Box::new(prepared.clone())),
                &dependencies,
                &[source(&prepared)],
            )
            .is_err()
        );
    }
}

#[test]
fn removal_requires_the_complete_owned_live_and_sidecar_state() {
    let root = tempfile::tempdir().expect("root");
    let before = record(root.path(), &["D3D9.dll"], &[]);
    assert!(
        run(
            root.path(),
            &before,
            LumaActiveUpdateDgVoodooInput::Remove,
            &[path(root.path(), "D3D9.dll")],
            &[],
        )
        .is_err()
    );

    let root = tempfile::tempdir().expect("root");
    let before = record(root.path(), &["D3D9.dll"], &[]);
    std::fs::write(root.path().join("D3D9.dll"), b"owned").expect("runtime");
    std::fs::write(root.path().join("D3D9.dll.bak"), b"foreign").expect("sidecar");
    assert!(
        run(
            root.path(),
            &before,
            LumaActiveUpdateDgVoodooInput::Remove,
            &[path(root.path(), "D3D9.dll")],
            &[],
        )
        .is_err()
    );

    let root = tempfile::tempdir().expect("root");
    let before = record(root.path(), &["D3D9.dll"], &["D3D9.dll"]);
    std::fs::write(root.path().join("D3D9.dll"), b"owned").expect("runtime");
    assert!(
        run(
            root.path(),
            &before,
            LumaActiveUpdateDgVoodooInput::Remove,
            &[path(root.path(), "D3D9.dll")],
            &[],
        )
        .is_err()
    );
}

#[test]
fn dependencies_are_unique_strict_game_children_and_non_overlapping() {
    let root = tempfile::tempdir().expect("root");
    let before = record(root.path(), &[], &[]);
    let prepared = prepared(&[]);
    let source = [source(&prepared)];
    let cases = [
        vec![
            path(root.path(), "dgVoodoo.conf"),
            path(root.path(), "DGVOODOO.CONF"),
        ],
        vec![
            path(root.path(), "nested"),
            path(root.path(), "nested/file.dll"),
        ],
        vec![path(root.path(), "../outside.dll")],
        vec![path(root.path(), "dgVoodoo.conf")],
    ];
    for dependencies in cases.iter().take(3) {
        assert!(
            run(
                root.path(),
                &before,
                LumaActiveUpdateDgVoodooInput::Replace(Box::new(prepared.clone())),
                dependencies,
                &source,
            )
            .is_err()
        );
    }
    assert!(
        run(
            root.path(),
            &before,
            LumaActiveUpdateDgVoodooInput::Replace(Box::new(prepared)),
            &cases[3],
            &source,
        )
        .is_ok()
    );
}

#[test]
fn backed_claim_without_created_claim_is_rejected_before_observation() {
    let root = tempfile::tempdir().expect("root");
    let before = record(root.path(), &[], &["D3D9.dll"]);
    assert!(
        run(
            root.path(),
            &before,
            LumaActiveUpdateDgVoodooInput::Remove,
            &[path(root.path(), "D3D9.dll")],
            &[],
        )
        .is_err()
    );
}

#[test]
fn classification_failure_does_not_leave_earlier_effects_in_accumulator() {
    let root = tempfile::tempdir().expect("root");
    let before = record(root.path(), &["D3D8.dll", "D3D9.dll"], &[]);
    std::fs::write(root.path().join("D3D8.dll"), b"d3d8").expect("first runtime");
    let prepared = prepared(&[("D3D8.dll", b"d3d8")]);
    let dependencies = [
        path(root.path(), "D3D8.dll"),
        path(root.path(), "D3D9.dll"),
        path(root.path(), "dgVoodoo.conf"),
    ];
    let mut accumulator = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);
    let result = project_dgvoodoo(
        &before,
        &authority(root.path()),
        LumaActiveUpdateDgVoodooInput::Replace(Box::new(prepared.clone())),
        &dependencies,
        &[source(&prepared)],
        &mut accumulator,
    );
    assert!(result.is_err());
    assert!(accumulator.finalize().expect("finalize").is_none());
}
