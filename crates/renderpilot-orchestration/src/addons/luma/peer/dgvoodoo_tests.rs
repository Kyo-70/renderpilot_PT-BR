use super::dgvoodoo::{DgVoodooDecision, DgVoodooLoweringError, lower_dgvoodoo_decision};
use super::effects::{
    LumaPeerEffectAccumulator, LumaPeerEffectGroup, LumaPeerEffects, LumaPeerOperationOrder,
};
use super::snapshot_input::LumaSnapshotInputError;
use crate::peer_mutation_executor::{
    EndpointExpectation, EndpointPostcondition, PeerPathSnapshot, observe_peer_path_snapshot,
};
use renderpilot_domain::{PathRef, PeerEndpointRole, managed_sidecar_path, normalized_path_key};

fn path_ref(path: &std::path::Path) -> PathRef {
    PathRef::new(path.to_string_lossy().into_owned()).expect("path")
}

fn snapshot(root: &std::path::Path, name: &str, bytes: &[u8]) -> PeerPathSnapshot {
    let file = root.join(name);
    std::fs::write(&file, bytes).expect("file");
    observe_peer_path_snapshot(&path_ref(&file), &path_ref(root)).expect("snapshot")
}

fn absent_snapshot(root: &std::path::Path, name: &str) -> PeerPathSnapshot {
    observe_peer_path_snapshot(&path_ref(&root.join(name)), &path_ref(root)).expect("absence")
}

fn finalize(accumulator: LumaPeerEffectAccumulator) -> LumaPeerEffects {
    accumulator
        .finalize()
        .expect("finalize")
        .expect("dgvoodoo effects")
}

#[test]
fn unchanged_or_reused_preserves_existing_effect() {
    let mut accumulator = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);
    accumulator
        .create(
            LumaPeerEffectGroup::Generic,
            PathRef::parse_exact("C:/renderpilot-tests/luma-dgvoodoo/game/peer.dll").expect("path"),
            b"peer".to_vec(),
        )
        .expect("existing effect");
    lower_dgvoodoo_decision(DgVoodooDecision::UnchangedOrReused, &mut accumulator)
        .expect("unchanged");
    assert_eq!(finalize(accumulator).program().endpoints().len(), 1);
}

#[test]
fn create_managed_accepts_absent_runtime_and_config_paths() {
    let root = tempfile::tempdir().expect("root");
    let runtime_path = path_ref(&root.path().join("d3d9.dll"));
    let config_path = path_ref(&root.path().join("dgvoodoo.conf"));
    let runtime_absent = absent_snapshot(root.path(), "d3d9.dll");
    let config_absent = absent_snapshot(root.path(), "dgvoodoo.conf");
    let mut accumulator = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);
    lower_dgvoodoo_decision(
        DgVoodooDecision::CreateManaged {
            live_path: &runtime_path,
            live_snapshot: &runtime_absent,
            prepared_bytes: b"runtime".to_vec(),
        },
        &mut accumulator,
    )
    .expect("runtime create");
    lower_dgvoodoo_decision(
        DgVoodooDecision::CreateManaged {
            live_path: &config_path,
            live_snapshot: &config_absent,
            prepared_bytes: b"config".to_vec(),
        },
        &mut accumulator,
    )
    .expect("config create");
    let effects = finalize(accumulator);
    assert_eq!(effects.program().endpoints().len(), 2);
    assert!(
        effects
            .program()
            .endpoints()
            .iter()
            .all(|endpoint| endpoint.role() == PeerEndpointRole::Disjoint)
    );
    assert_eq!(
        effects.payloads(),
        &[Some(b"runtime".to_vec()), Some(b"config".to_vec())]
    );

    let present = snapshot(root.path(), "present.dll", b"existing");
    let present_path = path_ref(&root.path().join("present.dll"));
    let mut rejected = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);
    let error = lower_dgvoodoo_decision(
        DgVoodooDecision::CreateManaged {
            live_path: &present_path,
            live_snapshot: &present,
            prepared_bytes: b"new".to_vec(),
        },
        &mut rejected,
    )
    .expect_err("present create must reject");
    assert!(matches!(
        error,
        DgVoodooLoweringError::Snapshot(LumaSnapshotInputError::ExpectedAbsent(_))
    ));
    assert!(rejected.finalize().expect("rejected").is_none());
}

#[test]
fn acquire_managed_is_an_exact_disjoint_sidecar_then_live_pair() {
    let root = tempfile::tempdir().expect("root");
    let live_path = path_ref(&root.path().join("d3d9.dll"));
    let sidecar_path = managed_sidecar_path(&live_path).expect("sidecar");
    let live = snapshot(root.path(), "d3d9.dll", b"foreign");
    let sidecar = absent_snapshot(root.path(), "d3d9.dll.bak");
    let mut accumulator = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);
    lower_dgvoodoo_decision(
        DgVoodooDecision::AcquireManaged {
            live_path: &live_path,
            sidecar_path: &sidecar_path,
            live_snapshot: &live,
            sidecar_snapshot: &sidecar,
            prepared_bytes: b"managed".to_vec(),
        },
        &mut accumulator,
    )
    .expect("acquire");
    let effects = finalize(accumulator);
    let endpoints = effects.program().endpoints();
    assert_eq!(endpoints.len(), 2);
    assert_eq!(
        normalized_path_key(endpoints[0].path().as_str()),
        normalized_path_key(sidecar_path.as_str())
    );
    assert_eq!(endpoints[1].path(), &live_path);
    assert!(
        endpoints
            .iter()
            .all(|endpoint| endpoint.role() == PeerEndpointRole::Disjoint)
    );
    assert!(matches!(endpoints[0].before(), EndpointExpectation::Absent));
    assert!(matches!(
        endpoints[1].before(),
        EndpointExpectation::File(file) if Some(file) == live.file()
    ));
    assert_eq!(
        effects.payloads(),
        &[Some(b"foreign".to_vec()), Some(b"managed".to_vec())]
    );
}

#[test]
fn acquire_managed_rejects_bad_state_or_sidecar_without_effects() {
    let root = tempfile::tempdir().expect("root");
    let live_path = path_ref(&root.path().join("d3d9.dll"));
    let sidecar_path = managed_sidecar_path(&live_path).expect("sidecar");
    let live = snapshot(root.path(), "d3d9.dll", b"foreign");
    let absent_sidecar = absent_snapshot(root.path(), "d3d9.dll.bak");

    let wrong_path = path_ref(&root.path().join("other.dll.bak"));
    let mut wrong = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);
    let error = lower_dgvoodoo_decision(
        DgVoodooDecision::AcquireManaged {
            live_path: &live_path,
            sidecar_path: &wrong_path,
            live_snapshot: &live,
            sidecar_snapshot: &absent_sidecar,
            prepared_bytes: b"managed".to_vec(),
        },
        &mut wrong,
    )
    .expect_err("wrong path must reject");
    assert!(matches!(
        error,
        DgVoodooLoweringError::Snapshot(LumaSnapshotInputError::SidecarPathMismatch { .. })
    ));
    assert!(wrong.finalize().expect("wrong").is_none());

    let present_sidecar = snapshot(root.path(), "d3d9.dll.bak", b"existing");
    let mut occupied = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);
    let error = lower_dgvoodoo_decision(
        DgVoodooDecision::AcquireManaged {
            live_path: &live_path,
            sidecar_path: &sidecar_path,
            live_snapshot: &live,
            sidecar_snapshot: &present_sidecar,
            prepared_bytes: b"managed".to_vec(),
        },
        &mut occupied,
    )
    .expect_err("present sidecar must reject");
    assert!(matches!(
        error,
        DgVoodooLoweringError::Snapshot(LumaSnapshotInputError::ExpectedAbsent(_))
    ));
    assert!(occupied.finalize().expect("occupied").is_none());

    let missing_path = path_ref(&root.path().join("missing.dll"));
    let missing_sidecar = managed_sidecar_path(&missing_path).expect("sidecar");
    let absent_live = absent_snapshot(root.path(), "missing.dll");
    let mut absent = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);
    let error = lower_dgvoodoo_decision(
        DgVoodooDecision::AcquireManaged {
            live_path: &missing_path,
            sidecar_path: &missing_sidecar,
            live_snapshot: &absent_live,
            sidecar_snapshot: &absent_sidecar,
            prepared_bytes: b"managed".to_vec(),
        },
        &mut absent,
    )
    .expect_err("absent live must reject");
    assert!(matches!(
        error,
        DgVoodooLoweringError::Snapshot(LumaSnapshotInputError::ExpectedFile(_))
    ));
    assert!(absent.finalize().expect("absent").is_none());
}

#[test]
fn replace_owned_accepts_runtime_file_and_rejects_absent() {
    let root = tempfile::tempdir().expect("root");
    let live_path = path_ref(&root.path().join("dgvoodoo.conf"));
    let live = snapshot(root.path(), "dgvoodoo.conf", b"old config");
    let mut accumulator = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);
    lower_dgvoodoo_decision(
        DgVoodooDecision::ReplaceOwned {
            live_path: &live_path,
            live_snapshot: &live,
            prepared_bytes: b"merged config".to_vec(),
        },
        &mut accumulator,
    )
    .expect("replace");
    let effects = finalize(accumulator);
    assert_eq!(
        effects.program().endpoints()[0].role(),
        PeerEndpointRole::Disjoint
    );
    assert_eq!(effects.payloads(), &[Some(b"merged config".to_vec())]);

    let absent = absent_snapshot(root.path(), "absent.dll");
    let absent_path = path_ref(&root.path().join("absent.dll"));
    let mut rejected = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);
    let error = lower_dgvoodoo_decision(
        DgVoodooDecision::ReplaceOwned {
            live_path: &absent_path,
            live_snapshot: &absent,
            prepared_bytes: b"new".to_vec(),
        },
        &mut rejected,
    )
    .expect_err("absent replace must reject");
    assert!(matches!(
        error,
        DgVoodooLoweringError::Snapshot(LumaSnapshotInputError::ExpectedFile(_))
    ));
}

#[test]
fn release_owned_absent_removes_only_the_present_runtime_file() {
    let root = tempfile::tempdir().expect("root");
    let live_path = path_ref(&root.path().join("d3d9.dll"));
    let live = snapshot(root.path(), "d3d9.dll", b"managed");
    let mut accumulator = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::Uninstall);
    lower_dgvoodoo_decision(
        DgVoodooDecision::ReleaseOwnedAbsent {
            live_path: &live_path,
            live_snapshot: &live,
        },
        &mut accumulator,
    )
    .expect("remove");
    let effects = finalize(accumulator);
    let endpoint = &effects.program().endpoints()[0];
    assert_eq!(endpoint.role(), PeerEndpointRole::Disjoint);
    assert!(matches!(endpoint.after(), EndpointPostcondition::Absent));
    assert_eq!(effects.payloads(), &[None]);
}

#[test]
fn release_owned_present_restores_config_baseline_then_removes_sidecar() {
    let root = tempfile::tempdir().expect("root");
    let live_path = path_ref(&root.path().join("dgvoodoo.conf"));
    let sidecar_path = managed_sidecar_path(&live_path).expect("sidecar");
    let live = snapshot(root.path(), "dgvoodoo.conf", b"managed config");
    let sidecar = snapshot(root.path(), "dgvoodoo.conf.bak", b"original config");
    let mut accumulator = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::Uninstall);
    lower_dgvoodoo_decision(
        DgVoodooDecision::ReleaseOwnedPresent {
            live_path: &live_path,
            sidecar_path: &sidecar_path,
            live_snapshot: &live,
            sidecar_snapshot: &sidecar,
        },
        &mut accumulator,
    )
    .expect("release");
    let effects = finalize(accumulator);
    let endpoints = effects.program().endpoints();
    assert_eq!(endpoints.len(), 2);
    assert_eq!(endpoints[0].path(), &live_path);
    assert_eq!(endpoints[1].path(), &sidecar_path);
    assert!(
        endpoints
            .iter()
            .all(|endpoint| endpoint.role() == PeerEndpointRole::Disjoint)
    );
    assert!(matches!(
        endpoints[0].after(),
        EndpointPostcondition::File(_)
    ));
    assert!(matches!(
        endpoints[1].after(),
        EndpointPostcondition::Absent
    ));
    assert_eq!(
        effects.payloads(),
        &[Some(b"original config".to_vec()), None]
    );
}

#[test]
fn release_owned_present_rejects_absence_or_wrong_sidecar_without_effects() {
    let root = tempfile::tempdir().expect("root");
    let live_path = path_ref(&root.path().join("d3d9.dll"));
    let sidecar_path = managed_sidecar_path(&live_path).expect("sidecar");
    let live = snapshot(root.path(), "d3d9.dll", b"managed");
    let sidecar = snapshot(root.path(), "d3d9.dll.bak", b"original");

    let wrong_path = path_ref(&root.path().join("other.dll.bak"));
    let mut wrong = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::Uninstall);
    let error = lower_dgvoodoo_decision(
        DgVoodooDecision::ReleaseOwnedPresent {
            live_path: &live_path,
            sidecar_path: &wrong_path,
            live_snapshot: &live,
            sidecar_snapshot: &sidecar,
        },
        &mut wrong,
    )
    .expect_err("wrong path must reject");
    assert!(matches!(
        error,
        DgVoodooLoweringError::Snapshot(LumaSnapshotInputError::SidecarPathMismatch { .. })
    ));
    assert!(wrong.finalize().expect("wrong").is_none());

    let absent = absent_snapshot(root.path(), "missing.dll");
    let missing_path = path_ref(&root.path().join("missing.dll"));
    let missing_sidecar = managed_sidecar_path(&missing_path).expect("sidecar");
    let mut missing_live = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::Uninstall);
    let error = lower_dgvoodoo_decision(
        DgVoodooDecision::ReleaseOwnedPresent {
            live_path: &missing_path,
            sidecar_path: &missing_sidecar,
            live_snapshot: &absent,
            sidecar_snapshot: &sidecar,
        },
        &mut missing_live,
    )
    .expect_err("absent live must reject");
    assert!(matches!(
        error,
        DgVoodooLoweringError::Snapshot(LumaSnapshotInputError::ExpectedFile(_))
    ));
    assert!(missing_live.finalize().expect("missing live").is_none());

    let absent_sidecar = absent_snapshot(root.path(), "missing.dll.bak");
    let mut missing_sidecar = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::Uninstall);
    let error = lower_dgvoodoo_decision(
        DgVoodooDecision::ReleaseOwnedPresent {
            live_path: &live_path,
            sidecar_path: &sidecar_path,
            live_snapshot: &live,
            sidecar_snapshot: &absent_sidecar,
        },
        &mut missing_sidecar,
    )
    .expect_err("absent sidecar must reject");
    assert!(matches!(
        error,
        DgVoodooLoweringError::Snapshot(LumaSnapshotInputError::ExpectedFile(_))
    ));
}
