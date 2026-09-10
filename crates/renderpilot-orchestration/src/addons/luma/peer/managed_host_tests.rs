use std::path::Path;

use renderpilot_domain::{
    FileOwnership, FileReceipt, GameId, GameProxyTopology, ManagedAddonFile, ManagedFileBaseline,
    PathRef, ProxyImplementation, ProxyLink, ProxyRootPrestate, Sha256Hash, managed_sidecar_path,
};

use crate::addons::luma::peer::managed_host::{
    ManagedHostReleaseError, lower_managed_host_release,
};
use crate::peer_mutation_executor::{EndpointExpectation, EndpointPostcondition};

use super::effects::{LumaPeerEffectAccumulator, LumaPeerEffects, LumaPeerOperationOrder};

fn path_ref(path: &Path) -> PathRef {
    PathRef::new(path.to_string_lossy().into_owned()).expect("path")
}

fn digest(bytes: &[u8]) -> Sha256Hash {
    renderpilot_detection::sha256_bytes(bytes).expect("digest")
}

fn topology(root: &Path, host_bytes: &[u8], ownership: FileOwnership) -> GameProxyTopology {
    let root_slot = path_ref(&root.join("dxgi.dll"));
    let host = path_ref(&root.join("ReShade64.dll"));
    let receipt = match ownership {
        FileOwnership::Owned => FileReceipt::owned("reshade-id", digest(host_bytes)),
        FileOwnership::Reused => FileReceipt::reused("reshade-id", digest(host_bytes)),
    }
    .expect("receipt");
    GameProxyTopology {
        id: "optiscaler:managed-host-test".to_owned(),
        game_id: GameId::new("manual:managed-host-test").expect("game"),
        root_slot: root_slot.clone(),
        outer: ProxyLink {
            implementation: ProxyImplementation::OptiScaler,
            path: root_slot,
            receipt: FileReceipt::owned("outer-id", digest(b"outer")).expect("outer receipt"),
        },
        downstream: Some(ProxyLink {
            implementation: ProxyImplementation::ReShade,
            path: host,
            receipt,
        }),
        downstream_origin: Some(path_ref(&root.join("dxgi.dll"))),
        root_prestate: ProxyRootPrestate::Absent,
    }
}

fn owned_claim(
    path: &PathRef,
    baseline: ManagedFileBaseline,
    installed: &[u8],
) -> ManagedAddonFile {
    ManagedAddonFile::owned(path.clone(), baseline, digest(installed))
}

fn plan_accumulator() -> LumaPeerEffectAccumulator {
    LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::Uninstall)
}

fn finalize(accumulator: LumaPeerEffectAccumulator) -> LumaPeerEffects {
    accumulator
        .finalize()
        .expect("finalize")
        .expect("host effects")
}

#[test]
fn reused_host_is_a_noop_when_topology_receipt_is_reused() {
    let root = tempfile::tempdir().expect("root");
    let host = path_ref(&root.path().join("ReShade64.dll"));
    let topology = topology(root.path(), b"accepted", FileOwnership::Reused);
    let claim = ManagedAddonFile::reused(host, digest(b"accepted"));
    let mut accumulator = plan_accumulator();

    lower_managed_host_release(&topology, &claim, &path_ref(root.path()), &mut accumulator)
        .expect("reused no-op");
    assert!(accumulator.finalize().expect("finalize").is_none());
}

#[test]
fn owned_absent_host_release_emits_one_topology_remove() {
    let root = tempfile::tempdir().expect("root");
    let host_path = root.path().join("ReShade64.dll");
    std::fs::write(&host_path, b"managed").expect("host");
    let host = path_ref(&host_path);
    let topology = topology(root.path(), b"managed", FileOwnership::Owned);
    let claim = owned_claim(&host, ManagedFileBaseline::Absent, b"managed");
    let mut accumulator = plan_accumulator();

    lower_managed_host_release(&topology, &claim, &path_ref(root.path()), &mut accumulator)
        .expect("owned absent release");
    let effects = finalize(accumulator);
    let endpoint = &effects.program().endpoints()[0];
    assert_eq!(endpoint.path(), &host);
    assert_eq!(
        endpoint.role(),
        renderpilot_domain::PeerEndpointRole::TopologyDownstream
    );
    assert!(matches!(endpoint.before(), EndpointExpectation::File(_)));
    assert!(matches!(endpoint.after(), EndpointPostcondition::Absent));
    assert_eq!(effects.payloads(), &[None]);
}

#[test]
fn owned_present_host_release_restores_then_removes_canonical_sidecar() {
    let root = tempfile::tempdir().expect("root");
    let host_path = root.path().join("ReShade64.dll");
    std::fs::write(&host_path, b"managed").expect("host");
    std::fs::write(root.path().join("ReShade64.dll.bak"), b"baseline").expect("sidecar");
    let host = path_ref(&host_path);
    let sidecar = managed_sidecar_path(&host).expect("sidecar path");
    let baseline = digest(b"baseline");
    let topology = topology(root.path(), b"managed", FileOwnership::Owned);
    let claim = owned_claim(
        &host,
        ManagedFileBaseline::Present {
            sha256: baseline.clone(),
        },
        b"managed",
    );
    let mut accumulator = plan_accumulator();

    lower_managed_host_release(&topology, &claim, &path_ref(root.path()), &mut accumulator)
        .expect("owned present release");
    let effects = finalize(accumulator);
    let endpoints = effects.program().endpoints();
    assert_eq!(endpoints.len(), 2);
    assert_eq!(endpoints[0].path(), &host);
    assert_eq!(
        endpoints[0].role(),
        renderpilot_domain::PeerEndpointRole::TopologyDownstream
    );
    assert!(matches!(
        endpoints[0].after(),
        EndpointPostcondition::File(hash) if hash == &baseline
    ));
    assert_eq!(endpoints[1].path(), &sidecar);
    assert_eq!(
        endpoints[1].role(),
        renderpilot_domain::PeerEndpointRole::Disjoint
    );
    assert!(matches!(
        endpoints[1].after(),
        EndpointPostcondition::Absent
    ));
    assert_eq!(effects.payloads(), &[Some(b"baseline".to_vec()), None]);
}

#[test]
fn topology_and_claim_mismatches_fail_closed() {
    let root = tempfile::tempdir().expect("root");
    let host_path = root.path().join("ReShade64.dll");
    std::fs::write(&host_path, b"managed").expect("host");
    let host = path_ref(&host_path);
    let claim = owned_claim(&host, ManagedFileBaseline::Absent, b"managed");
    let root_ref = path_ref(root.path());

    let mut no_downstream = topology(root.path(), b"managed", FileOwnership::Owned);
    no_downstream.downstream = None;
    no_downstream.downstream_origin = None;
    let mut accumulator = plan_accumulator();
    let error = lower_managed_host_release(&no_downstream, &claim, &root_ref, &mut accumulator)
        .expect_err("missing downstream");
    assert!(matches!(error, ManagedHostReleaseError::InvalidTopology(_)));

    let mut wrong_outer = topology(root.path(), b"managed", FileOwnership::Owned);
    wrong_outer.outer.implementation = ProxyImplementation::SpecialK;
    let mut accumulator = plan_accumulator();
    let error = lower_managed_host_release(&wrong_outer, &claim, &root_ref, &mut accumulator)
        .expect_err("wrong outer");
    assert!(matches!(error, ManagedHostReleaseError::InvalidTopology(_)));

    let mut wrong_downstream = topology(root.path(), b"managed", FileOwnership::Owned);
    wrong_downstream
        .downstream
        .as_mut()
        .expect("downstream")
        .implementation = ProxyImplementation::Unknown;
    let mut accumulator = plan_accumulator();
    let error = lower_managed_host_release(&wrong_downstream, &claim, &root_ref, &mut accumulator)
        .expect_err("wrong downstream");
    assert!(matches!(error, ManagedHostReleaseError::InvalidTopology(_)));

    let mut wrong_root = topology(root.path(), b"managed", FileOwnership::Owned);
    wrong_root.outer.path = path_ref(&root.path().join("other.dll"));
    let mut accumulator = plan_accumulator();
    let error = lower_managed_host_release(&wrong_root, &claim, &root_ref, &mut accumulator)
        .expect_err("invalid topology");
    assert!(matches!(error, ManagedHostReleaseError::Topology(_)));

    let other = path_ref(&root.path().join("other-host.dll"));
    let mut accumulator = plan_accumulator();
    let error = lower_managed_host_release(
        &topology(root.path(), b"managed", FileOwnership::Owned),
        &owned_claim(&other, ManagedFileBaseline::Absent, b"managed"),
        &root_ref,
        &mut accumulator,
    )
    .expect_err("path mismatch");
    assert!(matches!(
        error,
        ManagedHostReleaseError::PathMismatch { .. }
    ));

    let wrong_digest_topology = topology(root.path(), b"different", FileOwnership::Owned);
    let mut accumulator = plan_accumulator();
    let error =
        lower_managed_host_release(&wrong_digest_topology, &claim, &root_ref, &mut accumulator)
            .expect_err("receipt digest mismatch");
    assert!(matches!(
        error,
        ManagedHostReleaseError::DigestMismatch { .. }
    ));

    let wrong_ownership_topology = topology(root.path(), b"managed", FileOwnership::Reused);
    let mut accumulator = plan_accumulator();
    let error = lower_managed_host_release(
        &wrong_ownership_topology,
        &claim,
        &root_ref,
        &mut accumulator,
    )
    .expect_err("receipt ownership mismatch");
    assert!(matches!(error, ManagedHostReleaseError::ClaimMismatch(_)));
}

#[test]
fn physical_failures_and_equal_restore_fail_closed() {
    let root = tempfile::tempdir().expect("root");
    let host_path = root.path().join("ReShade64.dll");
    let host = path_ref(&host_path);
    let root_ref = path_ref(root.path());
    let topology = topology(root.path(), b"managed", FileOwnership::Owned);

    let missing_claim = owned_claim(&host, ManagedFileBaseline::Absent, b"managed");
    let mut accumulator = plan_accumulator();
    let error = lower_managed_host_release(&topology, &missing_claim, &root_ref, &mut accumulator)
        .expect_err("missing live");
    assert!(matches!(error, ManagedHostReleaseError::Snapshot(_)));

    std::fs::write(&host_path, b"different").expect("live");
    let wrong_live_claim = owned_claim(&host, ManagedFileBaseline::Absent, b"managed");
    let mut accumulator = plan_accumulator();
    let error =
        lower_managed_host_release(&topology, &wrong_live_claim, &root_ref, &mut accumulator)
            .expect_err("wrong live");
    assert!(matches!(
        error,
        ManagedHostReleaseError::DigestMismatch { .. }
    ));

    std::fs::write(&host_path, b"managed").expect("live");
    std::fs::write(root.path().join("ReShade64.dll.bak"), b"wrong").expect("sidecar");
    let baseline = digest(b"baseline");
    let present_claim = owned_claim(
        &host,
        ManagedFileBaseline::Present { sha256: baseline },
        b"managed",
    );
    let mut accumulator = plan_accumulator();
    let error = lower_managed_host_release(&topology, &present_claim, &root_ref, &mut accumulator)
        .expect_err("wrong sidecar");
    assert!(matches!(
        error,
        ManagedHostReleaseError::DigestMismatch { .. }
    ));

    std::fs::remove_file(root.path().join("ReShade64.dll.bak")).expect("sidecar");
    let mut accumulator = plan_accumulator();
    let error = lower_managed_host_release(&topology, &present_claim, &root_ref, &mut accumulator)
        .expect_err("missing sidecar");
    assert!(matches!(error, ManagedHostReleaseError::Snapshot(_)));

    std::fs::write(root.path().join("ReShade64.dll.bak"), b"managed").expect("sidecar");
    let equal = digest(b"managed");
    let equal_claim = owned_claim(
        &host,
        ManagedFileBaseline::Present { sha256: equal },
        b"managed",
    );
    let mut accumulator = plan_accumulator();
    let error = lower_managed_host_release(&topology, &equal_claim, &root_ref, &mut accumulator)
        .expect_err("byte-identical restore");
    assert!(matches!(error, ManagedHostReleaseError::ClaimMismatch(_)));
    assert!(accumulator.finalize().expect("finalize").is_none());
}

#[test]
fn absent_baseline_rejects_an_existing_sidecar_and_out_of_root_observation() {
    let root = tempfile::tempdir().expect("root");
    let host_path = root.path().join("ReShade64.dll");
    std::fs::write(&host_path, b"managed").expect("host");
    std::fs::write(root.path().join("ReShade64.dll.bak"), b"orphan").expect("sidecar");
    let host = path_ref(&host_path);
    let base_topology = topology(root.path(), b"managed", FileOwnership::Owned);
    let claim = owned_claim(&host, ManagedFileBaseline::Absent, b"managed");
    let mut accumulator = plan_accumulator();
    let error = lower_managed_host_release(
        &base_topology,
        &claim,
        &path_ref(root.path()),
        &mut accumulator,
    )
    .expect_err("orphan sidecar");
    assert!(matches!(error, ManagedHostReleaseError::Snapshot(_)));

    let outside = tempfile::tempdir().expect("outside");
    let outside_path = outside.path().join("ReShade64.dll");
    std::fs::write(&outside_path, b"managed").expect("outside host");
    let outside_ref = path_ref(&outside_path);
    let outside_topology = GameProxyTopology {
        downstream: Some(ProxyLink {
            implementation: ProxyImplementation::ReShade,
            path: outside_ref.clone(),
            receipt: FileReceipt::owned("outside", digest(b"managed")).expect("receipt"),
        }),
        downstream_origin: Some(path_ref(&root.path().join("dxgi.dll"))),
        ..base_topology
    };
    let outside_claim = owned_claim(&outside_ref, ManagedFileBaseline::Absent, b"managed");
    let mut accumulator = plan_accumulator();
    let error = lower_managed_host_release(
        &outside_topology,
        &outside_claim,
        &path_ref(root.path()),
        &mut accumulator,
    )
    .expect_err("outside host");
    assert!(matches!(error, ManagedHostReleaseError::Observation { .. }));
}
