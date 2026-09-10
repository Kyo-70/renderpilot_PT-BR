use super::fixtures::{path, prepared, record, run, source};
use crate::addons::luma::peer::active_update::model::LumaActiveUpdateDgVoodooInput;

fn claims(
    projection: crate::addons::luma::peer::active_update::model::DgVoodooProjection,
) -> (usize, usize, usize, usize) {
    let (add_created, remove_created, add_backed, remove_backed) =
        projection.into_claims().into_parts();
    (
        add_created.len(),
        remove_created.len(),
        add_backed.len(),
        remove_backed.len(),
    )
}

#[test]
fn preserve_is_inert_even_with_irrelevant_dependency_data() {
    let root = tempfile::tempdir().expect("root");
    let before = record(root.path(), &[], &[]);
    let result = run(
        root.path(),
        &before,
        LumaActiveUpdateDgVoodooInput::Preserve,
        &[path(&root.path().join("outside"), "file.dll")],
        &[],
    )
    .expect("preserve");
    assert!(result.1.is_none());
    assert_eq!(claims(result.0), (0, 0, 0, 0));
}

#[test]
fn replace_creates_a_new_runtime_but_leaves_unowned_config_untouched() {
    let root = tempfile::tempdir().expect("root");
    let before = record(root.path(), &[], &[]);
    let prepared = prepared(&[("D3D9.dll", b"d3d9")]);
    let dependencies = [
        path(root.path(), "D3D9.dll"),
        path(root.path(), "dgVoodoo.conf"),
    ];
    let result = run(
        root.path(),
        &before,
        LumaActiveUpdateDgVoodooInput::Replace(Box::new(prepared.clone())),
        &dependencies,
        &[source(&prepared)],
    )
    .expect("replace");
    assert_eq!(claims(result.0), (1, 0, 0, 0));
    assert_eq!(
        result
            .1
            .expect("runtime effect")
            .program()
            .endpoints()
            .len(),
        1
    );
}

#[test]
fn replace_rejects_an_unowned_existing_runtime_without_effects() {
    let root = tempfile::tempdir().expect("root");
    std::fs::write(root.path().join("D3D9.dll"), b"foreign").expect("runtime");
    let before = record(root.path(), &[], &[]);
    let prepared = prepared(&[("D3D9.dll", b"d3d9")]);
    let dependencies = [
        path(root.path(), "D3D9.dll"),
        path(root.path(), "dgVoodoo.conf"),
    ];
    let result = run(
        root.path(),
        &before,
        LumaActiveUpdateDgVoodooInput::Replace(Box::new(prepared.clone())),
        &dependencies,
        &[source(&prepared)],
    );
    assert!(result.is_err());
}

#[test]
fn retained_runtime_equal_change_and_missing_are_owned_only() {
    for state in ["equal", "change", "missing"] {
        let root = tempfile::tempdir().expect("root");
        let before = record(root.path(), &["D3D9.dll"], &[]);
        if state != "missing" {
            let bytes: &[u8] = if state == "equal" { b"d3d9" } else { b"old" };
            std::fs::write(root.path().join("D3D9.dll"), bytes).expect("runtime");
        }
        let prepared = prepared(&[("D3D9.dll", b"d3d9")]);
        let dependencies = [
            path(root.path(), "D3D9.dll"),
            path(root.path(), "dgVoodoo.conf"),
        ];
        let result = run(
            root.path(),
            &before,
            LumaActiveUpdateDgVoodooInput::Replace(Box::new(prepared.clone())),
            &dependencies,
            &[source(&prepared)],
        )
        .expect("retained runtime");
        let has_effect = result.1.is_some();
        assert_eq!(has_effect, state != "equal");
        assert_eq!(claims(result.0), (0, 0, 0, 0));
    }
}

#[test]
fn retained_backed_runtime_requires_and_preserves_sidecar() {
    let root = tempfile::tempdir().expect("root");
    let before = record(root.path(), &["D3D9.dll"], &["D3D9.dll"]);
    std::fs::write(root.path().join("D3D9.dll"), b"old").expect("runtime");
    std::fs::write(root.path().join("D3D9.dll.bak"), b"baseline").expect("sidecar");
    let prepared = prepared(&[("D3D9.dll", b"d3d9")]);
    let dependencies = [
        path(root.path(), "D3D9.dll"),
        path(root.path(), "dgVoodoo.conf"),
    ];
    let result = run(
        root.path(),
        &before,
        LumaActiveUpdateDgVoodooInput::Replace(Box::new(prepared.clone())),
        &dependencies,
        &[source(&prepared)],
    )
    .expect("backed runtime");
    assert!(result.1.is_some());
    assert_eq!(claims(result.0), (0, 0, 0, 0));
}

#[test]
fn owned_config_merges_equal_change_and_repairs_from_baseline() {
    let dependencies_name = "dgVoodoo.conf";
    let dependencies_suffix = |root: &std::path::Path| [path(root, dependencies_name)];
    let prepared = prepared(&[]);

    let root = tempfile::tempdir().expect("root");
    let before = record(root.path(), &[dependencies_name], &[]);
    std::fs::write(
        root.path().join(dependencies_name),
        "[General]\r\nOutputAPI = d3d11\r\n",
    )
    .expect("config");
    let result = run(
        root.path(),
        &before,
        LumaActiveUpdateDgVoodooInput::Replace(Box::new(prepared.clone())),
        &dependencies_suffix(root.path()),
        &[source(&prepared)],
    )
    .expect("equal config");
    assert!(result.1.is_none());

    std::fs::write(
        root.path().join(dependencies_name),
        "[General]\r\nOutputAPI = d3d9\r\n",
    )
    .expect("changed config");
    let result = run(
        root.path(),
        &before,
        LumaActiveUpdateDgVoodooInput::Replace(Box::new(prepared.clone())),
        &dependencies_suffix(root.path()),
        &[source(&prepared)],
    )
    .expect("changed config");
    assert!(result.1.is_some());

    std::fs::remove_file(root.path().join(dependencies_name)).expect("remove config");
    let result = run(
        root.path(),
        &before,
        LumaActiveUpdateDgVoodooInput::Replace(Box::new(prepared.clone())),
        &dependencies_suffix(root.path()),
        &[source(&prepared)],
    )
    .expect("repair config");
    assert!(result.1.is_some());
    assert_eq!(claims(result.0), (0, 0, 0, 0));
}

#[test]
fn backed_config_repairs_from_exact_sidecar_baseline() {
    let root = tempfile::tempdir().expect("root");
    let before = record(root.path(), &["dgVoodoo.conf"], &["dgVoodoo.conf"]);
    std::fs::write(
        root.path().join("dgVoodoo.conf.bak"),
        "[General]\r\nOutputAPI = d3d9\r\n",
    )
    .expect("sidecar");
    let prepared = prepared(&[]);
    let result = run(
        root.path(),
        &before,
        LumaActiveUpdateDgVoodooInput::Replace(Box::new(prepared.clone())),
        &[path(root.path(), "dgVoodoo.conf")],
        &[source(&prepared)],
    )
    .expect("backed repair");
    assert!(result.1.is_some());
}

#[test]
fn unowned_config_and_invalid_utf8_are_handled_conservatively() {
    let root = tempfile::tempdir().expect("root");
    std::fs::write(root.path().join("dgVoodoo.conf"), b"foreign").expect("config");
    let before = record(root.path(), &[], &[]);
    let prepared = prepared(&[]);
    let dependency = [path(root.path(), "dgVoodoo.conf")];
    let result = run(
        root.path(),
        &before,
        LumaActiveUpdateDgVoodooInput::Replace(Box::new(prepared.clone())),
        &dependency,
        &[source(&prepared)],
    )
    .expect("unowned config");
    assert!(result.1.is_none());

    let before = record(root.path(), &["dgVoodoo.conf"], &[]);
    std::fs::write(root.path().join("dgVoodoo.conf"), [0xff, 0xfe]).expect("invalid utf8");
    let result = run(
        root.path(),
        &before,
        LumaActiveUpdateDgVoodooInput::Replace(Box::new(prepared.clone())),
        &dependency,
        &[source(&prepared)],
    );
    assert!(result.is_err());
}

#[test]
fn remove_releases_created_only_and_backed_claims() {
    let root = tempfile::tempdir().expect("root");
    let before = record(root.path(), &["D3D9.dll"], &[]);
    std::fs::write(root.path().join("D3D9.dll"), b"owned").expect("runtime");
    let result = run(
        root.path(),
        &before,
        LumaActiveUpdateDgVoodooInput::Remove,
        &[path(root.path(), "D3D9.dll")],
        &[],
    )
    .expect("remove created");
    assert_eq!(claims(result.0), (0, 1, 0, 0));
    assert!(result.1.is_some());

    let root = tempfile::tempdir().expect("root");
    let before = record(root.path(), &["D3D9.dll"], &["D3D9.dll"]);
    std::fs::write(root.path().join("D3D9.dll"), b"owned").expect("runtime");
    std::fs::write(root.path().join("D3D9.dll.bak"), b"baseline").expect("sidecar");
    let result = run(
        root.path(),
        &before,
        LumaActiveUpdateDgVoodooInput::Remove,
        &[path(root.path(), "D3D9.dll")],
        &[],
    )
    .expect("remove backed");
    assert_eq!(claims(result.0), (0, 1, 0, 1));
}

#[test]
fn replace_rename_releases_old_and_creates_new_runtime() {
    let root = tempfile::tempdir().expect("root");
    let before = record(root.path(), &["D3D9.dll"], &[]);
    std::fs::write(root.path().join("D3D9.dll"), b"old").expect("old runtime");
    let prepared = prepared(&[("D3D8.dll", b"new")]);
    let dependencies = [
        path(root.path(), "D3D8.dll"),
        path(root.path(), "D3D9.dll"),
        path(root.path(), "dgVoodoo.conf"),
    ];
    let result = run(
        root.path(),
        &before,
        LumaActiveUpdateDgVoodooInput::Replace(Box::new(prepared.clone())),
        &dependencies,
        &[source(&prepared)],
    )
    .expect("rename");
    assert_eq!(claims(result.0), (1, 1, 0, 0));
    assert_eq!(result.1.expect("effects").program().endpoints().len(), 2);
}

#[test]
fn runtime_endpoints_and_payloads_follow_normalized_live_order() {
    let root = tempfile::tempdir().expect("root");
    let before = record(root.path(), &[], &[]);
    let prepared = prepared(&[("D3D9.dll", b"nine"), ("D3D8.dll", b"eight")]);
    let dependencies = [
        path(root.path(), "D3D9.dll"),
        path(root.path(), "D3D8.dll"),
        path(root.path(), "dgVoodoo.conf"),
    ];
    let result = run(
        root.path(),
        &before,
        LumaActiveUpdateDgVoodooInput::Replace(Box::new(prepared.clone())),
        &dependencies,
        &[source(&prepared)],
    )
    .expect("ordered runtime");
    let effects = result.1.expect("effects");
    let endpoints = effects.program().endpoints();
    assert!(endpoints[0].path().as_str().ends_with("D3D8.dll"));
    assert!(endpoints[1].path().as_str().ends_with("D3D9.dll"));
    assert_eq!(effects.payloads()[0], Some(b"eight".to_vec()));
    assert_eq!(effects.payloads()[1], Some(b"nine".to_vec()));
}
