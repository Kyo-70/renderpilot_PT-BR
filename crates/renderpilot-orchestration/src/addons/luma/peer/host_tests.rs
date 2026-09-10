use super::effects::{LumaPeerEffectAccumulator, LumaPeerEffects, LumaPeerOperationOrder};
use super::host::{LumaHostDecision, LumaHostLoweringError, lower_host_decision};
use super::snapshot_input::LumaSnapshotInputError;
use crate::peer_mutation_executor::{
    EndpointExpectation, EndpointPostcondition, PeerPathSnapshot, observe_peer_path_snapshot,
};
use renderpilot_domain::{PathRef, PeerEndpointRole, managed_sidecar_path, normalized_path_key};

fn snapshot(root: &std::path::Path, name: &str, bytes: &[u8]) -> PeerPathSnapshot {
    let file = root.join(name);
    std::fs::write(&file, bytes).expect("file");
    observe_peer_path_snapshot(&path_ref(&file), &path_ref(root)).expect("snapshot")
}

fn absent_snapshot(root: &std::path::Path, name: &str) -> PeerPathSnapshot {
    observe_peer_path_snapshot(&path_ref(&root.join(name)), &path_ref(root)).expect("absence")
}

fn path_ref(path: &std::path::Path) -> PathRef {
    PathRef::new(path.to_string_lossy().into_owned()).expect("path")
}

fn lower(
    decision: LumaHostDecision<'_>,
    accumulator: &mut LumaPeerEffectAccumulator,
) -> Result<(), LumaHostLoweringError> {
    lower_host_decision(decision, accumulator)
}

fn finalize(accumulator: LumaPeerEffectAccumulator) -> LumaPeerEffects {
    accumulator
        .finalize()
        .expect("finalize")
        .expect("host effects")
}

#[test]
fn owned_create_requires_absent_and_emits_one_host_create() {
    let root = tempfile::tempdir().expect("root");
    let live_path = path_ref(&root.path().join("host.dll"));
    let absent = absent_snapshot(root.path(), "host.dll");
    let mut accumulator = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);
    lower(
        LumaHostDecision::Create {
            live_path: &live_path,
            live_snapshot: &absent,
            prepared_bytes: b"host".to_vec(),
        },
        &mut accumulator,
    )
    .expect("host create");

    let effects = finalize(accumulator);
    let endpoint = &effects.program().endpoints()[0];
    assert_eq!(endpoint.path(), &live_path);
    assert_eq!(endpoint.role(), PeerEndpointRole::TopologyDownstream);
    assert!(matches!(endpoint.before(), EndpointExpectation::Absent));
    assert_eq!(effects.payloads(), &[Some(b"host".to_vec())]);

    let present = snapshot(root.path(), "present.dll", b"foreign");
    let present_path = path_ref(&root.path().join("present.dll"));
    let mut rejected = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);
    let error = lower(
        LumaHostDecision::Create {
            live_path: &present_path,
            live_snapshot: &present,
            prepared_bytes: b"host".to_vec(),
        },
        &mut rejected,
    )
    .expect_err("present host must reject create");
    assert!(matches!(
        error,
        LumaHostLoweringError::Snapshot(LumaSnapshotInputError::ExpectedAbsent(_))
    ));
    assert!(rejected.finalize().expect("rejected accumulator").is_none());
}

#[test]
fn owned_replace_requires_file_and_preserves_exact_before_image() {
    let root = tempfile::tempdir().expect("root");
    let live_path = path_ref(&root.path().join("host.dll"));
    let present = snapshot(root.path(), "host.dll", b"old");
    let mut accumulator = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);
    lower(
        LumaHostDecision::Replace {
            live_path: &live_path,
            live_snapshot: &present,
            prepared_bytes: b"new".to_vec(),
        },
        &mut accumulator,
    )
    .expect("host replace");
    let effects = finalize(accumulator);
    let endpoint = &effects.program().endpoints()[0];
    assert!(matches!(
        endpoint.before(),
        EndpointExpectation::File(file) if Some(file) == present.file()
    ));
    assert!(matches!(endpoint.after(), EndpointPostcondition::File(_)));

    let absent = absent_snapshot(root.path(), "absent.dll");
    let absent_path = path_ref(&root.path().join("absent.dll"));
    let mut rejected = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);
    let error = lower(
        LumaHostDecision::Replace {
            live_path: &absent_path,
            live_snapshot: &absent,
            prepared_bytes: b"new".to_vec(),
        },
        &mut rejected,
    )
    .expect_err("absent host must reject replace");
    assert!(matches!(
        error,
        LumaHostLoweringError::Snapshot(LumaSnapshotInputError::ExpectedFile(_))
    ));
}

#[test]
fn owned_release_absent_requires_file_and_emits_one_host_remove() {
    let root = tempfile::tempdir().expect("root");
    let live_path = path_ref(&root.path().join("host.dll"));
    let present = snapshot(root.path(), "host.dll", b"managed");
    let mut accumulator = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::Uninstall);
    lower(
        LumaHostDecision::ReleaseAbsent {
            live_path: &live_path,
            live_snapshot: &present,
        },
        &mut accumulator,
    )
    .expect("host remove");
    let effects = finalize(accumulator);
    let endpoint = &effects.program().endpoints()[0];
    assert_eq!(endpoint.role(), PeerEndpointRole::TopologyDownstream);
    assert!(matches!(endpoint.after(), EndpointPostcondition::Absent));
    assert_eq!(effects.payloads(), &[None]);

    let absent = absent_snapshot(root.path(), "absent.dll");
    let absent_path = path_ref(&root.path().join("absent.dll"));
    let mut rejected = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::Uninstall);
    let error = lower(
        LumaHostDecision::ReleaseAbsent {
            live_path: &absent_path,
            live_snapshot: &absent,
        },
        &mut rejected,
    )
    .expect_err("absent host must reject release");
    assert!(matches!(
        error,
        LumaHostLoweringError::Snapshot(LumaSnapshotInputError::ExpectedFile(_))
    ));
}

#[test]
fn owned_release_present_uses_derived_sidecar_and_exact_baseline_bytes() {
    let root = tempfile::tempdir().expect("root");
    let live_path = path_ref(&root.path().join("host.dll"));
    let sidecar_path = managed_sidecar_path(&live_path).expect("sidecar");
    let live = snapshot(root.path(), "host.dll", b"managed");
    let sidecar = snapshot(root.path(), "host.dll.bak", b"original");
    let mut accumulator = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::Uninstall);
    lower(
        LumaHostDecision::ReleasePresent {
            live_path: &live_path,
            sidecar_path: &sidecar_path,
            live_snapshot: &live,
            sidecar_snapshot: &sidecar,
        },
        &mut accumulator,
    )
    .expect("host release");

    let effects = finalize(accumulator);
    let endpoints = effects.program().endpoints();
    assert_eq!(endpoints.len(), 2);
    assert_eq!(endpoints[0].path(), &live_path);
    assert_eq!(
        normalized_path_key(endpoints[1].path().as_str()),
        normalized_path_key(sidecar_path.as_str())
    );
    assert_eq!(endpoints[0].role(), PeerEndpointRole::TopologyDownstream);
    assert_eq!(endpoints[1].role(), PeerEndpointRole::Disjoint);
    assert!(matches!(
        endpoints[0].after(),
        EndpointPostcondition::File(_)
    ));
    assert!(matches!(
        endpoints[1].after(),
        EndpointPostcondition::Absent
    ));
    assert_eq!(effects.payloads(), &[Some(b"original".to_vec()), None]);
}

#[test]
fn owned_release_present_rejects_absence_and_wrong_sidecar_association() {
    let root = tempfile::tempdir().expect("root");
    let live_path = path_ref(&root.path().join("host.dll"));
    let sidecar_path = managed_sidecar_path(&live_path).expect("sidecar");
    let live = snapshot(root.path(), "host.dll", b"managed");
    let sidecar = snapshot(root.path(), "host.dll.bak", b"original");

    let wrong_sidecar = path_ref(&root.path().join("other.dll.bak"));
    let mut wrong_path_accumulator =
        LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::Uninstall);
    let error = lower(
        LumaHostDecision::ReleasePresent {
            live_path: &live_path,
            sidecar_path: &wrong_sidecar,
            live_snapshot: &live,
            sidecar_snapshot: &sidecar,
        },
        &mut wrong_path_accumulator,
    )
    .expect_err("wrong sidecar path must reject");
    assert!(matches!(
        error,
        LumaHostLoweringError::Snapshot(LumaSnapshotInputError::SidecarPathMismatch { .. })
    ));
    assert!(
        wrong_path_accumulator
            .finalize()
            .expect("wrong path accumulator")
            .is_none()
    );

    let absent_live = absent_snapshot(root.path(), "missing.dll");
    let missing_live_path = path_ref(&root.path().join("missing.dll"));
    let missing_sidecar_path = managed_sidecar_path(&missing_live_path).expect("sidecar");
    let mut missing_live_accumulator =
        LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::Uninstall);
    let error = lower(
        LumaHostDecision::ReleasePresent {
            live_path: &missing_live_path,
            sidecar_path: &missing_sidecar_path,
            live_snapshot: &absent_live,
            sidecar_snapshot: &sidecar,
        },
        &mut missing_live_accumulator,
    )
    .expect_err("absent live must reject");
    assert!(matches!(
        error,
        LumaHostLoweringError::Snapshot(LumaSnapshotInputError::ExpectedFile(_))
    ));

    let absent_sidecar = absent_snapshot(root.path(), "missing.dll.bak");
    let mut missing_sidecar_accumulator =
        LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::Uninstall);
    let error = lower(
        LumaHostDecision::ReleasePresent {
            live_path: &live_path,
            sidecar_path: &sidecar_path,
            live_snapshot: &live,
            sidecar_snapshot: &absent_sidecar,
        },
        &mut missing_sidecar_accumulator,
    )
    .expect_err("absent sidecar must reject");
    assert!(matches!(
        error,
        LumaHostLoweringError::Snapshot(LumaSnapshotInputError::ExpectedFile(_))
    ));
}
