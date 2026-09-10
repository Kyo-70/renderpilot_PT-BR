use sha2::{Digest, Sha256};

use super::cascade::{
    LumaDlssCascadeDecision, LumaDlssCascadeLoweringError, lower_dlss_cascade_decision,
};
use super::effects::{LumaPeerEffectAccumulator, LumaPeerEffects, LumaPeerOperationOrder};
use crate::peer_mutation_executor::{
    EndpointExpectation, EndpointPostcondition, PeerPathSnapshot, observe_peer_path_snapshot,
};
use renderpilot_domain::{PathRef, PeerEndpointRole, Sha256Hash, managed_sidecar_path};

fn path(value: &std::path::Path) -> PathRef {
    PathRef::new(value.to_string_lossy().into_owned()).expect("path")
}

fn digest(bytes: &[u8]) -> Sha256Hash {
    Sha256Hash::new(hex::encode(Sha256::digest(bytes))).expect("digest")
}

fn snapshot(path: &PathRef, root: &PathRef) -> PeerPathSnapshot {
    observe_peer_path_snapshot(path, root).expect("snapshot")
}

fn effects(accumulator: LumaPeerEffectAccumulator) -> LumaPeerEffects {
    accumulator
        .finalize()
        .expect("finalize")
        .expect("expected physical effects")
}

fn setup(
    live_bytes: Option<&[u8]>,
    sidecar_bytes: Option<&[u8]>,
) -> (
    tempfile::TempDir,
    PathRef,
    PathRef,
    PeerPathSnapshot,
    PeerPathSnapshot,
) {
    let root = tempfile::tempdir().expect("root");
    let live_path = root.path().join("dlss.dll");
    if let Some(bytes) = live_bytes {
        std::fs::write(&live_path, bytes).expect("live");
    }
    let live = path(&live_path);
    let sidecar = managed_sidecar_path(&live).expect("managed sidecar");
    if let Some(bytes) = sidecar_bytes {
        std::fs::write(sidecar.as_str(), bytes).expect("sidecar");
    }
    let root_ref = path(root.path());
    let live_snapshot = snapshot(&live, &root_ref);
    let sidecar_snapshot = snapshot(&sidecar, &root_ref);
    (root, live, sidecar, live_snapshot, sidecar_snapshot)
}

#[test]
fn current_only_removes_live_with_exact_active_image() {
    let (_root, live, _sidecar, live_snapshot, _sidecar_snapshot) =
        setup(Some(b"active"), Some(b"orphan sidecar"));
    let active = digest(b"active");
    let mut accumulator = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::Uninstall);
    lower_dlss_cascade_decision(
        LumaDlssCascadeDecision::CurrentOnly {
            live_path: &live,
            live_snapshot: &live_snapshot,
            expected_active_digest: &active,
        },
        &mut accumulator,
    )
    .expect("current-only lowering");

    let effects = effects(accumulator);
    let endpoint = &effects.program().endpoints()[0];
    assert_eq!(endpoint.path(), &live);
    assert_eq!(endpoint.role(), PeerEndpointRole::Disjoint);
    assert!(
        matches!(endpoint.before(), EndpointExpectation::File(file) if file.digest() == &active)
    );
    assert!(matches!(endpoint.after(), EndpointPostcondition::Absent));
    assert_eq!(effects.payloads(), &[None]);
}

#[test]
fn present_baseline_restores_live_then_removes_sidecar() {
    let (_root, live, sidecar, live_snapshot, sidecar_snapshot) =
        setup(Some(b"active"), Some(b"baseline"));
    let active = digest(b"active");
    let baseline = digest(b"baseline");
    let mut accumulator = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::Uninstall);
    lower_dlss_cascade_decision(
        LumaDlssCascadeDecision::RestorePresent {
            live_path: &live,
            sidecar_path: &sidecar,
            live_snapshot: &live_snapshot,
            sidecar_snapshot: &sidecar_snapshot,
            expected_active_digest: &active,
            expected_baseline_digest: &baseline,
        },
        &mut accumulator,
    )
    .expect("present restore lowering");

    let effects = effects(accumulator);
    assert_eq!(
        effects
            .program()
            .endpoints()
            .iter()
            .map(|endpoint| endpoint.path())
            .collect::<Vec<_>>(),
        [&live, &sidecar]
    );
    let endpoints = effects.program().endpoints();
    assert_eq!(endpoints[0].role(), PeerEndpointRole::Disjoint);
    assert_eq!(endpoints[1].role(), PeerEndpointRole::Disjoint);
    assert!(
        matches!(endpoints[0].before(), EndpointExpectation::File(file) if file.digest() == &active)
    );
    assert!(matches!(endpoints[0].after(), EndpointPostcondition::File(hash) if hash == &baseline));
    assert!(
        matches!(endpoints[1].before(), EndpointExpectation::File(file) if file.digest() == &baseline)
    );
    assert!(matches!(
        endpoints[1].after(),
        EndpointPostcondition::Absent
    ));
    assert_eq!(effects.payloads(), &[Some(b"baseline".to_vec()), None]);
}

#[test]
fn missing_live_restores_from_sidecar_then_removes_sidecar() {
    let (_root, live, sidecar, live_snapshot, sidecar_snapshot) = setup(None, Some(b"baseline"));
    let baseline = digest(b"baseline");
    let mut accumulator = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::Uninstall);
    lower_dlss_cascade_decision(
        LumaDlssCascadeDecision::RestoreMissingLive {
            live_path: &live,
            sidecar_path: &sidecar,
            live_snapshot: &live_snapshot,
            sidecar_snapshot: &sidecar_snapshot,
            expected_baseline_digest: &baseline,
        },
        &mut accumulator,
    )
    .expect("missing-live restore lowering");

    let effects = effects(accumulator);
    let endpoints = effects.program().endpoints();
    assert_eq!(endpoints[0].path(), &live);
    assert_eq!(endpoints[0].role(), PeerEndpointRole::Disjoint);
    assert!(matches!(endpoints[0].before(), EndpointExpectation::Absent));
    assert!(matches!(endpoints[0].after(), EndpointPostcondition::File(hash) if hash == &baseline));
    assert_eq!(endpoints[1].path(), &sidecar);
    assert_eq!(endpoints[1].role(), PeerEndpointRole::Disjoint);
    assert!(
        matches!(endpoints[1].before(), EndpointExpectation::File(file) if file.digest() == &baseline)
    );
    assert!(matches!(
        endpoints[1].after(),
        EndpointPostcondition::Absent
    ));
    assert_eq!(effects.payloads(), &[Some(b"baseline".to_vec()), None]);
}

#[test]
fn already_live_baseline_removes_only_present_sidecar() {
    let (_root, live, sidecar, live_snapshot, sidecar_snapshot) =
        setup(Some(b"baseline"), Some(b"baseline"));
    let baseline = digest(b"baseline");
    let mut accumulator = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::Uninstall);
    lower_dlss_cascade_decision(
        LumaDlssCascadeDecision::BaselineAlreadyLive {
            live_path: &live,
            sidecar_path: &sidecar,
            live_snapshot: &live_snapshot,
            sidecar_snapshot: &sidecar_snapshot,
            expected_active_digest: &baseline,
            expected_baseline_digest: &baseline,
        },
        &mut accumulator,
    )
    .expect("already-live lowering");

    let effects = effects(accumulator);
    assert_eq!(effects.program().endpoints().len(), 1);
    assert_eq!(effects.program().endpoints()[0].path(), &sidecar);
    assert!(matches!(
        effects.program().endpoints()[0].after(),
        EndpointPostcondition::Absent
    ));
    assert_eq!(effects.payloads(), &[None]);
}

#[test]
fn already_live_without_sidecar_has_no_physical_effects() {
    let (_root, live, sidecar, live_snapshot, sidecar_snapshot) = setup(Some(b"baseline"), None);
    let baseline = digest(b"baseline");
    let mut accumulator = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::Uninstall);
    lower_dlss_cascade_decision(
        LumaDlssCascadeDecision::BaselineAlreadyLiveWithoutSidecar {
            live_path: &live,
            sidecar_path: &sidecar,
            live_snapshot: &live_snapshot,
            sidecar_snapshot: &sidecar_snapshot,
            expected_active_digest: &baseline,
            expected_baseline_digest: &baseline,
        },
        &mut accumulator,
    )
    .expect("already-live no-sidecar lowering");
    assert!(accumulator.finalize().expect("finalize").is_none());
}

#[test]
fn lowering_rejects_wrong_catalog_digest_and_byte_identical_replace() {
    let (_root, live, sidecar, live_snapshot, sidecar_snapshot) =
        setup(Some(b"active"), Some(b"baseline"));
    let wrong = digest(b"wrong");
    let baseline = digest(b"baseline");
    let mut accumulator = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::Uninstall);
    let error = lower_dlss_cascade_decision(
        LumaDlssCascadeDecision::RestorePresent {
            live_path: &live,
            sidecar_path: &sidecar,
            live_snapshot: &live_snapshot,
            sidecar_snapshot: &sidecar_snapshot,
            expected_active_digest: &wrong,
            expected_baseline_digest: &baseline,
        },
        &mut accumulator,
    )
    .expect_err("wrong active digest must fail closed");
    assert!(matches!(
        error,
        LumaDlssCascadeLoweringError::ExpectedDigestMismatch { .. }
    ));

    let (_root, live, sidecar, live_snapshot, sidecar_snapshot) =
        setup(Some(b"same"), Some(b"same"));
    let same = digest(b"same");
    let mut accumulator = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::Uninstall);
    let error = lower_dlss_cascade_decision(
        LumaDlssCascadeDecision::RestorePresent {
            live_path: &live,
            sidecar_path: &sidecar,
            live_snapshot: &live_snapshot,
            sidecar_snapshot: &sidecar_snapshot,
            expected_active_digest: &same,
            expected_baseline_digest: &same,
        },
        &mut accumulator,
    )
    .expect_err("byte-identical replace must fail closed");
    assert!(matches!(
        error,
        LumaDlssCascadeLoweringError::RestoreWouldBeByteIdentical(_)
    ));
}

#[test]
fn lowering_rejects_wrong_sidecar_shape_and_noncanonical_route() {
    let (_root, live, _sidecar, live_snapshot, sidecar_snapshot) =
        setup(Some(b"active"), Some(b"baseline"));
    let active = digest(b"active");
    let baseline = digest(b"baseline");
    let mut accumulator = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::Uninstall);
    let error = lower_dlss_cascade_decision(
        LumaDlssCascadeDecision::RestorePresent {
            live_path: &live,
            sidecar_path: &live,
            live_snapshot: &live_snapshot,
            sidecar_snapshot: &sidecar_snapshot,
            expected_active_digest: &active,
            expected_baseline_digest: &baseline,
        },
        &mut accumulator,
    )
    .expect_err("noncanonical sidecar must fail closed");
    assert!(matches!(
        error,
        LumaDlssCascadeLoweringError::SidecarPathMismatch { .. }
    ));

    let (_root, live, sidecar, live_snapshot, sidecar_snapshot) = setup(Some(b"baseline"), None);
    let baseline = digest(b"baseline");
    let mut accumulator = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::Uninstall);
    let error = lower_dlss_cascade_decision(
        LumaDlssCascadeDecision::BaselineAlreadyLive {
            live_path: &live,
            sidecar_path: &sidecar,
            live_snapshot: &live_snapshot,
            sidecar_snapshot: &sidecar_snapshot,
            expected_active_digest: &baseline,
            expected_baseline_digest: &baseline,
        },
        &mut accumulator,
    )
    .expect_err("missing sidecar must fail closed for present-baseline shape");
    assert!(matches!(
        error,
        LumaDlssCascadeLoweringError::ExpectedFile(_)
    ));

    let (_root, live, sidecar, live_snapshot, sidecar_snapshot) =
        setup(Some(b"active"), Some(b"baseline"));
    let active = digest(b"active");
    let wrong_baseline = digest(b"other baseline");
    let mut accumulator = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::Uninstall);
    let error = lower_dlss_cascade_decision(
        LumaDlssCascadeDecision::RestorePresent {
            live_path: &live,
            sidecar_path: &sidecar,
            live_snapshot: &live_snapshot,
            sidecar_snapshot: &sidecar_snapshot,
            expected_active_digest: &active,
            expected_baseline_digest: &wrong_baseline,
        },
        &mut accumulator,
    )
    .expect_err("wrong sidecar digest must fail closed");
    assert!(matches!(
        error,
        LumaDlssCascadeLoweringError::ExpectedDigestMismatch { .. }
    ));
}
