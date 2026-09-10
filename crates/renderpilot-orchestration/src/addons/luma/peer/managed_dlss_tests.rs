use std::path::{Path, PathBuf};

use renderpilot_domain::{
    ManagedAddonFile, ManagedFileBaseline, PathRef, Sha256Hash, managed_sidecar_path,
};

use crate::addons::luma::dlss::PlannedDlss;
use crate::coordinated_files::CoordinatedFilePlan;

use super::effects::{LumaPeerEffectAccumulator, LumaPeerEffects, LumaPeerOperationOrder};
use super::managed_dlss::{ManagedDlssUninstallError, lower_managed_dlss_uninstall};
use crate::peer_mutation_executor::{EndpointExpectation, EndpointPostcondition};

fn path_ref(path: &Path) -> PathRef {
    PathRef::new(path.to_string_lossy().into_owned()).expect("path")
}

fn digest(bytes: &[u8]) -> Sha256Hash {
    renderpilot_detection::sha256_bytes(bytes).expect("digest")
}

fn owned(path: &PathRef, baseline: ManagedFileBaseline, installed: &[u8]) -> ManagedAddonFile {
    ManagedAddonFile::owned(path.clone(), baseline, digest(installed))
}

fn plan_remove(path: &PathRef, expected_live: Vec<Sha256Hash>) -> PlannedDlss {
    PlannedDlss {
        action: CoordinatedFilePlan::RemoveAndRelease {
            path: PathBuf::from(path.as_str()),
            expected_live,
        },
        binding: None,
    }
}

fn plan_restore(
    path: &PathRef,
    baseline: &Sha256Hash,
    expected_live: Vec<Sha256Hash>,
) -> PlannedDlss {
    PlannedDlss {
        action: CoordinatedFilePlan::RestoreAndRelease {
            path: PathBuf::from(path.as_str()),
            baseline_sha256: baseline.clone(),
            expected_live,
        },
        binding: None,
    }
}

fn finalize(accumulator: LumaPeerEffectAccumulator) -> LumaPeerEffects {
    accumulator
        .finalize()
        .expect("finalize")
        .expect("managed DLSS effects")
}

#[test]
fn reused_keep_is_a_noop_even_with_an_orphan_sidecar() {
    let root = tempfile::tempdir().expect("root");
    let live = root.path().join("nvngx_dlss.dll");
    std::fs::write(&live, b"foreign").expect("live");
    std::fs::write(root.path().join("nvngx_dlss.dll.bak"), b"orphan").expect("sidecar");
    let live = path_ref(&live);
    let persisted = ManagedAddonFile::reused(live, digest(b"foreign"));
    let plan = PlannedDlss {
        action: CoordinatedFilePlan::Keep,
        binding: None,
    };
    let mut accumulator = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::Uninstall);

    lower_managed_dlss_uninstall(&persisted, &plan, &path_ref(root.path()), &mut accumulator)
        .expect("reused release");
    assert!(accumulator.finalize().expect("finalize").is_none());
}

#[test]
fn owned_absent_baseline_removes_the_exact_live_file() {
    let root = tempfile::tempdir().expect("root");
    let live_path = root.path().join("nvngx_dlss.dll");
    std::fs::write(&live_path, b"managed").expect("live");
    let live = path_ref(&live_path);
    let persisted = owned(&live, ManagedFileBaseline::Absent, b"managed");
    let plan = plan_remove(&live, vec![digest(b"managed")]);
    let mut accumulator = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::Uninstall);

    lower_managed_dlss_uninstall(&persisted, &plan, &path_ref(root.path()), &mut accumulator)
        .expect("remove release");
    let effects = finalize(accumulator);
    let endpoint = &effects.program().endpoints()[0];
    assert_eq!(endpoint.path(), &live);
    assert!(matches!(endpoint.before(), EndpointExpectation::File(_)));
    assert!(matches!(endpoint.after(), EndpointPostcondition::Absent));
    assert_eq!(effects.payloads(), &[None]);
}

#[test]
fn owned_present_baseline_restores_live_then_removes_sidecar() {
    let root = tempfile::tempdir().expect("root");
    let live_path = root.path().join("nvngx_dlss.dll");
    let sidecar_path = root.path().join("nvngx_dlss.dll.bak");
    std::fs::write(&live_path, b"managed").expect("live");
    std::fs::write(&sidecar_path, b"baseline").expect("sidecar");
    let live = path_ref(&live_path);
    let sidecar = managed_sidecar_path(&live).expect("sidecar path");
    let baseline = digest(b"baseline");
    let persisted = owned(
        &live,
        ManagedFileBaseline::Present {
            sha256: baseline.clone(),
        },
        b"managed",
    );
    let plan = plan_restore(&live, &baseline, vec![digest(b"managed")]);
    let mut accumulator = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::Uninstall);

    lower_managed_dlss_uninstall(&persisted, &plan, &path_ref(root.path()), &mut accumulator)
        .expect("restore release");
    let effects = finalize(accumulator);
    let endpoints = effects.program().endpoints();
    assert_eq!(endpoints.len(), 2);
    assert_eq!(endpoints[0].path(), &live);
    assert!(matches!(
        endpoints[0].before(),
        EndpointExpectation::File(_)
    ));
    assert!(matches!(endpoints[0].after(), EndpointPostcondition::File(hash) if hash == &baseline));
    assert_eq!(endpoints[1].path(), &sidecar);
    assert!(matches!(
        endpoints[1].before(),
        EndpointExpectation::File(_)
    ));
    assert!(matches!(
        endpoints[1].after(),
        EndpointPostcondition::Absent
    ));
    assert_eq!(effects.payloads(), &[Some(b"baseline".to_vec()), None]);
}

#[test]
fn owned_catalog_consumed_keep_is_rejected() {
    let root = tempfile::tempdir().expect("root");
    let live = path_ref(&root.path().join("nvngx_dlss.dll"));
    let persisted = owned(&live, ManagedFileBaseline::Absent, b"managed");
    let plan = PlannedDlss {
        action: CoordinatedFilePlan::Keep,
        binding: None,
    };
    let mut accumulator = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::Uninstall);

    let error =
        lower_managed_dlss_uninstall(&persisted, &plan, &path_ref(root.path()), &mut accumulator)
            .expect_err("owned Keep must use the catalog route");
    assert!(matches!(error, ManagedDlssUninstallError::InvalidPlan(_)));
    assert!(accumulator.finalize().expect("finalize").is_none());
}

#[test]
fn release_rejects_wrong_expected_set_or_live_digest() {
    let root = tempfile::tempdir().expect("root");
    let live_path = root.path().join("nvngx_dlss.dll");
    std::fs::write(&live_path, b"actual").expect("live");
    let live = path_ref(&live_path);
    let persisted = owned(&live, ManagedFileBaseline::Absent, b"installed");

    let mut empty_expected = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::Uninstall);
    let error = lower_managed_dlss_uninstall(
        &persisted,
        &plan_remove(&live, Vec::new()),
        &path_ref(root.path()),
        &mut empty_expected,
    )
    .expect_err("empty expected set");
    assert!(matches!(
        error,
        ManagedDlssUninstallError::InvalidExpectedLive(_)
    ));

    let mut wrong_live = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::Uninstall);
    let error = lower_managed_dlss_uninstall(
        &persisted,
        &plan_remove(&live, vec![digest(b"installed")]),
        &path_ref(root.path()),
        &mut wrong_live,
    )
    .expect_err("wrong live digest");
    assert!(matches!(
        error,
        ManagedDlssUninstallError::ExpectedDigestMismatch { .. }
    ));

    let missing_path = root.path().join("missing.dll");
    let missing = path_ref(&missing_path);
    let missing_claim = owned(&missing, ManagedFileBaseline::Absent, b"installed");
    let mut missing_live = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::Uninstall);
    let error = lower_managed_dlss_uninstall(
        &missing_claim,
        &plan_remove(&missing, vec![digest(b"installed")]),
        &path_ref(root.path()),
        &mut missing_live,
    )
    .expect_err("missing live file");
    assert!(matches!(error, ManagedDlssUninstallError::Lowering(_)));
}

#[test]
fn restore_rejects_wrong_sidecar_and_byte_identical_claims() {
    let root = tempfile::tempdir().expect("root");
    let live_path = root.path().join("nvngx_dlss.dll");
    std::fs::write(&live_path, b"same").expect("live");
    std::fs::write(root.path().join("nvngx_dlss.dll.bak"), b"different").expect("sidecar");
    let live = path_ref(&live_path);
    let baseline = digest(b"baseline");
    let persisted = owned(
        &live,
        ManagedFileBaseline::Present {
            sha256: baseline.clone(),
        },
        b"same",
    );
    let mut wrong_sidecar = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::Uninstall);
    let error = lower_managed_dlss_uninstall(
        &persisted,
        &plan_restore(&live, &baseline, vec![digest(b"same")]),
        &path_ref(root.path()),
        &mut wrong_sidecar,
    )
    .expect_err("wrong sidecar digest");
    assert!(matches!(
        error,
        ManagedDlssUninstallError::ExpectedDigestMismatch { .. }
    ));

    let equal = digest(b"same");
    let equal_claim = owned(
        &live,
        ManagedFileBaseline::Present {
            sha256: equal.clone(),
        },
        b"same",
    );
    let mut equal_accumulator = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::Uninstall);
    let error = lower_managed_dlss_uninstall(
        &equal_claim,
        &plan_restore(&live, &equal, vec![equal.clone()]),
        &path_ref(root.path()),
        &mut equal_accumulator,
    )
    .expect_err("byte-identical restore");
    assert!(matches!(
        error,
        ManagedDlssUninstallError::InstalledBaselineEqual(_)
    ));
    assert!(equal_accumulator.finalize().expect("finalize").is_none());
}

#[test]
fn release_rejects_sidecar_presence_or_absence_and_out_of_root_paths() {
    let root = tempfile::tempdir().expect("root");
    let live_path = root.path().join("nvngx_dlss.dll");
    std::fs::write(&live_path, b"managed").expect("live");
    let live = path_ref(&live_path);
    let persisted_absent = owned(&live, ManagedFileBaseline::Absent, b"managed");
    std::fs::write(root.path().join("nvngx_dlss.dll.bak"), b"orphan").expect("orphan sidecar");
    let mut occupied_sidecar = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::Uninstall);
    let error = lower_managed_dlss_uninstall(
        &persisted_absent,
        &plan_remove(&live, vec![digest(b"managed")]),
        &path_ref(root.path()),
        &mut occupied_sidecar,
    )
    .expect_err("absent baseline cannot have a sidecar");
    assert!(matches!(error, ManagedDlssUninstallError::Lowering(_)));

    std::fs::remove_file(root.path().join("nvngx_dlss.dll.bak")).expect("remove orphan");
    let baseline = digest(b"baseline");
    let persisted_present = owned(
        &live,
        ManagedFileBaseline::Present {
            sha256: baseline.clone(),
        },
        b"managed",
    );
    let mut missing_sidecar = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::Uninstall);
    let error = lower_managed_dlss_uninstall(
        &persisted_present,
        &plan_restore(&live, &baseline, vec![digest(b"managed")]),
        &path_ref(root.path()),
        &mut missing_sidecar,
    )
    .expect_err("present baseline requires a sidecar");
    assert!(matches!(error, ManagedDlssUninstallError::Lowering(_)));

    let outside = tempfile::tempdir().expect("outside");
    let outside_live_path = outside.path().join("nvngx_dlss.dll");
    std::fs::write(&outside_live_path, b"managed").expect("outside live");
    let outside_live = path_ref(&outside_live_path);
    let outside_claim = owned(&outside_live, ManagedFileBaseline::Absent, b"managed");
    let mut outside_accumulator = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::Uninstall);
    let error = lower_managed_dlss_uninstall(
        &outside_claim,
        &plan_remove(&outside_live, vec![digest(b"managed")]),
        &path_ref(root.path()),
        &mut outside_accumulator,
    )
    .expect_err("endpoint outside root");
    assert!(matches!(
        error,
        ManagedDlssUninstallError::Observation { .. }
    ));
}

#[test]
fn release_rejects_noncanonical_endpoint_paths() {
    let root = tempfile::tempdir().expect("root");
    let canonical = root.path().join("nvngx_dlss.dll");
    std::fs::write(&canonical, b"managed").expect("live");
    let noncanonical = root.path().join("nested").join("..").join("nvngx_dlss.dll");
    let persisted = owned(
        &path_ref(&noncanonical),
        ManagedFileBaseline::Absent,
        b"managed",
    );
    let mut accumulator = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::Uninstall);
    let error = lower_managed_dlss_uninstall(
        &persisted,
        &plan_remove(&path_ref(&noncanonical), vec![digest(b"managed")]),
        &path_ref(root.path()),
        &mut accumulator,
    )
    .expect_err("noncanonical endpoint");
    assert!(matches!(error, ManagedDlssUninstallError::InvalidPlan(_)));
}

#[test]
fn release_rejects_path_or_binding_mismatches() {
    let root = tempfile::tempdir().expect("root");
    let live_path = root.path().join("nvngx_dlss.dll");
    std::fs::write(&live_path, b"managed").expect("live");
    let live = path_ref(&live_path);
    let persisted = owned(&live, ManagedFileBaseline::Absent, b"managed");

    let other = path_ref(&root.path().join("other.dll"));
    let mut wrong_path = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::Uninstall);
    let error = lower_managed_dlss_uninstall(
        &persisted,
        &plan_remove(&other, vec![digest(b"managed")]),
        &path_ref(root.path()),
        &mut wrong_path,
    )
    .expect_err("path mismatch");
    assert!(matches!(
        error,
        ManagedDlssUninstallError::PathMismatch { .. }
    ));

    let mut binding = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::Uninstall);
    let mut bound_plan = plan_remove(&live, vec![digest(b"managed")]);
    bound_plan.binding = Some(persisted.clone());
    let error = lower_managed_dlss_uninstall(
        &persisted,
        &bound_plan,
        &path_ref(root.path()),
        &mut binding,
    )
    .expect_err("surviving binding");
    assert!(matches!(error, ManagedDlssUninstallError::InvalidPlan(_)));

    let mut unsupported = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::Uninstall);
    let unsupported_plan = PlannedDlss {
        action: CoordinatedFilePlan::Reuse {
            path: PathBuf::from(live.as_str()),
            sha256: digest(b"managed"),
        },
        binding: None,
    };
    let error = lower_managed_dlss_uninstall(
        &persisted,
        &unsupported_plan,
        &path_ref(root.path()),
        &mut unsupported,
    )
    .expect_err("non-release action");
    assert!(matches!(error, ManagedDlssUninstallError::InvalidPlan(_)));
}
