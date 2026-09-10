use sha2::{Digest, Sha256};
use std::path::Path;

use super::effects::{
    LumaPeerEffectAccumulator, LumaPeerEffectGroup, LumaPeerEffects, LumaPeerOperationOrder,
};
use crate::peer_mutation_executor::{EndpointExpectation, EndpointPostcondition, VerifiedPeerFile};
use renderpilot_domain::{PathRef, PeerEndpointRole, Sha256Hash};

const TEST_ROOT: &str = "C:/renderpilot-tests/luma-effects";

fn path(value: &str) -> PathRef {
    let native = if value.len() >= 2 && value.as_bytes()[1] == b':' {
        value.to_owned()
    } else {
        format!("{TEST_ROOT}/{value}")
    };
    PathRef::from_canonical_native_absolute(Path::new(&native)).expect("path")
}

fn digest(bytes: &[u8]) -> Sha256Hash {
    Sha256Hash::new(hex::encode(Sha256::digest(bytes))).expect("digest")
}

fn image(identity: &str, bytes: &[u8]) -> VerifiedPeerFile {
    VerifiedPeerFile::new_with_length(identity.to_owned(), digest(bytes), bytes.len() as u64)
        .expect("image")
}

fn finalize(accumulator: LumaPeerEffectAccumulator) -> LumaPeerEffects {
    accumulator
        .finalize()
        .expect("finalize")
        .expect("physical effects")
}

fn endpoint_paths(effects: &LumaPeerEffects) -> Vec<&str> {
    effects
        .program()
        .endpoints()
        .iter()
        .map(|endpoint| {
            endpoint
                .path()
                .as_str()
                .strip_prefix(TEST_ROOT)
                .and_then(|path| path.strip_prefix('/'))
                .unwrap_or_else(|| endpoint.path().as_str())
        })
        .collect()
}

#[test]
fn create_replace_remove_preserve_exact_images_and_payload_alignment() {
    let old = image("old", b"old");
    let mut accumulator = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);
    accumulator
        .create(
            LumaPeerEffectGroup::Generic,
            path("game/create.dll"),
            Vec::new(),
        )
        .expect("create");
    accumulator
        .replace(
            LumaPeerEffectGroup::DgVoodoo,
            path("game/replace.dll"),
            &old,
            b"replacement".to_vec(),
        )
        .expect("replace");
    accumulator
        .remove(
            LumaPeerEffectGroup::DlssCascade,
            path("game/remove.dll"),
            &old,
        )
        .expect("remove");

    let effects = finalize(accumulator);
    assert_eq!(
        endpoint_paths(&effects),
        ["game/create.dll", "game/replace.dll", "game/remove.dll"]
    );
    assert_eq!(effects.payloads()[0], Some(Vec::new()));
    assert_eq!(effects.payloads()[1], Some(b"replacement".to_vec()));
    assert_eq!(effects.payloads()[2], None);

    let endpoints = effects.program().endpoints();
    assert!(matches!(endpoints[0].before(), EndpointExpectation::Absent));
    assert!(matches!(
        endpoints[0].after(),
        EndpointPostcondition::File(hash) if hash == &digest(b"")
    ));
    assert!(matches!(
        endpoints[1].before(),
        EndpointExpectation::File(file) if file == &old
    ));
    assert!(matches!(
        endpoints[2].after(),
        EndpointPostcondition::Absent
    ));
    assert!(
        endpoints
            .iter()
            .all(|endpoint| endpoint.role() == PeerEndpointRole::Disjoint)
    );
}

#[test]
fn acquisition_validates_original_bytes_and_keeps_sidecar_before_live() {
    let original = b"foreign original";
    let before = image("foreign", original);
    let mut accumulator = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);
    accumulator
        .acquire_foreign(
            LumaPeerEffectGroup::Generic,
            path("game/host.dll"),
            path("game/host.dll.bak"),
            &before,
            original.to_vec(),
            b"managed".to_vec(),
        )
        .expect("acquisition");

    let effects = finalize(accumulator);
    assert_eq!(
        endpoint_paths(&effects),
        ["game/host.dll.bak", "game/host.dll"]
    );
    assert_eq!(
        effects.payloads(),
        &[Some(original.to_vec()), Some(b"managed".to_vec())]
    );
    let endpoints = effects.program().endpoints();
    assert!(matches!(endpoints[0].before(), EndpointExpectation::Absent));
    assert!(matches!(
        endpoints[0].after(),
        EndpointPostcondition::File(hash) if hash == &digest(original)
    ));
    assert!(matches!(
        endpoints[1].before(),
        EndpointExpectation::File(file) if file == &before
    ));
}

#[test]
fn acquisition_rejects_original_bytes_that_do_not_match_live_image() {
    let mut accumulator = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);
    let result = accumulator.acquire_foreign(
        LumaPeerEffectGroup::Generic,
        path("game/host.dll"),
        path("game/host.dll.bak"),
        &image("foreign", b"observed"),
        b"changed".to_vec(),
        b"managed".to_vec(),
    );
    assert!(result.is_err());
}

#[test]
fn host_acquisition_keeps_sidecar_before_topology_replace() {
    let original = b"foreign original";
    let before = image("foreign", original);
    let mut accumulator = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);
    accumulator
        .acquire_foreign(
            LumaPeerEffectGroup::Host,
            path("game/ReShade64.dll"),
            path("game/ReShade64.dll.bak"),
            &before,
            original.to_vec(),
            b"managed".to_vec(),
        )
        .expect("host acquisition");

    let effects = finalize(accumulator);
    assert_eq!(
        endpoint_paths(&effects),
        ["game/ReShade64.dll.bak", "game/ReShade64.dll"]
    );
    assert_eq!(
        effects.payloads(),
        &[Some(original.to_vec()), Some(b"managed".to_vec())]
    );
    let endpoints = effects.program().endpoints();
    assert_eq!(endpoints[0].role(), PeerEndpointRole::Disjoint);
    assert!(matches!(endpoints[0].before(), EndpointExpectation::Absent));
    assert!(matches!(
        endpoints[0].after(),
        EndpointPostcondition::File(hash) if hash == &digest(original)
    ));
    assert_eq!(endpoints[1].role(), PeerEndpointRole::TopologyDownstream);
    assert!(matches!(
        endpoints[1].before(),
        EndpointExpectation::File(file) if file == &before
    ));
    assert!(matches!(
        endpoints[1].after(),
        EndpointPostcondition::File(hash) if hash == &digest(b"managed")
    ));
}

#[test]
fn release_validates_baseline_and_keeps_live_before_sidecar() {
    let live_before = image("live", b"managed");
    let sidecar_before = image("sidecar", b"original");
    for (group, expected_live_role) in [
        (LumaPeerEffectGroup::Generic, PeerEndpointRole::Disjoint),
        (
            LumaPeerEffectGroup::Host,
            PeerEndpointRole::TopologyDownstream,
        ),
    ] {
        let mut accumulator = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::Uninstall);
        accumulator
            .release_present(
                group,
                path("game/host.dll"),
                path("game/host.dll.bak"),
                &live_before,
                &sidecar_before,
                b"original".to_vec(),
            )
            .expect("release");
        let effects = finalize(accumulator);
        assert_eq!(
            endpoint_paths(&effects),
            ["game/host.dll", "game/host.dll.bak"]
        );
        assert_eq!(effects.payloads(), &[Some(b"original".to_vec()), None]);
        assert_eq!(effects.program().endpoints()[0].role(), expected_live_role);
        assert_eq!(
            effects.program().endpoints()[1].role(),
            PeerEndpointRole::Disjoint
        );
    }
}

#[test]
fn release_rejects_baseline_bytes_that_do_not_match_sidecar_image() {
    let mut accumulator = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::Uninstall);
    let result = accumulator.release_present(
        LumaPeerEffectGroup::Generic,
        path("game/host.dll"),
        path("game/host.dll.bak"),
        &image("live", b"managed"),
        &image("sidecar", b"original"),
        b"tampered".to_vec(),
    );
    assert!(result.is_err());
}

#[test]
fn exact_duplicate_bundles_deduplicate_without_losing_payloads() {
    let before = image("live", b"old");
    let mut accumulator = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);
    for _ in 0..2 {
        accumulator
            .replace(
                LumaPeerEffectGroup::Generic,
                path("game/peer.dll"),
                &before,
                b"new".to_vec(),
            )
            .expect("duplicate replace");
    }
    let effects = finalize(accumulator);
    assert_eq!(effects.program().endpoints().len(), 1);
    assert_eq!(effects.payloads(), &[Some(b"new".to_vec())]);
}

#[test]
fn conflicting_partial_and_cross_group_overlaps_are_rejected() {
    let before = image("live", b"old");
    let mut conflicting = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);
    conflicting
        .replace(
            LumaPeerEffectGroup::Generic,
            path("game/peer.dll"),
            &before,
            b"new".to_vec(),
        )
        .expect("first replace");
    assert!(
        conflicting
            .replace(
                LumaPeerEffectGroup::Generic,
                path("game/peer.dll"),
                &before,
                b"different".to_vec(),
            )
            .is_err()
    );

    let mut partial = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);
    partial
        .create(
            LumaPeerEffectGroup::Generic,
            path("game/peer.dll"),
            b"one".to_vec(),
        )
        .expect("first create");
    assert!(
        partial
            .create(
                LumaPeerEffectGroup::DgVoodoo,
                path("game/peer.dll/child"),
                b"two".to_vec(),
            )
            .is_err()
    );

    let mut cross_group = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);
    cross_group
        .create(
            LumaPeerEffectGroup::Generic,
            path("game/shared.dll"),
            b"one".to_vec(),
        )
        .expect("first create");
    assert!(
        cross_group
            .create(
                LumaPeerEffectGroup::DgVoodoo,
                path("game/shared.dll"),
                b"one".to_vec(),
            )
            .is_err()
    );
}

#[test]
fn pair_paths_must_not_be_equal_or_partially_overlapping() {
    let before = image("live", b"original");
    let mut equal = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);
    assert!(
        equal
            .acquire_foreign(
                LumaPeerEffectGroup::Generic,
                path("game/peer.dll"),
                path("game/PEER.dll"),
                &before,
                b"original".to_vec(),
                b"new".to_vec(),
            )
            .is_err()
    );

    let mut partial = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::Uninstall);
    assert!(
        partial
            .release_present(
                LumaPeerEffectGroup::Generic,
                path("game/peer.dll"),
                path("game/peer.dll/child"),
                &image("live", b"new"),
                &before,
                b"original".to_vec(),
            )
            .is_err()
    );
}

#[test]
fn normalized_windows_slashes_and_case_still_reject_partial_overlap() {
    let mut accumulator = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);
    accumulator
        .create(
            LumaPeerEffectGroup::Generic,
            path(r"C:\Games\Peer.dll"),
            b"one".to_vec(),
        )
        .expect("first create");
    assert!(
        accumulator
            .create(
                LumaPeerEffectGroup::DgVoodoo,
                path("c:/games/peer.dll/child"),
                b"two".to_vec(),
            )
            .is_err()
    );
}

#[test]
fn component_boundaries_do_not_treat_name_prefixes_as_overlap() {
    let mut names = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);
    names
        .create(LumaPeerEffectGroup::Generic, path("foo"), b"one".to_vec())
        .expect("first create");
    names
        .create(
            LumaPeerEffectGroup::DgVoodoo,
            path("foobar"),
            b"two".to_vec(),
        )
        .expect("name prefix is not a component overlap");

    let mut components = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);
    components
        .create(LumaPeerEffectGroup::Generic, path("foo"), b"one".to_vec())
        .expect("first create");
    assert!(
        components
            .create(
                LumaPeerEffectGroup::DgVoodoo,
                path("foo/bar"),
                b"two".to_vec(),
            )
            .is_err()
    );
}

#[test]
fn host_group_has_one_topology_live_endpoint_and_other_groups_are_disjoint() {
    let mut host = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);
    host.create(
        LumaPeerEffectGroup::Host,
        path("game/host.dll"),
        b"host".to_vec(),
    )
    .expect("host create");
    host.create(
        LumaPeerEffectGroup::Host,
        path("game/second-host.dll"),
        b"second".to_vec(),
    )
    .expect("second host create");
    assert!(host.finalize().is_err());

    let mut generic = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);
    generic
        .create(
            LumaPeerEffectGroup::Generic,
            path("game/generic.dll"),
            b"generic".to_vec(),
        )
        .expect("generic create");
    let effects = finalize(generic);
    assert_eq!(
        effects.program().endpoints()[0].role(),
        PeerEndpointRole::Disjoint
    );
}

fn add_all_groups(accumulator: &mut LumaPeerEffectAccumulator, reverse: bool, uninstall: bool) {
    let entries = [
        (LumaPeerEffectGroup::Generic, "game/generic.dll"),
        (LumaPeerEffectGroup::DgVoodoo, "game/dgvoodoo.dll"),
        (LumaPeerEffectGroup::DlssCascade, "game/dlss.dll"),
        (LumaPeerEffectGroup::Host, "game/host.dll"),
    ];
    let entries = if reverse {
        entries.into_iter().rev().collect::<Vec<_>>()
    } else {
        entries.into_iter().collect::<Vec<_>>()
    };
    for (group, value) in entries {
        if uninstall {
            accumulator
                .remove(group, path(value), &image(value, b"old"))
                .expect("remove");
        } else {
            accumulator
                .create(group, path(value), value.as_bytes().to_vec())
                .expect("create");
        }
    }
}

#[test]
fn final_order_is_deterministic_for_both_operation_orders() {
    let mut install = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);
    add_all_groups(&mut install, true, false);
    let mut install_sorted =
        LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);
    add_all_groups(&mut install_sorted, false, false);
    let install_effects = finalize(install);
    let install_sorted_effects = finalize(install_sorted);
    assert_eq!(
        endpoint_paths(&install_effects),
        endpoint_paths(&install_sorted_effects)
    );
    assert_eq!(
        endpoint_paths(&install_sorted_effects),
        [
            "game/generic.dll",
            "game/dgvoodoo.dll",
            "game/dlss.dll",
            "game/host.dll"
        ]
    );

    let mut uninstall = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::Uninstall);
    add_all_groups(&mut uninstall, true, true);
    let effects = finalize(uninstall);
    assert_eq!(
        endpoint_paths(&effects),
        [
            "game/dlss.dll",
            "game/dgvoodoo.dll",
            "game/generic.dll",
            "game/host.dll"
        ]
    );
}

#[test]
fn empty_accumulator_has_no_effects_and_final_ordinals_are_unique() {
    let accumulator = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);
    assert!(accumulator.finalize().expect("empty finalize").is_none());

    let mut accumulator = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);
    accumulator
        .create(
            LumaPeerEffectGroup::Generic,
            path("GAME/peer.dll"),
            b"one".to_vec(),
        )
        .expect("create");
    accumulator
        .create(
            LumaPeerEffectGroup::DgVoodoo,
            path("game/other.dll"),
            b"two".to_vec(),
        )
        .expect("create");
    let effects = finalize(accumulator);
    let ordinals = effects
        .program()
        .endpoints()
        .iter()
        .enumerate()
        .map(|(ordinal, _)| ordinal)
        .collect::<Vec<_>>();
    assert_eq!(ordinals, [0, 1]);
}
