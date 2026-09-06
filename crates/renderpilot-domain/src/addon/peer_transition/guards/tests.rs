use super::*;
use crate::{
    AddonKind, FileReceipt, GameId, ManagedAddonFile, ProxyImplementation, ProxyLink,
    ProxyRootPrestate, Sha256Hash,
};

fn hash(byte: char) -> Sha256Hash {
    Sha256Hash::new(format!("{:x}", (byte as u8) % 16).repeat(64)).expect("hash")
}

fn path(value: &str) -> PathRef {
    PathRef::new(format!("C:/Games/Test/{value}")).expect("path")
}

fn game() -> GameId {
    GameId::new("manual:read-guards").expect("game")
}

fn peer(managed: Vec<ManagedAddonFile>) -> InstalledAddon {
    peer_with_created(&[], managed)
}

fn peer_with_created(created: &[&str], managed: Vec<ManagedAddonFile>) -> InstalledAddon {
    let mut peer = InstalledAddon::new(game(), AddonKind::RenoDx, path("peer.addon64"));
    for created in created {
        peer = peer.with_created_file(path(created));
    }
    peer.try_with_managed_files(managed).expect("peer")
}

fn outer() -> GameProxyTopology {
    let root = path("dxgi.dll");
    GameProxyTopology {
        id: "topology:read-guards".to_owned(),
        game_id: game(),
        root_slot: root.clone(),
        outer: ProxyLink {
            implementation: ProxyImplementation::OptiScaler,
            path: root,
            receipt: FileReceipt::owned("outer-id", hash('a')).expect("receipt"),
        },
        downstream: None,
        downstream_origin: None,
        root_prestate: ProxyRootPrestate::Absent,
    }
}

fn downstream(topology: &mut GameProxyTopology, ownership: crate::FileOwnership, digest: char) {
    let path = path("ReShade64.dll");
    topology.downstream = Some(ProxyLink {
        implementation: ProxyImplementation::ReShade,
        path,
        receipt: match ownership {
            crate::FileOwnership::Owned => FileReceipt::owned("downstream-id", hash(digest)),
            crate::FileOwnership::Reused => FileReceipt::reused("downstream-id", hash(digest)),
        }
        .expect("receipt"),
    });
    topology.downstream_origin = Some(topology.root_slot.clone());
    topology.root_prestate = ProxyRootPrestate::RelocatedDownstream;
}

#[test]
fn topology_outer_and_unchanged_downstream_are_receipt_guards() {
    let mut topology = outer();
    downstream(&mut topology, crate::FileOwnership::Owned, 'b');
    let requirements = required_read_guards(
        None,
        Some(&peer_with_created(&["payload.dll"], Vec::new())),
        Some(&topology),
        Some(&PlannedGameProxyTopology::Exact(topology.clone())),
        ProxyPeerRoute::DurableDisjoint,
        &[
            PeerEndpointIntent::create(
                path("peer.addon64"),
                PeerEndpointRole::Disjoint,
                Some(hash('a')),
                Some(3),
            )
            .expect("intent"),
            PeerEndpointIntent::create(
                path("payload.dll"),
                PeerEndpointRole::Disjoint,
                Some(hash('c')),
                Some(3),
            )
            .expect("intent"),
        ],
    )
    .expect("guards");
    assert_eq!(requirements.len(), 2);
    assert_eq!(
        requirements[0].sources(),
        &[PeerReadGuardSource::TopologyOuter]
    );
    assert_eq!(
        requirements[1].sources(),
        &[PeerReadGuardSource::TopologyDownstream]
    );
    assert!(matches!(
        requirements[0].expectation(),
        PeerReadGuardExpectation::Receipt { identity, sha256 }
            if identity == "outer-id" && sha256 == &hash('a')
    ));
}

#[test]
fn changed_downstream_is_an_endpoint_not_a_guard() {
    let topology = outer();
    let planned = PlannedGameProxyTopology::ObservedOwnedDownstream {
        id: topology.id.clone(),
        game_id: topology.game_id.clone(),
        root_slot: topology.root_slot.clone(),
        outer: topology.outer.clone(),
        implementation: ProxyImplementation::ReShade,
        downstream_path: path("ReShade64.dll"),
        downstream_origin: topology.root_slot.clone(),
        root_prestate: topology.root_prestate,
        planned_sha256: hash('b'),
        planned_length: 4,
    };
    let endpoint = PeerEndpointIntent::create(
        path("ReShade64.dll"),
        PeerEndpointRole::TopologyDownstream,
        Some(hash('b')),
        Some(4),
    )
    .expect("endpoint");
    let requirements = required_read_guards(
        None,
        Some(&peer(vec![ManagedAddonFile::owned(
            path("ReShade64.dll"),
            ManagedFileBaseline::Absent,
            hash('b'),
        )])),
        Some(&topology),
        Some(&planned),
        ProxyPeerRoute::Coordinated(CoordinatedPeerOperation::Create),
        &[
            PeerEndpointIntent::create(
                path("peer.addon64"),
                PeerEndpointRole::Disjoint,
                Some(hash('a')),
                Some(3),
            )
            .expect("intent"),
            endpoint,
        ],
    )
    .expect("guards");
    assert!(requirements.iter().all(|requirement| {
        normalized_path_key(requirement.path().as_str())
            != normalized_path_key("C:/Games/Test/ReShade64.dll")
    }));
}

#[test]
fn coordinated_replace_guards_outer_and_baseline_but_not_changed_downstream() {
    let mut topology = outer();
    downstream(&mut topology, crate::FileOwnership::Owned, 'a');
    let before = peer(vec![ManagedAddonFile::owned(
        path("ReShade64.dll"),
        ManagedFileBaseline::Absent,
        hash('a'),
    )]);
    let after = peer(vec![ManagedAddonFile::owned(
        path("ReShade64.dll"),
        ManagedFileBaseline::Absent,
        hash('b'),
    )]);
    let planned = PlannedGameProxyTopology::ObservedOwnedDownstream {
        id: topology.id.clone(),
        game_id: topology.game_id.clone(),
        root_slot: topology.root_slot.clone(),
        outer: topology.outer.clone(),
        implementation: ProxyImplementation::ReShade,
        downstream_path: path("ReShade64.dll"),
        downstream_origin: topology.root_slot.clone(),
        root_prestate: topology.root_prestate,
        planned_sha256: hash('b'),
        planned_length: 4,
    };
    let requirements = required_read_guards(
        Some(&before),
        Some(&after),
        Some(&topology),
        Some(&planned),
        ProxyPeerRoute::Coordinated(CoordinatedPeerOperation::ReplaceSamePath),
        &[PeerEndpointIntent::replace(
            path("ReShade64.dll"),
            PeerEndpointRole::TopologyDownstream,
            Some(hash('b')),
            Some(4),
        )
        .expect("endpoint")],
    )
    .expect("guards");
    assert_eq!(requirements.len(), 2);
    assert!(
        requirements.iter().all(|requirement| {
            requirement.sources() != [PeerReadGuardSource::TopologyDownstream]
        })
    );
    assert!(requirements.iter().any(|requirement| {
        requirement.path() == &path("ReShade64.dll.bak")
            && requirement.expectation() == &PeerReadGuardExpectation::Absent
    }));
}

#[test]
fn coordinated_remove_guards_outer_and_baseline_but_not_removed_downstream() {
    let mut before_topology = outer();
    downstream(&mut before_topology, crate::FileOwnership::Owned, 'a');
    let after_topology = outer();
    let before = peer(vec![ManagedAddonFile::owned(
        path("ReShade64.dll"),
        ManagedFileBaseline::Absent,
        hash('a'),
    )]);
    let requirements = required_read_guards(
        Some(&before),
        None,
        Some(&before_topology),
        Some(&PlannedGameProxyTopology::Exact(after_topology)),
        ProxyPeerRoute::Coordinated(CoordinatedPeerOperation::Remove),
        &[
            PeerEndpointIntent::remove(path("peer.addon64"), PeerEndpointRole::Disjoint)
                .expect("endpoint"),
            PeerEndpointIntent::remove(path("ReShade64.dll"), PeerEndpointRole::TopologyDownstream)
                .expect("endpoint"),
        ],
    )
    .expect("guards");
    assert_eq!(requirements.len(), 2);
    assert!(
        requirements
            .iter()
            .all(|requirement| { requirement.path() != &path("ReShade64.dll") })
    );
    assert!(requirements.iter().any(|requirement| {
        requirement.path() == &path("ReShade64.dll.bak")
            && requirement.expectation() == &PeerReadGuardExpectation::Absent
    }));
}

#[test]
fn owned_baselines_guard_absence_or_digest_until_their_sidecar_endpoint_changes() {
    let before = peer(vec![ManagedAddonFile::owned(
        path("managed.dll"),
        ManagedFileBaseline::Present { sha256: hash('p') },
        hash('i'),
    )]);
    let after = peer(vec![ManagedAddonFile::owned(
        path("managed.dll"),
        ManagedFileBaseline::Present { sha256: hash('p') },
        hash('n'),
    )]);
    let requirements = required_read_guards(
        Some(&before),
        Some(&after),
        None,
        None,
        ProxyPeerRoute::DurableDisjoint,
        &[PeerEndpointIntent::replace(
            path("managed.dll"),
            PeerEndpointRole::Disjoint,
            Some(hash('n')),
            Some(3),
        )
        .expect("intent")],
    )
    .expect("guards");
    assert!(matches!(
        requirements[0].expectation(),
        PeerReadGuardExpectation::Digest { sha256 } if sha256 == &hash('p')
    ));

    let absent = peer(vec![ManagedAddonFile::owned(
        path("absent.dll"),
        ManagedFileBaseline::Absent,
        hash('i'),
    )]);
    let absent_requirements = required_read_guards(
        None,
        Some(&absent),
        None,
        None,
        ProxyPeerRoute::DurableDisjoint,
        &[
            PeerEndpointIntent::create(
                path("peer.addon64"),
                PeerEndpointRole::Disjoint,
                Some(hash('a')),
                Some(3),
            )
            .expect("intent"),
            PeerEndpointIntent::create(
                path("absent.dll"),
                PeerEndpointRole::Disjoint,
                Some(hash('i')),
                Some(3),
            )
            .expect("intent"),
        ],
    )
    .expect("guards");
    assert!(matches!(
        absent_requirements[0].expectation(),
        PeerReadGuardExpectation::Absent
    ));

    let sidecar = managed_sidecar_path(&path("managed.dll")).expect("sidecar");
    let endpoint_requirements = required_read_guards(
        Some(&before),
        None,
        None,
        None,
        ProxyPeerRoute::DurableDisjoint,
        &[
            PeerEndpointIntent::remove(path("peer.addon64"), PeerEndpointRole::Disjoint)
                .expect("endpoint"),
            PeerEndpointIntent::replace(
                path("managed.dll"),
                PeerEndpointRole::Disjoint,
                Some(hash('p')),
                Some(3),
            )
            .expect("endpoint"),
            PeerEndpointIntent::remove(sidecar, PeerEndpointRole::Disjoint).expect("endpoint"),
        ],
    )
    .expect("guards");
    assert!(endpoint_requirements.is_empty());
}

#[test]
fn reused_membership_guards_are_union_deduplicated_for_add_same_and_release() {
    let reused = ManagedAddonFile::reused(path("shared.dll"), hash('r'));
    let before_empty = peer_with_created(&[], Vec::new());
    let after_added = peer_with_created(&["payload.dll"], vec![reused]);
    let addition = required_read_guards(
        Some(&before_empty),
        Some(&after_added),
        None,
        None,
        ProxyPeerRoute::DurableDisjoint,
        &[PeerEndpointIntent::create(
            path("payload.dll"),
            PeerEndpointRole::Disjoint,
            Some(hash('p')),
            Some(3),
        )
        .expect("payload")],
    )
    .expect("reused addition guard");
    assert_eq!(addition.len(), 1);
    assert_eq!(addition[0].path(), &path("shared.dll"));
    assert_eq!(
        addition[0].sources(),
        &[PeerReadGuardSource::ManagedReusedLive]
    );

    let before_same = after_added;
    let same = required_read_guards(
        Some(&before_same),
        Some(&before_same),
        None,
        None,
        ProxyPeerRoute::DurableDisjoint,
        &[PeerEndpointIntent::replace(
            path("payload.dll"),
            PeerEndpointRole::Disjoint,
            Some(hash('n')),
            Some(3),
        )
        .expect("payload")],
    )
    .expect("same reused guard");
    assert_eq!(same.len(), 1);
    assert_eq!(same[0].path(), &path("shared.dll"));

    let after_released = peer_with_created(&["payload.dll"], Vec::new());
    let release = required_read_guards(
        Some(&before_same),
        Some(&after_released),
        None,
        None,
        ProxyPeerRoute::DurableDisjoint,
        &[PeerEndpointIntent::replace(
            path("payload.dll"),
            PeerEndpointRole::Disjoint,
            Some(hash('n')),
            Some(3),
        )
        .expect("payload")],
    )
    .expect("reused release guard");
    assert_eq!(release.len(), 1);
    assert_eq!(release[0].path(), &path("shared.dll"));
}

#[test]
fn reused_acquisition_uses_endpoint_preflight_without_overlapping_guards() {
    let before = peer(vec![ManagedAddonFile::reused(
        path("shared.dll"),
        hash('r'),
    )]);
    let after = peer(vec![ManagedAddonFile::owned(
        path("shared.dll"),
        ManagedFileBaseline::Present { sha256: hash('r') },
        hash('i'),
    )]);
    let requirements = required_read_guards(
        Some(&before),
        Some(&after),
        None,
        None,
        ProxyPeerRoute::DurableDisjoint,
        &[
            PeerEndpointIntent::create(
                path("shared.dll.bak"),
                PeerEndpointRole::Disjoint,
                Some(hash('r')),
                Some(3),
            )
            .expect("sidecar"),
            PeerEndpointIntent::replace(
                path("shared.dll"),
                PeerEndpointRole::Disjoint,
                Some(hash('i')),
                Some(4),
            )
            .expect("live"),
        ],
    )
    .expect("acquisition guards");
    assert!(requirements.is_empty());
}

#[test]
fn reused_live_claim_cannot_be_an_endpoint() {
    let reused = peer(vec![ManagedAddonFile::reused(
        path("shared.dll"),
        hash('r'),
    )]);
    let error = required_read_guards(
        Some(&reused),
        None,
        None,
        None,
        ProxyPeerRoute::DurableDisjoint,
        &[
            PeerEndpointIntent::remove(path("shared.dll"), PeerEndpointRole::Disjoint)
                .expect("endpoint"),
        ],
    )
    .expect_err("reused live endpoint must be rejected");
    assert!(matches!(
        error,
        PeerTransitionError::ReadGuardEndpointOverlap(guard, endpoint)
            if guard == path("shared.dll") && endpoint == path("shared.dll")
    ));
}

#[test]
fn compatible_sources_merge_and_conflicts_are_rejected() {
    let topology = outer();
    let reused = peer_with_created(
        &["payload.dll"],
        vec![ManagedAddonFile::reused(path("dxgi.dll"), hash('a'))],
    );
    let merged = required_read_guards(
        Some(&reused),
        Some(&reused),
        Some(&topology),
        Some(&PlannedGameProxyTopology::Exact(topology.clone())),
        ProxyPeerRoute::DurableDisjoint,
        &[PeerEndpointIntent::replace(
            path("payload.dll"),
            PeerEndpointRole::Disjoint,
            Some(hash('c')),
            Some(3),
        )
        .expect("intent")],
    )
    .expect("merge");
    assert_eq!(
        merged[0].sources(),
        &[
            PeerReadGuardSource::TopologyOuter,
            PeerReadGuardSource::ManagedReusedLive
        ]
    );
    assert!(matches!(
        merged[0].expectation(),
        PeerReadGuardExpectation::Receipt { identity, sha256 }
            if identity == "outer-id" && sha256 == &hash('a')
    ));

    let conflict = peer_with_created(
        &["payload.dll"],
        vec![ManagedAddonFile::reused(path("dxgi.dll"), hash('b'))],
    );
    let error = required_read_guards(
        Some(&conflict),
        None,
        Some(&topology),
        Some(&PlannedGameProxyTopology::Exact(topology.clone())),
        ProxyPeerRoute::DurableDisjoint,
        &[PeerEndpointIntent::replace(
            path("payload.dll"),
            PeerEndpointRole::Disjoint,
            Some(hash('c')),
            Some(3),
        )
        .expect("intent")],
    )
    .expect_err("incompatible expectations");
    assert!(matches!(
        error,
        PeerTransitionError::ReadGuardExpectationConflict(guard_path)
            if guard_path == path("dxgi.dll")
    ));
}

#[test]
fn guard_output_is_sorted_and_rejects_ancestor_overlap() {
    let sorted_peer = peer(vec![
        ManagedAddonFile::owned(path("z.dll"), ManagedFileBaseline::Absent, hash('z')),
        ManagedAddonFile::owned(path("a.dll"), ManagedFileBaseline::Absent, hash('a')),
    ]);
    let requirements = required_read_guards(
        None,
        Some(&sorted_peer),
        None,
        None,
        ProxyPeerRoute::DurableDisjoint,
        &[
            PeerEndpointIntent::create(
                path("peer.addon64"),
                PeerEndpointRole::Disjoint,
                Some(hash('a')),
                Some(3),
            )
            .expect("intent"),
            PeerEndpointIntent::create(
                path("a.dll"),
                PeerEndpointRole::Disjoint,
                Some(hash('a')),
                Some(3),
            )
            .expect("intent"),
            PeerEndpointIntent::create(
                path("z.dll"),
                PeerEndpointRole::Disjoint,
                Some(hash('z')),
                Some(3),
            )
            .expect("intent"),
        ],
    )
    .expect("guards");
    assert_eq!(
        requirements[0].path(),
        &managed_sidecar_path(&path("a.dll")).unwrap()
    );
    assert_eq!(
        requirements[1].path(),
        &managed_sidecar_path(&path("z.dll")).unwrap()
    );

    let peer = peer(vec![ManagedAddonFile::owned(
        path("z.dll"),
        ManagedFileBaseline::Absent,
        hash('z'),
    )]);
    let error = required_read_guards(
        None,
        Some(&peer),
        None,
        None,
        ProxyPeerRoute::DurableDisjoint,
        &[PeerEndpointIntent::create(
            path("z.dll.bak/child"),
            PeerEndpointRole::Disjoint,
            Some(hash('a')),
            None,
        )
        .expect("intent")],
    )
    .expect_err("ancestor overlap");
    assert!(matches!(
        error,
        PeerTransitionError::ReadGuardEndpointOverlap(_, _)
    ));
}

#[test]
fn evidence_validation_is_typed_and_ordered() {
    let requirement = PeerReadGuardRequirement::new(
        path("guard.dll"),
        vec![PeerReadGuardSource::ManagedReusedLive],
        PeerReadGuardExpectation::Digest { sha256: hash('d') },
    );
    validate_read_guards(
        std::slice::from_ref(&requirement),
        &[PeerReadGuardEvidence::new(
            path("guard.dll"),
            Some(PeerFileImage::new("identity", hash('d'), 4).unwrap()),
        )],
    )
    .expect("matching evidence");
    let error = validate_read_guards(
        &[requirement],
        &[PeerReadGuardEvidence::new(path("guard.dll"), None)],
    )
    .expect_err("absent does not satisfy digest");
    assert!(matches!(
        error,
        PeerTransitionError::ReadGuardMismatch(guard_path) if guard_path == path("guard.dll")
    ));
}

#[test]
fn topology_source_predicate_classifies_all_sources_and_mixes() {
    let cases = [
        (PeerReadGuardSource::TopologyOuter, true),
        (PeerReadGuardSource::TopologyDownstream, true),
        (PeerReadGuardSource::ManagedOwnedBaseline, false),
        (PeerReadGuardSource::ManagedReusedLive, false),
        (PeerReadGuardSource::CatalogBaselineLive, false),
        (PeerReadGuardSource::CatalogBaselineSidecar, false),
    ];

    for (source, expected) in cases {
        let requirement = PeerReadGuardRequirement::new(
            path("guard.dll"),
            vec![source],
            PeerReadGuardExpectation::Absent,
        );
        assert_eq!(requirement.is_topology_sourced(), expected, "{source:?}");
    }

    let topology_and_managed = PeerReadGuardRequirement::new(
        path("guard.dll"),
        vec![
            PeerReadGuardSource::ManagedOwnedBaseline,
            PeerReadGuardSource::TopologyOuter,
        ],
        PeerReadGuardExpectation::Absent,
    );
    assert!(topology_and_managed.is_topology_sourced());

    let topology_and_catalog = PeerReadGuardRequirement::new(
        path("guard.dll"),
        vec![
            PeerReadGuardSource::CatalogBaselineLive,
            PeerReadGuardSource::TopologyDownstream,
        ],
        PeerReadGuardExpectation::Absent,
    );
    assert!(topology_and_catalog.is_topology_sourced());

    let managed_and_catalog = PeerReadGuardRequirement::new(
        path("guard.dll"),
        vec![
            PeerReadGuardSource::ManagedReusedLive,
            PeerReadGuardSource::CatalogBaselineSidecar,
        ],
        PeerReadGuardExpectation::Absent,
    );
    assert!(!managed_and_catalog.is_topology_sourced());
}
