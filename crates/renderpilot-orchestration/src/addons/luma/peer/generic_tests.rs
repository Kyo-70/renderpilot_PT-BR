use super::effects::{
    LumaPeerEffectAccumulator, LumaPeerEffectGroup, LumaPeerEffects, LumaPeerOperationOrder,
};
use super::generic::{LumaGenericDecision, LumaGenericLoweringError, lower_generic_decision};
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
        .expect("generic effects")
}

#[test]
fn unchanged_preserves_a_preexisting_non_generic_effect() {
    let mut accumulator = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);
    accumulator
        .create(
            LumaPeerEffectGroup::Host,
            PathRef::parse_exact("C:/renderpilot-tests/luma-generic/game/host.dll").expect("path"),
            b"host".to_vec(),
        )
        .expect("host effect");
    lower_generic_decision(LumaGenericDecision::Unchanged, &mut accumulator).expect("unchanged");

    let effects = finalize(accumulator);
    assert_eq!(effects.program().endpoints().len(), 1);
    assert_eq!(
        effects.program().endpoints()[0].role(),
        PeerEndpointRole::TopologyDownstream
    );
}

#[test]
fn create_requires_absence_and_accepts_empty_payload() {
    let root = tempfile::tempdir().expect("root");
    let live_path = path_ref(&root.path().join("generic.dll"));
    let absent = absent_snapshot(root.path(), "generic.dll");
    let mut accumulator = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);
    lower_generic_decision(
        LumaGenericDecision::Create {
            live_path: &live_path,
            live_snapshot: &absent,
            prepared_bytes: Vec::new(),
        },
        &mut accumulator,
    )
    .expect("generic create");
    let effects = finalize(accumulator);
    let endpoint = &effects.program().endpoints()[0];
    assert_eq!(endpoint.role(), PeerEndpointRole::Disjoint);
    assert!(matches!(endpoint.before(), EndpointExpectation::Absent));
    assert_eq!(effects.payloads(), &[Some(Vec::new())]);

    let present = snapshot(root.path(), "present.dll", b"foreign");
    let present_path = path_ref(&root.path().join("present.dll"));
    let mut rejected = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);
    let error = lower_generic_decision(
        LumaGenericDecision::Create {
            live_path: &present_path,
            live_snapshot: &present,
            prepared_bytes: b"new".to_vec(),
        },
        &mut rejected,
    )
    .expect_err("present create must reject");
    assert!(matches!(
        error,
        LumaGenericLoweringError::Snapshot(LumaSnapshotInputError::ExpectedAbsent(_))
    ));
    assert!(rejected.finalize().expect("rejected").is_none());
}

#[test]
fn acquire_foreign_uses_live_original_and_canonical_sidecar_pair() {
    let root = tempfile::tempdir().expect("root");
    let live_path = path_ref(&root.path().join("generic.dll"));
    let sidecar_path = managed_sidecar_path(&live_path).expect("sidecar");
    let live = snapshot(root.path(), "generic.dll", b"foreign");
    let sidecar = absent_snapshot(root.path(), "generic.dll.bak");
    let mut accumulator = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);
    lower_generic_decision(
        LumaGenericDecision::AcquireForeign {
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
    assert_eq!(endpoints[0].role(), PeerEndpointRole::Disjoint);
    assert_eq!(endpoints[1].path(), &live_path);
    assert_eq!(endpoints[1].role(), PeerEndpointRole::Disjoint);
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
fn acquire_foreign_rejects_bad_states_or_sidecar_path_without_effects() {
    let root = tempfile::tempdir().expect("root");
    let live_path = path_ref(&root.path().join("generic.dll"));
    let sidecar_path = managed_sidecar_path(&live_path).expect("sidecar");
    let live = snapshot(root.path(), "generic.dll", b"foreign");
    let absent_live = absent_snapshot(root.path(), "missing.dll");
    let absent_sidecar = absent_snapshot(root.path(), "generic.dll.bak");

    let wrong_sidecar = path_ref(&root.path().join("other.dll.bak"));
    let mut wrong_path = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);
    let error = lower_generic_decision(
        LumaGenericDecision::AcquireForeign {
            live_path: &live_path,
            sidecar_path: &wrong_sidecar,
            live_snapshot: &live,
            sidecar_snapshot: &absent_sidecar,
            prepared_bytes: b"managed".to_vec(),
        },
        &mut wrong_path,
    )
    .expect_err("wrong sidecar path must reject");
    assert!(matches!(
        error,
        LumaGenericLoweringError::Snapshot(LumaSnapshotInputError::SidecarPathMismatch { .. })
    ));
    assert!(wrong_path.finalize().expect("wrong path").is_none());

    let present_sidecar = snapshot(root.path(), "generic.dll.bak", b"existing");
    let mut occupied = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);
    let error = lower_generic_decision(
        LumaGenericDecision::AcquireForeign {
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
        LumaGenericLoweringError::Snapshot(LumaSnapshotInputError::ExpectedAbsent(_))
    ));
    assert!(occupied.finalize().expect("occupied").is_none());

    let missing_path = path_ref(&root.path().join("missing.dll"));
    let missing_sidecar = managed_sidecar_path(&missing_path).expect("missing sidecar");
    let mut absent = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);
    let error = lower_generic_decision(
        LumaGenericDecision::AcquireForeign {
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
        LumaGenericLoweringError::Snapshot(LumaSnapshotInputError::ExpectedFile(_))
    ));
    assert!(absent.finalize().expect("absent live").is_none());
}

#[test]
fn replace_owned_requires_file_and_keeps_disjoint_role() {
    let root = tempfile::tempdir().expect("root");
    let live_path = path_ref(&root.path().join("generic.dll"));
    let live = snapshot(root.path(), "generic.dll", b"old");
    let mut accumulator = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);
    lower_generic_decision(
        LumaGenericDecision::ReplaceOwned {
            live_path: &live_path,
            live_snapshot: &live,
            prepared_bytes: b"new".to_vec(),
        },
        &mut accumulator,
    )
    .expect("replace");
    let effects = finalize(accumulator);
    assert_eq!(
        effects.program().endpoints()[0].role(),
        PeerEndpointRole::Disjoint
    );
    assert_eq!(effects.payloads(), &[Some(b"new".to_vec())]);

    let absent = absent_snapshot(root.path(), "absent.dll");
    let absent_path = path_ref(&root.path().join("absent.dll"));
    let mut rejected = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);
    let error = lower_generic_decision(
        LumaGenericDecision::ReplaceOwned {
            live_path: &absent_path,
            live_snapshot: &absent,
            prepared_bytes: b"new".to_vec(),
        },
        &mut rejected,
    )
    .expect_err("absent replace must reject");
    assert!(matches!(
        error,
        LumaGenericLoweringError::Snapshot(LumaSnapshotInputError::ExpectedFile(_))
    ));
}

#[test]
fn remove_created_requires_file_and_emits_only_generic_remove() {
    let root = tempfile::tempdir().expect("root");
    let live_path = path_ref(&root.path().join("generic.dll"));
    let live = snapshot(root.path(), "generic.dll", b"created");
    let mut accumulator = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::Uninstall);
    lower_generic_decision(
        LumaGenericDecision::RemoveCreated {
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

    let absent = absent_snapshot(root.path(), "absent.dll");
    let absent_path = path_ref(&root.path().join("absent.dll"));
    let mut rejected = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::Uninstall);
    let error = lower_generic_decision(
        LumaGenericDecision::RemoveCreated {
            live_path: &absent_path,
            live_snapshot: &absent,
        },
        &mut rejected,
    )
    .expect_err("absent remove must reject");
    assert!(matches!(
        error,
        LumaGenericLoweringError::Snapshot(LumaSnapshotInputError::ExpectedFile(_))
    ));
}

#[test]
fn release_backed_restores_sidecar_baseline_then_removes_sidecar() {
    let root = tempfile::tempdir().expect("root");
    let live_path = path_ref(&root.path().join("generic.dll"));
    let sidecar_path = managed_sidecar_path(&live_path).expect("sidecar");
    let live = snapshot(root.path(), "generic.dll", b"managed");
    let sidecar = snapshot(root.path(), "generic.dll.bak", b"original");
    let mut accumulator = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::Uninstall);
    lower_generic_decision(
        LumaGenericDecision::ReleaseBacked {
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
    assert_eq!(endpoints[0].role(), PeerEndpointRole::Disjoint);
    assert!(matches!(
        endpoints[0].after(),
        EndpointPostcondition::File(_)
    ));
    assert_eq!(
        normalized_path_key(endpoints[1].path().as_str()),
        normalized_path_key(sidecar_path.as_str())
    );
    assert_eq!(endpoints[1].role(), PeerEndpointRole::Disjoint);
    assert!(matches!(
        endpoints[1].after(),
        EndpointPostcondition::Absent
    ));
    assert_eq!(effects.payloads(), &[Some(b"original".to_vec()), None]);
}

#[test]
fn release_backed_rejects_absence_or_wrong_sidecar_without_effects() {
    let root = tempfile::tempdir().expect("root");
    let live_path = path_ref(&root.path().join("generic.dll"));
    let sidecar_path = managed_sidecar_path(&live_path).expect("sidecar");
    let live = snapshot(root.path(), "generic.dll", b"managed");
    let sidecar = snapshot(root.path(), "generic.dll.bak", b"original");

    let wrong_sidecar = path_ref(&root.path().join("other.dll.bak"));
    let mut wrong_path = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::Uninstall);
    let error = lower_generic_decision(
        LumaGenericDecision::ReleaseBacked {
            live_path: &live_path,
            sidecar_path: &wrong_sidecar,
            live_snapshot: &live,
            sidecar_snapshot: &sidecar,
        },
        &mut wrong_path,
    )
    .expect_err("wrong sidecar path must reject");
    assert!(matches!(
        error,
        LumaGenericLoweringError::Snapshot(LumaSnapshotInputError::SidecarPathMismatch { .. })
    ));
    assert!(wrong_path.finalize().expect("wrong path").is_none());

    let absent_live = absent_snapshot(root.path(), "missing.dll");
    let missing_live_path = path_ref(&root.path().join("missing.dll"));
    let missing_sidecar = managed_sidecar_path(&missing_live_path).expect("sidecar");
    let mut missing_live_accumulator =
        LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::Uninstall);
    let error = lower_generic_decision(
        LumaGenericDecision::ReleaseBacked {
            live_path: &missing_live_path,
            sidecar_path: &missing_sidecar,
            live_snapshot: &absent_live,
            sidecar_snapshot: &sidecar,
        },
        &mut missing_live_accumulator,
    )
    .expect_err("absent live must reject");
    assert!(matches!(
        error,
        LumaGenericLoweringError::Snapshot(LumaSnapshotInputError::ExpectedFile(_))
    ));
    assert!(
        missing_live_accumulator
            .finalize()
            .expect("missing live")
            .is_none()
    );

    let absent_sidecar = absent_snapshot(root.path(), "missing.dll.bak");
    let mut missing_sidecar_accumulator =
        LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::Uninstall);
    let error = lower_generic_decision(
        LumaGenericDecision::ReleaseBacked {
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
        LumaGenericLoweringError::Snapshot(LumaSnapshotInputError::ExpectedFile(_))
    ));
}
