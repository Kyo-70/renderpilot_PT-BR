use std::path::Path;

use renderpilot_domain::{
    AddonKind, FileReceipt, GameId, GameProxyTopology, InstalledAddon, ManagedAddonFile,
    ManagedFileBaseline, PathRef, ProxyImplementation, ProxyLink, ProxyRootPrestate, Sha256Hash,
    managed_sidecar_path,
};

use crate::peer_mutation_executor::{EndpointExpectation, EndpointPostcondition};

use super::effects::{LumaPeerEffectAccumulator, LumaPeerEffects, LumaPeerOperationOrder};
use super::engine_uninstall::{EngineUninstallError, lower_engine_uninstall};
use super::generic::LumaGenericLoweringError;
use super::snapshot_input::LumaSnapshotInputError;

fn path_ref(path: &Path) -> PathRef {
    PathRef::new(path.to_string_lossy().into_owned()).expect("path")
}

fn digest(bytes: &[u8]) -> Sha256Hash {
    renderpilot_detection::sha256_bytes(bytes).expect("digest")
}

fn topology(root: &Path) -> GameProxyTopology {
    let root_slot = path_ref(&root.join("dxgi.dll"));
    let downstream = path_ref(&root.join("ReShade64.dll"));
    GameProxyTopology {
        id: "optiscaler:engine-uninstall-test".to_owned(),
        game_id: GameId::new("manual:engine-uninstall-test").expect("game"),
        root_slot: root_slot.clone(),
        outer: ProxyLink {
            implementation: ProxyImplementation::OptiScaler,
            path: root_slot.clone(),
            receipt: FileReceipt::owned("outer", digest(b"outer")).expect("receipt"),
        },
        downstream: Some(ProxyLink {
            implementation: ProxyImplementation::ReShade,
            path: downstream,
            receipt: FileReceipt::reused("reshade", digest(b"reshade")).expect("receipt"),
        }),
        downstream_origin: Some(root_slot),
        root_prestate: ProxyRootPrestate::Absent,
    }
}

fn record(root: &Path, first: &str) -> InstalledAddon {
    InstalledAddon::new(
        GameId::new("manual:engine-uninstall-test").expect("game"),
        AddonKind::Luma,
        path_ref(&root.join(first)),
    )
}

fn accumulator() -> LumaPeerEffectAccumulator {
    LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::Uninstall)
}

fn finalize(accumulator: LumaPeerEffectAccumulator) -> LumaPeerEffects {
    accumulator
        .finalize()
        .expect("finalize")
        .expect("engine effects")
}

#[test]
fn generic_created_only_ignores_orphan_sidecar() {
    let root = tempfile::tempdir().expect("root");
    let live_path = root.path().join("generic.dll");
    std::fs::write(&live_path, b"created").expect("live");
    std::fs::write(root.path().join("generic.dll.bak"), b"orphan").expect("orphan");
    let record = record(root.path(), "generic.dll");
    let mut effects = accumulator();

    lower_engine_uninstall(
        &record,
        &topology(root.path()),
        &path_ref(root.path()),
        None,
        &mut effects,
    )
    .expect("generic remove");
    let effects = finalize(effects);
    assert_eq!(effects.program().endpoints().len(), 1);
    assert!(matches!(
        effects.program().endpoints()[0].after(),
        EndpointPostcondition::Absent
    ));
    assert_eq!(effects.payloads(), &[None]);
}

#[test]
fn generic_backed_file_restores_exact_sidecar_bytes() {
    let root = tempfile::tempdir().expect("root");
    let live_path = root.path().join("generic.dll");
    std::fs::write(&live_path, b"created").expect("live");
    std::fs::write(root.path().join("generic.dll.bak"), b"baseline").expect("sidecar");
    let record = record(root.path(), "generic.dll").with_backed_up_file(path_ref(&live_path));
    let mut effects = accumulator();

    lower_engine_uninstall(
        &record,
        &topology(root.path()),
        &path_ref(root.path()),
        None,
        &mut effects,
    )
    .expect("generic restore");
    let effects = finalize(effects);
    let endpoints = effects.program().endpoints();
    assert_eq!(endpoints.len(), 2);
    assert_eq!(endpoints[0].path(), &path_ref(&live_path));
    assert!(matches!(
        endpoints[0].before(),
        EndpointExpectation::File(_)
    ));
    assert!(matches!(
        endpoints[0].after(),
        EndpointPostcondition::File(_)
    ));
    assert!(matches!(
        endpoints[1].after(),
        EndpointPostcondition::Absent
    ));
    assert_eq!(effects.payloads(), &[Some(b"baseline".to_vec()), None]);
}

#[test]
fn dependency_basename_maps_to_dgvoodoo_without_topology_basename_rejection() {
    let root = tempfile::tempdir().expect("root");
    let dependency = root.path().join("D3D11.dll");
    std::fs::write(&dependency, b"dependency").expect("dependency");
    let record = record(root.path(), "D3D11.dll");
    let mut effects = accumulator();

    lower_engine_uninstall(
        &record,
        &topology(root.path()),
        &path_ref(root.path()),
        None,
        &mut effects,
    )
    .expect("dgVoodoo remove");
    let effects = finalize(effects);
    let endpoint = &effects.program().endpoints()[0];
    assert_eq!(endpoint.path(), &path_ref(&dependency));
    assert!(matches!(endpoint.after(), EndpointPostcondition::Absent));
}

#[test]
fn deterministic_order_keeps_dgvoodoo_before_generic_and_payloads_aligned() {
    let root = tempfile::tempdir().expect("root");
    let dependency = root.path().join("D3D11.dll");
    let generic_created = root.path().join("z-generic.dll");
    let generic_backed = root.path().join("a-generic.dll");
    std::fs::write(&dependency, b"dependency").expect("dependency");
    std::fs::write(&generic_created, b"created").expect("generic created");
    std::fs::write(&generic_backed, b"active").expect("generic backed");
    std::fs::write(
        managed_sidecar_path(&path_ref(&generic_backed))
            .expect("sidecar")
            .as_str(),
        b"baseline",
    )
    .expect("sidecar");
    let record = record(root.path(), "z-generic.dll")
        .with_created_file(path_ref(&dependency))
        .with_created_file(path_ref(&generic_backed))
        .with_backed_up_file(path_ref(&generic_backed));
    let mut effects = accumulator();

    lower_engine_uninstall(
        &record,
        &topology(root.path()),
        &path_ref(root.path()),
        None,
        &mut effects,
    )
    .expect("deterministic engine release");
    let effects = finalize(effects);
    let endpoints = effects.program().endpoints();
    assert_eq!(endpoints.len(), 4);
    assert_eq!(endpoints[0].path(), &path_ref(&dependency));
    assert_eq!(endpoints[1].path(), &path_ref(&generic_backed));
    assert_eq!(
        endpoints[2].path(),
        &managed_sidecar_path(&path_ref(&generic_backed)).expect("sidecar")
    );
    assert_eq!(endpoints[3].path(), &path_ref(&generic_created));
    assert_eq!(
        effects.payloads(),
        &[None, Some(b"baseline".to_vec()), None, None]
    );
}

#[test]
fn structural_claim_errors_are_rejected_before_observation() {
    let root = tempfile::tempdir().expect("root");
    let duplicate_created_path = path_ref(&root.path().join("duplicate.dll"));
    let duplicate_created =
        record(root.path(), "duplicate.dll").with_created_file(duplicate_created_path);
    let mut effects = accumulator();
    let error = lower_engine_uninstall(
        &duplicate_created,
        &topology(root.path()),
        &path_ref(root.path()),
        None,
        &mut effects,
    )
    .expect_err("duplicate created path");
    assert!(matches!(error, EngineUninstallError::DuplicateCreated(_)));

    let duplicate_backed_path = path_ref(&root.path().join("backed.dll"));
    let duplicate_backed = record(root.path(), "backed.dll")
        .with_backed_up_file(duplicate_backed_path.clone())
        .with_backed_up_file(duplicate_backed_path);
    let mut effects = accumulator();
    let error = lower_engine_uninstall(
        &duplicate_backed,
        &topology(root.path()),
        &path_ref(root.path()),
        None,
        &mut effects,
    )
    .expect_err("duplicate backed path");
    assert!(matches!(error, EngineUninstallError::DuplicateBacked(_)));

    let backed_only = record(root.path(), "created.dll")
        .with_backed_up_file(path_ref(&root.path().join("backed-only.dll")));
    let mut effects = accumulator();
    let error = lower_engine_uninstall(
        &backed_only,
        &topology(root.path()),
        &path_ref(root.path()),
        None,
        &mut effects,
    )
    .expect_err("backed-only path");
    assert!(matches!(
        error,
        EngineUninstallError::BackedWithoutCreated(_)
    ));

    let engine_path = path_ref(&root.path().join("managed.dll"));
    let base = record(root.path(), "managed.dll");
    let managed =
        ManagedAddonFile::owned(engine_path, ManagedFileBaseline::Absent, digest(b"managed"));
    let mut value = serde_json::to_value(&base).expect("record json");
    value["managed_files"] = serde_json::json!([serde_json::to_value(managed).expect("managed")]);
    let malformed: InstalledAddon = serde_json::from_value(value).expect("malformed record");
    let mut effects = accumulator();
    let error = lower_engine_uninstall(
        &malformed,
        &topology(root.path()),
        &path_ref(root.path()),
        None,
        &mut effects,
    )
    .expect_err("engine managed overlap");
    assert!(matches!(
        error,
        EngineUninstallError::EngineManagedOverlap(_)
    ));
}

#[test]
fn topology_overlap_kind_and_noncanonical_claims_are_rejected() {
    let root = tempfile::tempdir().expect("root");
    let topology = topology(root.path());
    let root_record = record(root.path(), "dxgi.dll");
    let mut effects = accumulator();
    let error = lower_engine_uninstall(
        &root_record,
        &topology,
        &path_ref(root.path()),
        None,
        &mut effects,
    )
    .expect_err("topology root overlap");
    assert!(matches!(
        error,
        EngineUninstallError::TopologyOverlap { .. }
    ));

    let nested = root.path().join("ReShade64.dll").join("child.dll");
    let nested_record = record(root.path(), "ReShade64.dll").with_created_file(path_ref(&nested));
    let mut effects = accumulator();
    let error = lower_engine_uninstall(
        &nested_record,
        &topology,
        &path_ref(root.path()),
        None,
        &mut effects,
    )
    .expect_err("topology downstream overlap");
    assert!(matches!(
        error,
        EngineUninstallError::TopologyOverlap { .. }
    ));

    let noncanonical_path = root
        .path()
        .join("nested")
        .join("..")
        .join("noncanonical.dll");
    let noncanonical_record = record(root.path(), "nested\\..\\noncanonical.dll");
    let mut effects = accumulator();
    let error = lower_engine_uninstall(
        &noncanonical_record,
        &topology,
        &path_ref(root.path()),
        None,
        &mut effects,
    )
    .expect_err("noncanonical engine path");
    assert!(matches!(error, EngineUninstallError::NonCanonicalPath(_)));
    assert!(!noncanonical_path.exists());

    let wrong_kind = InstalledAddon::new(
        GameId::new("manual:engine-uninstall-test").expect("game"),
        AddonKind::RenoDx,
        path_ref(&root.path().join("wrong-kind.addon")),
    );
    let mut effects = accumulator();
    let error = lower_engine_uninstall(
        &wrong_kind,
        &topology,
        &path_ref(root.path()),
        None,
        &mut effects,
    )
    .expect_err("wrong record kind");
    assert!(matches!(error, EngineUninstallError::InvalidRecord(_)));
}

#[test]
fn physical_shape_and_root_failures_are_rejected() {
    let root = tempfile::tempdir().expect("root");
    let missing = record(root.path(), "missing.dll");
    let mut effects = accumulator();
    let error = lower_engine_uninstall(
        &missing,
        &topology(root.path()),
        &path_ref(root.path()),
        None,
        &mut effects,
    )
    .expect_err("missing live");
    assert!(matches!(
        error,
        EngineUninstallError::Generic(LumaGenericLoweringError::Snapshot(
            LumaSnapshotInputError::ExpectedFile(_)
        ))
    ));

    let backed_path = root.path().join("backed.dll");
    std::fs::write(&backed_path, b"active").expect("live");
    let backed = record(root.path(), "backed.dll").with_backed_up_file(path_ref(&backed_path));
    let mut effects = accumulator();
    let error = lower_engine_uninstall(
        &backed,
        &topology(root.path()),
        &path_ref(root.path()),
        None,
        &mut effects,
    )
    .expect_err("missing sidecar");
    assert!(matches!(
        error,
        EngineUninstallError::Generic(LumaGenericLoweringError::Snapshot(
            LumaSnapshotInputError::ExpectedFile(_)
        ))
    ));

    let outside = tempfile::tempdir().expect("outside");
    let outside_path = outside.path().join("outside.dll");
    std::fs::write(&outside_path, b"outside").expect("outside file");
    let outside_record = InstalledAddon::new(
        GameId::new("manual:engine-uninstall-test").expect("game"),
        AddonKind::Luma,
        path_ref(&outside_path),
    );
    let mut effects = accumulator();
    let error = lower_engine_uninstall(
        &outside_record,
        &topology(root.path()),
        &path_ref(root.path()),
        None,
        &mut effects,
    )
    .expect_err("outside endpoint");
    assert!(matches!(
        error,
        EngineUninstallError::PathOutsideAuthorizedRoots(_)
    ));
}

#[test]
fn split_payload_root_authorizes_generic_files_but_not_dgvoodoo() {
    let game = tempfile::tempdir().expect("game");
    let payload = tempfile::tempdir().expect("payload");
    let game_root = path_ref(game.path());
    let payload_root = path_ref(payload.path());

    let payload_backed_path = payload.path().join("luma.addon64");
    std::fs::write(&payload_backed_path, b"active").expect("payload live");
    std::fs::write(
        managed_sidecar_path(&path_ref(&payload_backed_path))
            .expect("payload sidecar")
            .as_str(),
        b"baseline",
    )
    .expect("payload sidecar");
    let payload_record = InstalledAddon::new(
        GameId::new("manual:engine-uninstall-test").expect("game id"),
        AddonKind::Luma,
        path_ref(&payload_backed_path),
    )
    .with_backed_up_file(path_ref(&payload_backed_path));
    let mut effects = accumulator();
    lower_engine_uninstall(
        &payload_record,
        &topology(game.path()),
        &game_root,
        Some(&payload_root),
        &mut effects,
    )
    .expect("payload restore");
    let effects = finalize(effects);
    assert_eq!(
        effects.program().endpoints()[0].path(),
        &path_ref(&payload_backed_path)
    );
    assert_eq!(effects.payloads()[0], Some(b"baseline".to_vec()));

    let payload_created_path = payload.path().join("created.addon64");
    std::fs::write(&payload_created_path, b"created").expect("payload created");
    let payload_created_record = InstalledAddon::new(
        GameId::new("manual:engine-uninstall-test").expect("game id"),
        AddonKind::Luma,
        path_ref(&payload_created_path),
    );
    let mut effects = accumulator();
    lower_engine_uninstall(
        &payload_created_record,
        &topology(game.path()),
        &game_root,
        Some(&payload_root),
        &mut effects,
    )
    .expect("payload remove");
    let effects = finalize(effects);
    assert_eq!(effects.program().endpoints().len(), 1);
    assert_eq!(
        effects.program().endpoints()[0].path(),
        &path_ref(&payload_created_path)
    );

    let payload_dependency_path = payload.path().join("D3D11.dll");
    std::fs::write(&payload_dependency_path, b"dependency").expect("payload dependency");
    let dependency_record = InstalledAddon::new(
        GameId::new("manual:engine-uninstall-test").expect("game id"),
        AddonKind::Luma,
        path_ref(&payload_dependency_path),
    );
    let mut effects = accumulator();
    let error = lower_engine_uninstall(
        &dependency_record,
        &topology(game.path()),
        &game_root,
        Some(&payload_root),
        &mut effects,
    )
    .expect_err("dgVoodoo payload escape");
    assert!(matches!(
        error,
        EngineUninstallError::DependencyOutsideGameRoot(_)
    ));

    let outside = tempfile::tempdir().expect("outside");
    let outside_path = outside.path().join("outside.addon64");
    std::fs::write(&outside_path, b"outside").expect("outside file");
    let outside_record = InstalledAddon::new(
        GameId::new("manual:engine-uninstall-test").expect("game id"),
        AddonKind::Luma,
        path_ref(&outside_path),
    );
    let mut effects = accumulator();
    let error = lower_engine_uninstall(
        &outside_record,
        &topology(game.path()),
        &game_root,
        Some(&payload_root),
        &mut effects,
    )
    .expect_err("outside both roots");
    assert!(matches!(
        error,
        EngineUninstallError::PathOutsideAuthorizedRoots(_)
    ));
}
