use super::active_dlss::{
    ActiveDlssClassification, ActiveDlssClassificationError, ActiveDlssLoweringError,
    classify_active_dlss, lower_active_dlss_owned,
};
use super::effects::{LumaPeerEffectAccumulator, LumaPeerOperationOrder};
use crate::addons::luma::test_support::build_nvidia_dlss_pe;
use crate::coordinated_files::CatalogPathClaim;
use crate::peer_mutation_executor::{
    EndpointExpectation, EndpointPostcondition, PeerPathSnapshot, observe_peer_path_snapshot,
};
use renderpilot_domain::{ManagedFileBaseline, ManagedFileMode, PathRef, managed_sidecar_path};

fn path(path: &std::path::Path) -> PathRef {
    PathRef::new(path.to_string_lossy().into_owned()).expect("path")
}

fn digest(bytes: &[u8]) -> renderpilot_domain::Sha256Hash {
    renderpilot_detection::sha256_bytes(bytes).expect("digest")
}

fn empty_claim() -> CatalogPathClaim {
    CatalogPathClaim::for_test(Vec::new(), None)
}

fn snapshot(path: &PathRef, root: &PathRef) -> PeerPathSnapshot {
    observe_peer_path_snapshot(path, root).expect("snapshot")
}

fn classify(
    target: &PathRef,
    bundled: Option<&[u8]>,
    claim: &CatalogPathClaim,
    live: &PeerPathSnapshot,
) -> ActiveDlssClassification {
    classify_active_dlss(target, bundled.map(<[u8]>::to_vec), claim, live).expect("classification")
}

#[test]
fn no_payload_has_no_binding_or_owned_sidecar_request() {
    let root = tempfile::tempdir().expect("root");
    let target = path(&root.path().join("nvngx_dlss.dll"));
    let live = snapshot(&target, &path(root.path()));

    let plan = classify(&target, None, &empty_claim(), &live);

    assert!(matches!(plan, ActiveDlssClassification::NoPayload));
    assert!(plan.binding().is_none());
    assert!(plan.owned().is_none());
}

#[test]
fn absent_live_lowers_to_one_owned_create_and_requires_absent_sidecar() {
    let root = tempfile::tempdir().expect("root");
    let target = path(&root.path().join("nvngx_dlss.dll"));
    let root_ref = path(root.path());
    let bundled = build_nvidia_dlss_pe([3, 7, 0, 0]);
    let live = snapshot(&target, &root_ref);
    let plan = classify(&target, Some(&bundled), &empty_claim(), &live);
    let owned = plan.owned().expect("owned plan");
    assert_eq!(owned.binding().mode(), ManagedFileMode::Owned);
    assert!(matches!(
        owned.binding().baseline(),
        ManagedFileBaseline::Absent
    ));
    assert_eq!(owned.binding().installed_sha256(), &digest(&bundled));

    let sidecar = owned.sidecar_path().expect("sidecar path");
    let sidecar_snapshot = snapshot(&sidecar, &root_ref);
    let mut accumulator = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);
    lower_active_dlss_owned(
        plan.into_owned().expect("owned plan"),
        &live,
        &sidecar_snapshot,
        &mut accumulator,
    )
    .expect("lower create");
    let effects = accumulator
        .finalize()
        .expect("finalize")
        .expect("one endpoint");
    assert_eq!(effects.program().endpoints().len(), 1);
    assert_eq!(effects.program().endpoints()[0].path(), &target);
    assert!(matches!(
        effects.program().endpoints()[0].before(),
        EndpointExpectation::Absent
    ));
    assert!(matches!(
        effects.program().endpoints()[0].after(),
        EndpointPostcondition::File(hash) if hash == &digest(&bundled)
    ));
    assert_eq!(effects.payloads(), &[Some(bundled)]);
}

#[test]
fn newer_or_equal_live_is_reused_without_sidecar_observation_or_endpoint() {
    let root = tempfile::tempdir().expect("root");
    let target_path = root.path().join("nvngx_dlss.dll");
    let target = path(&target_path);
    let root_ref = path(root.path());
    let live_bytes = build_nvidia_dlss_pe([3, 8, 0, 0]);
    std::fs::write(&target_path, &live_bytes).expect("live");
    std::fs::write(root.path().join("nvngx_dlss.dll.bak"), b"foreign backup")
        .expect("foreign sidecar");
    let live = snapshot(&target, &root_ref);

    let plan = classify(
        &target,
        Some(&build_nvidia_dlss_pe([3, 7, 0, 0])),
        &empty_claim(),
        &live,
    );

    assert!(matches!(plan, ActiveDlssClassification::Reused { .. }));
    assert!(plan.owned().is_none());
    assert_eq!(
        plan.binding().expect("binding").mode(),
        ManagedFileMode::Reused
    );
}

#[test]
fn older_live_lowers_sidecar_create_before_dlss_replace() {
    let root = tempfile::tempdir().expect("root");
    let target_path = root.path().join("nvngx_dlss.dll");
    let target = path(&target_path);
    let root_ref = path(root.path());
    let original = build_nvidia_dlss_pe([3, 6, 0, 0]);
    let bundled = build_nvidia_dlss_pe([3, 7, 0, 0]);
    std::fs::write(&target_path, &original).expect("live");
    let live = snapshot(&target, &root_ref);
    let plan = classify(&target, Some(&bundled), &empty_claim(), &live);
    let owned = plan.owned().expect("owned plan");
    assert!(matches!(
        owned.binding().baseline(),
        ManagedFileBaseline::Present { sha256 } if sha256 == &digest(&original)
    ));
    assert_eq!(owned.binding().installed_sha256(), &digest(&bundled));

    let sidecar = managed_sidecar_path(&target).expect("sidecar");
    let sidecar_snapshot = snapshot(&sidecar, &root_ref);
    let mut accumulator = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);
    lower_active_dlss_owned(
        plan.into_owned().expect("owned plan"),
        &live,
        &sidecar_snapshot,
        &mut accumulator,
    )
    .expect("lower replacement");
    let effects = accumulator
        .finalize()
        .expect("finalize")
        .expect("two endpoints");
    assert_eq!(
        effects
            .program()
            .endpoints()
            .iter()
            .map(|endpoint| endpoint.path().clone())
            .collect::<Vec<_>>(),
        vec![sidecar, target]
    );
    assert_eq!(effects.payloads(), &[Some(original), Some(bundled)]);
    assert!(matches!(
        effects.program().endpoints()[0].role(),
        renderpilot_domain::PeerEndpointRole::Disjoint
    ));
    assert!(matches!(
        effects.program().endpoints()[1].role(),
        renderpilot_domain::PeerEndpointRole::Disjoint
    ));
}

#[test]
fn invalid_bundled_and_live_bytes_fail_closed_before_lowering() {
    let root = tempfile::tempdir().expect("root");
    let target = path(&root.path().join("nvngx_dlss.dll"));
    let root_ref = path(root.path());
    let absent = snapshot(&target, &root_ref);
    assert!(matches!(
        classify_active_dlss(&target, Some(b"not a PE".to_vec()), &empty_claim(), &absent,),
        Err(ActiveDlssClassificationError::InvalidBundled(_))
    ));

    std::fs::write(root.path().join("nvngx_dlss.dll"), b"not a PE").expect("invalid live");
    let invalid_live = snapshot(&target, &root_ref);
    let bundled = build_nvidia_dlss_pe([3, 7, 0, 0]);
    assert!(matches!(
        classify_active_dlss(&target, Some(bundled), &empty_claim(), &invalid_live,),
        Err(ActiveDlssClassificationError::InvalidLive { .. })
    ));
}

#[test]
fn incompatible_live_fails_closed_before_sidecar() {
    let root = tempfile::tempdir().expect("root");
    let target_path = root.path().join("nvngx_dlss.dll");
    let target = path(&target_path);
    let root_ref = path(root.path());
    let live_bytes = build_nvidia_dlss_pe([1, 5, 0, 0]);
    std::fs::write(&target_path, &live_bytes).expect("live");
    let live = snapshot(&target, &root_ref);
    let bundled = build_nvidia_dlss_pe([3, 7, 0, 0]);

    assert!(matches!(
        classify_active_dlss(&target, Some(bundled), &empty_claim(), &live),
        Err(ActiveDlssClassificationError::IncompatibleLive { .. })
    ));
}

#[test]
fn catalog_exact_live_is_reused_but_missing_drift_and_older_are_forbidden() {
    let root = tempfile::tempdir().expect("root");
    let target_path = root.path().join("nvngx_dlss.dll");
    let target = path(&target_path);
    let root_ref = path(root.path());
    let live_bytes = build_nvidia_dlss_pe([3, 8, 0, 0]);
    let bundled = build_nvidia_dlss_pe([3, 7, 0, 0]);
    let live_hash = digest(&live_bytes);
    let claim = CatalogPathClaim::for_test(vec![live_hash.clone()], None);

    let missing = snapshot(&target, &root_ref);
    assert!(matches!(
        classify_active_dlss(&target, Some(bundled.clone()), &claim, &missing),
        Err(ActiveDlssClassificationError::CatalogMissing(_))
    ));

    std::fs::write(&target_path, &live_bytes).expect("live");
    std::fs::write(root.path().join("nvngx_dlss.dll.bak"), b"foreign backup")
        .expect("foreign sidecar");
    let exact = snapshot(&target, &root_ref);
    let reused = classify(&target, Some(&bundled), &claim, &exact);
    assert!(matches!(reused, ActiveDlssClassification::Reused { .. }));
    assert!(reused.owned().is_none());
    assert_eq!(
        reused.binding().expect("reused binding").installed_sha256(),
        &live_hash
    );

    let drift_bytes = build_nvidia_dlss_pe([3, 9, 0, 0]);
    std::fs::write(&target_path, &drift_bytes).expect("drift");
    let drift = snapshot(&target, &root_ref);
    assert!(matches!(
        classify_active_dlss(&target, Some(bundled.clone()), &claim, &drift),
        Err(ActiveDlssClassificationError::CatalogDrift { .. })
    ));

    let old_bytes = build_nvidia_dlss_pe([3, 6, 0, 0]);
    std::fs::write(&target_path, &old_bytes).expect("old");
    let old_hash = digest(&old_bytes);
    let old_claim = CatalogPathClaim::for_test(vec![old_hash], None);
    let old = snapshot(&target, &root_ref);
    assert!(matches!(
        classify_active_dlss(&target, Some(bundled), &old_claim, &old),
        Err(ActiveDlssClassificationError::CatalogReplacementForbidden { .. })
    ));
}

#[test]
fn owned_lowering_rejects_a_present_sidecar() {
    let root = tempfile::tempdir().expect("root");
    let target_path = root.path().join("nvngx_dlss.dll");
    let target = path(&target_path);
    let root_ref = path(root.path());
    let bundled = build_nvidia_dlss_pe([3, 7, 0, 0]);
    let live = snapshot(&target, &root_ref);
    let plan = classify(&target, Some(&bundled), &empty_claim(), &live);
    let sidecar = plan
        .owned()
        .expect("owned plan")
        .sidecar_path()
        .expect("sidecar");
    std::fs::write(sidecar.as_str(), b"foreign").expect("sidecar");
    let sidecar_snapshot = snapshot(&sidecar, &root_ref);
    let mut accumulator = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);

    assert!(matches!(
        lower_active_dlss_owned(
            plan.into_owned().expect("owned plan"),
            &live,
            &sidecar_snapshot,
            &mut accumulator,
        ),
        Err(ActiveDlssLoweringError::SnapshotExpectedAbsent(_))
    ));
    assert!(accumulator.finalize().expect("finalize").is_none());

    let older = build_nvidia_dlss_pe([3, 6, 0, 0]);
    std::fs::write(&target_path, &older).expect("older live");
    let live = snapshot(&target, &root_ref);
    let replacement = classify(&target, Some(&bundled), &empty_claim(), &live);
    let mut accumulator = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);
    assert!(matches!(
        lower_active_dlss_owned(
            replacement.into_owned().expect("owned replacement"),
            &live,
            &sidecar_snapshot,
            &mut accumulator,
        ),
        Err(ActiveDlssLoweringError::SnapshotExpectedAbsent(_))
    ));
    assert!(accumulator.finalize().expect("finalize").is_none());
}
