use super::*;
use crate::{
    ComponentFile, ComponentId, ComponentKind, ComponentRollbackBaseline, LibraryComponent,
    LibraryTechnology, Swappability,
};

fn component(id: &str, files: &[(&str, char)]) -> LibraryComponent {
    files.iter().fold(
        LibraryComponent::new(
            ComponentId::new(id).expect("component id"),
            game(),
            ComponentKind::NativeLibrary,
            LibraryTechnology::NvidiaStreamline,
            Swappability::Swappable,
        ),
        |component, (name, digest)| {
            component.with_file(ComponentFile::new(path(name)).with_sha256(hash(*digest)))
        },
    )
}

fn rollback_claim(
    before: &LibraryComponent,
    baseline: &[(&str, char)],
) -> PeerCatalogRollbackClaim {
    PeerCatalogRollbackClaim::new(
        vec![before.clone()],
        vec![PeerCatalogDeletedBaseline::new(
            before.id().clone(),
            ComponentRollbackBaseline::new(
                baseline
                    .iter()
                    .map(|(name, digest)| ComponentFile::new(path(name)).with_sha256(hash(*digest)))
                    .collect(),
            ),
        )],
    )
    .expect("rollback claim")
}

fn catalog_intent(
    name: &str,
    operation: PeerEndpointOperation,
    digest: Option<char>,
    length: Option<u64>,
) -> PeerEndpointIntent {
    PeerEndpointIntent::new(
        path(name),
        PeerEndpointRole::Disjoint,
        operation,
        digest.map(hash),
        length,
    )
    .expect("catalog intent")
}

#[test]
fn rollback_claim_derives_after_components_in_representative_order() {
    let before = component("component:streamline", &[("z.dll", 'z'), ("a.dll", 'a')]);
    let claim = rollback_claim(&before, &[("z.dll", 'b'), ("a.dll", 'c')]);
    assert_eq!(claim.before_components(), &[before]);
    assert_eq!(
        claim.after_components()[0].files()[0].path(),
        &path("a.dll")
    );
    assert_eq!(
        claim.after_components()[0].files()[1].path(),
        &path("z.dll")
    );
}

#[test]
fn rollback_claim_rejects_d3d12_and_missing_hashes() {
    let before = component("component:streamline", &[("x.dll", 'x')]);
    let d3d12 = ComponentRollbackBaseline::new(vec![]).with_d3d12_executable(
        crate::D3d12ExecutableBaseline::new(
            path("game.exe"),
            crate::D3d12ExecutableIdentity::new(1, hash('a')),
            crate::D3d12ExecutableIdentity::new(1, hash('a')),
        ),
    );
    assert!(matches!(
        PeerCatalogRollbackClaim::new(
            vec![before.clone()],
            vec![PeerCatalogDeletedBaseline::new(before.id().clone(), d3d12)],
        ),
        Err(PeerTransitionError::CatalogClaimInvalid(_))
    ));

    let missing = LibraryComponent::new(
        ComponentId::new("component:missing").expect("component id"),
        game(),
        ComponentKind::NativeLibrary,
        LibraryTechnology::NvidiaStreamline,
        Swappability::Swappable,
    )
    .with_file(ComponentFile::new(path("missing.dll")));
    let result = PeerCatalogRollbackClaim::new(
        vec![missing.clone()],
        vec![PeerCatalogDeletedBaseline::new(
            missing.id().clone(),
            ComponentRollbackBaseline::new(vec![
                ComponentFile::new(path("missing.dll")).with_sha256(hash('b')),
            ]),
        )],
    );
    assert!(matches!(
        result,
        Err(PeerTransitionError::CatalogClaimInvalid(_))
    ));
}

#[test]
fn catalog_current_only_remove_uses_selected_preimage_digest() {
    let before = component("component:remove", &[("current.dll", 'a')]);
    let claim = rollback_claim(&before, &[]);
    let intent = catalog_intent("current.dll", PeerEndpointOperation::Remove, None, None);
    let preimage = Some(image("current", 'a', 5));
    let contract = PeerCatalogPhysicalContract::derive(
        &claim,
        None,
        None,
        std::slice::from_ref(&intent),
        std::slice::from_ref(&preimage),
    )
    .expect("catalog remove");
    let transition = PeerTransitionContract::derive_physical_with_catalog(
        None,
        None,
        None,
        None,
        ProxyPeerRoute::DurableDisjoint,
        vec![intent],
        Some(&contract),
    )
    .expect("merged catalog remove");
    assert_eq!(transition.intents().len(), 1);
    assert_eq!(
        transition.intents()[0].operation(),
        PeerEndpointOperation::Remove
    );
}

#[test]
fn catalog_current_only_remove_cannot_mutate_retained_owned_path() {
    let before_component = component("component:owned-remove", &[("current.dll", 'a')]);
    let claim = rollback_claim(&before_component, &[]);
    let before_peer = peer(
        &[],
        &[],
        vec![managed(
            "current.dll",
            ManagedFileMode::Owned,
            ManagedFileBaseline::Absent,
            'a',
        )],
    );
    let after_peer = before_peer.clone();
    let intent = catalog_intent("current.dll", PeerEndpointOperation::Remove, None, None);
    let physical = vec![intent.clone()];
    let contract = PeerCatalogPhysicalContract::derive(
        &claim,
        Some(&before_peer),
        Some(&after_peer),
        &physical,
        &[Some(image("current", 'a', 5))],
    )
    .expect("catalog remove");
    assert!(matches!(
        PeerTransitionContract::derive_physical_with_catalog(
            Some(&before_peer),
            Some(&after_peer),
            None,
            None,
            ProxyPeerRoute::DurableDisjoint,
            physical,
            Some(&contract),
        ),
        Err(PeerTransitionError::CatalogBaselineSatisfactionMismatch(actual_path))
            if actual_path == path("current.dll")
    ));

    let after_peer = peer(&[], &[], Vec::new());
    let physical = vec![intent];
    let contract = PeerCatalogPhysicalContract::derive(
        &claim,
        Some(&before_peer),
        Some(&after_peer),
        &physical,
        &[Some(image("current", 'a', 5))],
    )
    .expect("catalog remove with peer release");
    PeerTransitionContract::derive_physical_with_catalog(
        Some(&before_peer),
        Some(&after_peer),
        None,
        None,
        ProxyPeerRoute::DurableDisjoint,
        physical,
        Some(&contract),
    )
    .expect("matching peer remove merges");
}

#[test]
fn catalog_restore_pair_and_owned_release_satisfaction_are_closed() {
    let before_component = component("component:restore", &[("host.dll", 'a')]);
    let claim = rollback_claim(&before_component, &[("host.dll", 'b')]);
    let live = catalog_intent(
        "host.dll",
        PeerEndpointOperation::Replace,
        Some('b'),
        Some(3),
    );
    let sidecar = catalog_intent("host.dll.bak", PeerEndpointOperation::Remove, None, None);
    let contract = PeerCatalogPhysicalContract::derive(
        &claim,
        None,
        None,
        &[live.clone(), sidecar.clone()],
        &[Some(image("live", 'a', 3)), Some(image("sidecar", 'b', 3))],
    )
    .expect("catalog restore pair");
    let transition = PeerTransitionContract::derive_physical_with_catalog(
        None,
        None,
        None,
        None,
        ProxyPeerRoute::DurableDisjoint,
        vec![live, sidecar],
        Some(&contract),
    )
    .expect("restore pair");
    assert_eq!(transition.intents().len(), 2);

    let before_peer = peer(
        &[],
        &[],
        vec![managed(
            "host.dll",
            ManagedFileMode::Owned,
            ManagedFileBaseline::Present { sha256: hash('b') },
            'b',
        )],
    );
    let addon_remove = remove("peer.addon64");
    let sidecar_only = catalog_intent("host.dll.bak", PeerEndpointOperation::Remove, None, None);
    let physical = vec![addon_remove.clone(), sidecar_only.clone()];
    let satisfied = PeerCatalogPhysicalContract::derive(
        &claim,
        Some(&before_peer),
        None,
        &physical,
        &[Some(image("addon", 'a', 3)), Some(image("sidecar", 'b', 3))],
    )
    .expect("sidecar satisfaction");
    let transition = PeerTransitionContract::derive_physical_with_catalog(
        Some(&before_peer),
        None,
        None,
        None,
        ProxyPeerRoute::DurableDisjoint,
        physical,
        Some(&satisfied),
    )
    .expect("satisfied release");
    assert_eq!(transition.intents(), &[addon_remove, sidecar_only]);
    let guards = required_read_guards_with_catalog(
        Some(&before_peer),
        None,
        None,
        None,
        ProxyPeerRoute::DurableDisjoint,
        transition.intents(),
        Some(&satisfied),
    )
    .expect("catalog guard");
    assert!(guards.iter().any(|guard| {
        guard.path() == &path("host.dll")
            && guard
                .sources()
                .contains(&PeerReadGuardSource::CatalogBaselineLive)
    }));
}

#[test]
fn catalog_requires_exact_o1_cardinality_and_baseline_sidecar() {
    let before = component("component:cardinality", &[("current.dll", 'a')]);
    let claim = rollback_claim(&before, &[("current.dll", 'b')]);
    let intent = catalog_intent(
        "current.dll",
        PeerEndpointOperation::Replace,
        Some('b'),
        Some(3),
    );
    assert!(matches!(
        PeerCatalogPhysicalContract::derive(&claim, None, None, &[intent], &[]),
        Err(PeerTransitionError::EvidenceCardinality { .. })
    ));
}

#[test]
fn catalog_claim_rejects_empty_delete_set_and_sidecar_collision() {
    let component = component("component:empty", &[("current.dll", 'a')]);
    assert!(matches!(
        PeerCatalogRollbackClaim::new(vec![component.clone()], Vec::new()),
        Err(PeerTransitionError::CatalogClaimInvalid(_))
    ));

    let collision = PeerCatalogRollbackClaim::new(
        vec![component.clone()],
        vec![PeerCatalogDeletedBaseline::new(
            component.id().clone(),
            ComponentRollbackBaseline::new(vec![
                ComponentFile::new(path("current.dll")).with_sha256(hash('b')),
                ComponentFile::new(path("current.dll.bak")).with_sha256(hash('c')),
            ]),
        )],
    );
    assert!(matches!(
        collision,
        Err(PeerTransitionError::CatalogClaimInvalid(_))
    ));
}

#[test]
fn catalog_satisfaction_allows_other_after_peer_membership() {
    let before_component = component("component:other", &[("host.dll", 'a')]);
    let claim = rollback_claim(&before_component, &[("host.dll", 'b')]);
    let before_peer = peer(
        &[],
        &[],
        vec![managed(
            "host.dll",
            ManagedFileMode::Owned,
            ManagedFileBaseline::Present { sha256: hash('b') },
            'b',
        )],
    );
    let after_peer = peer(&["other.dll"], &[], Vec::new());
    let other = create("other.dll", 'x', 2);
    let sidecar = catalog_intent("host.dll.bak", PeerEndpointOperation::Remove, None, None);
    let physical = vec![other, sidecar];
    let contract = PeerCatalogPhysicalContract::derive(
        &claim,
        Some(&before_peer),
        Some(&after_peer),
        &physical,
        &[None, Some(image("sidecar", 'b', 3))],
    )
    .expect("catalog satisfaction");
    let transition = PeerTransitionContract::derive_physical_with_catalog(
        Some(&before_peer),
        Some(&after_peer),
        None,
        None,
        ProxyPeerRoute::DurableDisjoint,
        physical,
        Some(&contract),
    )
    .expect("other peer membership remains legal");
    assert!(
        !transition
            .intents()
            .iter()
            .any(|intent| intent.path() == &path("host.dll"))
    );
}

#[test]
fn catalog_satisfaction_cannot_release_a_retained_owned_claim() {
    let before_component = component("component:retained-satisfaction", &[("host.dll", 'b')]);
    let claim = rollback_claim(&before_component, &[("host.dll", 'b')]);
    let before_peer = peer(
        &[],
        &[],
        vec![managed(
            "host.dll",
            ManagedFileMode::Owned,
            ManagedFileBaseline::Present { sha256: hash('b') },
            'b',
        )],
    );
    let after_peer = before_peer.clone();
    let sidecar = catalog_intent("host.dll.bak", PeerEndpointOperation::Remove, None, None);
    let physical = vec![sidecar];
    let contract = PeerCatalogPhysicalContract::derive(
        &claim,
        Some(&before_peer),
        Some(&after_peer),
        &physical,
        &[Some(image("sidecar", 'b', 3))],
    )
    .expect("catalog satisfaction projection");

    assert!(matches!(
        PeerTransitionContract::derive_physical_with_catalog(
            Some(&before_peer),
            Some(&after_peer),
            None,
            None,
            ProxyPeerRoute::DurableDisjoint,
            physical,
            Some(&contract),
        ),
        Err(PeerTransitionError::CatalogBaselineSatisfactionMismatch(actual_path))
            if actual_path == path("host.dll")
    ));
}

#[test]
fn catalog_satisfaction_coexists_with_a_coordinated_host_endpoint() {
    let before_component = component("component:coordinated", &[("catalog.dll", 'b')]);
    let claim = rollback_claim(&before_component, &[("catalog.dll", 'b')]);
    let before_peer = peer(
        &[],
        &[],
        vec![managed(
            "catalog.dll",
            ManagedFileMode::Owned,
            ManagedFileBaseline::Present { sha256: hash('b') },
            'b',
        )],
    );
    let after_peer = peer(
        &[],
        &[],
        vec![managed(
            "ReShade64.dll",
            ManagedFileMode::Owned,
            ManagedFileBaseline::Absent,
            'r',
        )],
    );
    let host = topology_intent(PeerEndpointOperation::Create, 'r', 3);
    let sidecar = catalog_intent("catalog.dll.bak", PeerEndpointOperation::Remove, None, None);
    let physical = vec![host, sidecar];
    let contract = PeerCatalogPhysicalContract::derive(
        &claim,
        Some(&before_peer),
        Some(&after_peer),
        &physical,
        &[None, Some(image("sidecar", 'b', 3))],
    )
    .expect("catalog satisfaction with host");
    let transition = PeerTransitionContract::derive_physical_with_catalog(
        Some(&before_peer),
        Some(&after_peer),
        Some(&outer_topology()),
        Some(&planned_observed('r', 3)),
        ProxyPeerRoute::Coordinated(CoordinatedPeerOperation::Create),
        physical,
        Some(&contract),
    )
    .expect("coordinated host and disjoint satisfaction");
    assert!(transition.intents().iter().any(|intent| {
        intent.role() == PeerEndpointRole::TopologyDownstream
            && intent.path() == &path("ReShade64.dll")
    }));
    assert!(
        !transition
            .intents()
            .iter()
            .any(|intent| intent.path() == &path("catalog.dll"))
    );
}

#[test]
fn catalog_sidecar_absence_fallback_emits_live_and_sidecar_guards() {
    let before_component = component("component:fallback", &[("restored.dll", 'b')]);
    let claim = rollback_claim(&before_component, &[("restored.dll", 'b')]);
    let after_peer = peer(&["other.dll"], &[], Vec::new());
    let addon = create("peer.addon64", 'a', 3);
    let other = create("other.dll", 'x', 2);
    let physical = vec![addon, other];
    let contract = PeerCatalogPhysicalContract::derive(
        &claim,
        None,
        Some(&after_peer),
        &physical,
        &[None, None],
    )
    .expect("sidecar-absent fallback");
    let guards = required_read_guards_with_catalog(
        None,
        Some(&after_peer),
        None,
        None,
        ProxyPeerRoute::DurableDisjoint,
        &physical,
        Some(&contract),
    )
    .expect("fallback guards");
    assert!(guards.iter().any(|guard| {
        guard.path() == &path("restored.dll")
            && guard.expectation() == &PeerReadGuardExpectation::Digest { sha256: hash('b') }
            && guard
                .sources()
                .contains(&PeerReadGuardSource::CatalogBaselineLive)
    }));
    assert!(guards.iter().any(|guard| {
        guard.path() == &path("restored.dll.bak")
            && guard.expectation() == &PeerReadGuardExpectation::Absent
            && guard
                .sources()
                .contains(&PeerReadGuardSource::CatalogBaselineSidecar)
    }));
}

#[test]
fn catalog_rejects_foreign_or_retained_managed_live_writes() {
    let before_component = component("component:foreign", &[("foreign.dll", 'a')]);
    let claim = rollback_claim(&before_component, &[("foreign.dll", 'b')]);
    let reused_peer = peer(
        &[],
        &[],
        vec![managed(
            "foreign.dll",
            ManagedFileMode::Reused,
            ManagedFileBaseline::Present { sha256: hash('a') },
            'a',
        )],
    );
    let live = catalog_intent(
        "foreign.dll",
        PeerEndpointOperation::Replace,
        Some('b'),
        Some(3),
    );
    let sidecar = catalog_intent("foreign.dll.bak", PeerEndpointOperation::Remove, None, None);
    assert!(matches!(
        PeerCatalogPhysicalContract::derive(
            &claim,
            Some(&reused_peer),
            None,
            &[live.clone(), sidecar.clone()],
            &[Some(image("live", 'a', 3)), Some(image("sidecar", 'b', 3))],
        ),
        Err(PeerTransitionError::CatalogPhysicalMismatch(_))
    ));

    let owned_peer = peer(
        &[],
        &[],
        vec![managed(
            "foreign.dll",
            ManagedFileMode::Owned,
            ManagedFileBaseline::Present { sha256: hash('a') },
            'a',
        )],
    );
    let retained_after = peer(
        &[],
        &[],
        vec![managed(
            "foreign.dll",
            ManagedFileMode::Owned,
            ManagedFileBaseline::Present { sha256: hash('a') },
            'a',
        )],
    );
    let physical = vec![live, sidecar];
    let catalog = PeerCatalogPhysicalContract::derive(
        &claim,
        Some(&owned_peer),
        Some(&retained_after),
        &physical,
        &[Some(image("live", 'a', 3)), Some(image("sidecar", 'b', 3))],
    )
    .expect("catalog physical projection");
    assert!(matches!(
        PeerTransitionContract::derive_physical_with_catalog(
            Some(&owned_peer),
            Some(&retained_after),
            None,
            None,
            ProxyPeerRoute::DurableDisjoint,
            physical,
            Some(&catalog),
        ),
        Err(PeerTransitionError::CatalogBaselineSatisfactionMismatch(_)
            | PeerTransitionError::PhysicalProgramMismatch(_))
    ));
}
