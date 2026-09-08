use super::super::peer_binding::PeerTopologyDirection;
use super::*;

#[test]
fn optiscaler_transition_paths_are_changed_receipt_closure() {
    let before = transition_state();
    let topology = transition_topology();
    let first_install = transition_path_keys(optiscaler_transition_paths(
        None,
        Some(&before),
        None,
        Some(&topology),
        OptiScalerPeerMutation::Keep,
    ));
    assert_eq!(first_install.len(), 4);

    let unchanged = transition_path_keys(optiscaler_transition_paths(
        Some(&before),
        Some(&before),
        Some(&topology),
        Some(&topology),
        OptiScalerPeerMutation::Keep,
    ));
    assert!(unchanged.is_empty());

    let mut hash_repair = before.clone();
    hash_repair.release_files[0].installed = exact_owned(
        Sha256Hash::new("d".repeat(64)).expect("hash"),
        "config-repair",
    );
    assert_eq!(
        transition_path_keys(optiscaler_transition_paths(
            Some(&before),
            Some(&hash_repair),
            Some(&topology),
            Some(&topology),
            OptiScalerPeerMutation::Keep,
        )),
        [normalized_path_key("C:/Games/Test/OptiScaler.ini")]
            .into_iter()
            .collect()
    );

    let mut ownership_change = before.clone();
    ownership_change.runtime_bindings[0].installed = FileReceipt::reused(
        "test:runtime",
        ownership_change.runtime_bindings[0]
            .installed
            .digest()
            .clone(),
    )
    .expect("reused runtime receipt");
    ownership_change.runtime_bindings[0].baseline = OptiScalerFileBaseline::Present {
        receipt: ownership_change.runtime_bindings[0].installed.clone(),
    };
    assert_eq!(
        transition_path_keys(optiscaler_transition_paths(
            Some(&before),
            Some(&ownership_change),
            Some(&topology),
            Some(&topology),
            OptiScalerPeerMutation::Keep,
        )),
        [normalized_path_key("C:/Games/Test/core.dll")]
            .into_iter()
            .collect()
    );

    let mut topology_hash_change = topology.clone();
    topology_hash_change.outer.receipt = exact_owned(
        Sha256Hash::new("e".repeat(64)).expect("hash"),
        "outer-refresh",
    );
    assert_eq!(
        transition_path_keys(optiscaler_transition_paths(
            Some(&before),
            Some(&before),
            Some(&topology),
            Some(&topology_hash_change),
            OptiScalerPeerMutation::Keep,
        )),
        [normalized_path_key("C:/Games/Test/dxgi.dll")]
            .into_iter()
            .collect()
    );

    let uninstall = transition_path_keys(optiscaler_transition_paths(
        Some(&before),
        None,
        Some(&topology),
        None,
        OptiScalerPeerMutation::Keep,
    ));
    assert_eq!(uninstall, first_install);

    let peer_game = GameId::new("manual:peer-sidecar-closure").expect("game id");
    let peer_addon = PathRef::new("C:/Games/Test/peer.addon64").expect("addon path");
    let baseline_sha256 = Sha256Hash::new("f".repeat(64)).expect("baseline hash");
    let installed_sha256 = Sha256Hash::new("1".repeat(64)).expect("installed hash");
    let peer_before = InstalledAddon::new(peer_game.clone(), AddonKind::RenoDx, peer_addon.clone())
        .try_with_managed_files(vec![ManagedAddonFile::owned(
            PathRef::new("C:/Games/Test/dxgi.dll").expect("source host"),
            ManagedFileBaseline::Present {
                sha256: baseline_sha256.clone(),
            },
            installed_sha256.clone(),
        )])
        .expect("before peer");
    let peer_after = InstalledAddon::new(peer_game, AddonKind::RenoDx, peer_addon)
        .try_with_managed_files(vec![ManagedAddonFile::owned(
            PathRef::new("C:/Games/Test/ReShade64.dll").expect("destination host"),
            ManagedFileBaseline::Present {
                sha256: baseline_sha256,
            },
            installed_sha256,
        )])
        .expect("after peer");
    let expected = ExpectedPeerRelocation {
        source: PathRef::new("C:/Games/Test/dxgi.dll").expect("source host"),
        destination: PathRef::new("C:/Games/Test/ReShade64.dll").expect("destination host"),
        sha256: Sha256Hash::new("1".repeat(64)).expect("installed hash"),
        ownership: FileOwnership::Owned,
        direction: PeerTopologyDirection::IntoOptiTopology,
    };
    assert_eq!(
        transition_path_keys(optiscaler_transition_paths(
            Some(&before),
            Some(&before),
            Some(&topology),
            Some(&topology),
            OptiScalerPeerMutation::Replace {
                before: &peer_before,
                after: &peer_after,
            },
        )),
        [
            normalized_path_key("C:/Games/Test/dxgi.dll"),
            normalized_path_key("C:/Games/Test/dxgi.dll.bak"),
            normalized_path_key("C:/Games/Test/ReShade64.dll"),
            normalized_path_key("C:/Games/Test/ReShade64.dll.bak"),
        ]
        .into_iter()
        .collect()
    );
    assert!(ensure_exact_peer_relocation(&peer_before, &peer_after, &expected).is_ok());

    let metadata_change = peer_after.clone().with_addon_version("unexpected-update");
    assert!(
        ensure_exact_peer_relocation(&peer_before, &metadata_change, &expected).is_err(),
        "the coordinated relocation cannot carry unrelated peer metadata"
    );

    let extra_receipt = ManagedAddonFile::owned(
        PathRef::new("C:/Games/Test/unrelated.dll").expect("unrelated path"),
        ManagedFileBaseline::Absent,
        Sha256Hash::new("2".repeat(64)).expect("unrelated digest"),
    );
    let extra_receipt_change = peer_after
        .clone()
        .try_with_managed_files(vec![peer_after.managed_files()[0].clone(), extra_receipt])
        .expect("valid peer fixture");
    assert!(
        ensure_exact_peer_relocation(&peer_before, &extra_receipt_change, &expected).is_err(),
        "the coordinated relocation must reject additional receipt changes"
    );
}

#[test]
fn exact_before_peer_fence_rejects_a_stale_replace_receipt() {
    let storage = SqliteStorage::in_memory().expect("storage");
    let game_id = GameId::new("manual:stale-peer-preimage").expect("game id");
    let game = test_game(game_id.clone());
    storage.upsert_game(&game).expect("game");
    let addon_file =
        PathRef::new("C:/Games/manual_stale-peer-preimage/peer.addon64").expect("addon file");
    let before = InstalledAddon::new(game_id.clone(), AddonKind::RenoDx, addon_file);
    storage
        .upsert_installed_addon(&before)
        .expect("initial peer receipt");
    storage
        .upsert_installed_addon(&before.clone().with_addon_version("new"))
        .expect("changed peer receipt");

    let error = storage
        .with_transaction(|transaction| ensure_exact_before_peer(transaction, &game_id, &before))
        .expect_err("stale peer replacement must fail closed");
    assert!(
        error
            .to_string()
            .contains("OptiScaler peer receipt changed before aggregate commit")
    );
}

#[test]
fn expected_peer_relocation_is_derived_from_topology_and_rejects_origin_chaining() {
    let topology = topology_with_peer(
        "C:/Games/Test/dxgi.dll",
        "C:/Games/Test/ReShade64.dll",
        ProxyRootPrestate::RelocatedDownstream,
        'd',
    );
    let install = expected_peer_relocation(None, Some(&topology))
        .expect("install projection")
        .expect("install relocation");
    assert_eq!(install.source.as_str(), "C:/Games/Test/dxgi.dll");
    assert_eq!(install.destination.as_str(), "C:/Games/Test/ReShade64.dll");
    assert_eq!(install.sha256, Sha256Hash::new("d".repeat(64)).unwrap());

    assert!(
        expected_peer_relocation(Some(&topology), Some(&topology))
            .expect("unchanged update projection")
            .is_none()
    );
    let mut outer_refresh = topology.clone();
    outer_refresh.outer.receipt =
        exact_owned(Sha256Hash::new("e".repeat(64)).unwrap(), "outer-refresh");
    assert!(
        expected_peer_relocation(Some(&topology), Some(&outer_refresh))
            .expect("outer-only update projection")
            .is_none()
    );

    let uninstall = expected_peer_relocation(Some(&topology), None)
        .expect("uninstall projection")
        .expect("uninstall relocation");
    assert_eq!(uninstall.source, install.destination);
    assert_eq!(uninstall.destination, install.source);
    assert_eq!(uninstall.sha256, install.sha256);

    let before = topology_with_peer(
        "C:/Games/Test/dxgi.dll",
        "C:/Games/Test/ReShade64.dll",
        ProxyRootPrestate::RelocatedDownstream,
        'd',
    );
    let after = topology_with_peer(
        "C:/Games/Test/dxgi-new.dll",
        "C:/Games/Test/ReShade-new.dll",
        ProxyRootPrestate::Absent,
        'd',
    );
    let mut after = after;
    after.root_slot = PathRef::new("C:/Games/Test/dxgi-new.dll").expect("new root");
    after.outer.path = after.root_slot.clone();
    after.downstream_origin = Some(after.root_slot.clone());
    let relocation = expected_peer_relocation(Some(&before), Some(&after))
        .expect("full topology relocation")
        .expect("peer relocation");
    assert_eq!(relocation.source.as_str(), "C:/Games/Test/ReShade64.dll");
    assert_eq!(
        relocation.destination.as_str(),
        "C:/Games/Test/ReShade-new.dll"
    );

    let mut chained_origin = after;
    chained_origin.downstream_origin = Some(PathRef::new("C:/Games/Test/ReShade64.dll").unwrap());
    assert!(
        expected_peer_relocation(Some(&before), Some(&chained_origin)).is_err(),
        "a downstream path is never a topology root return target"
    );
}

#[test]
fn expected_peer_relocation_rejects_invalid_outer_peer_origin_and_digest() {
    let valid = topology_with_peer(
        "C:/Games/Test/dxgi.dll",
        "C:/Games/Test/ReShade64.dll",
        ProxyRootPrestate::RelocatedDownstream,
        'd',
    );
    let mut wrong_outer = valid.clone();
    wrong_outer.outer.implementation = ProxyImplementation::ReShade;
    assert!(expected_peer_relocation(None, Some(&wrong_outer)).is_err());

    let mut wrong_peer = valid.clone();
    wrong_peer.downstream.as_mut().unwrap().implementation = ProxyImplementation::Unknown;
    assert!(expected_peer_relocation(None, Some(&wrong_peer)).is_err());

    let mut absent_root_claim = valid.clone();
    absent_root_claim.root_prestate = ProxyRootPrestate::Absent;
    assert!(expected_peer_relocation(None, Some(&absent_root_claim)).is_ok());

    let mut invalid_absent_origin = absent_root_claim;
    invalid_absent_origin.downstream_origin =
        Some(PathRef::new("C:/Games/Test/previous-ReShade64.dll").expect("origin"));
    assert!(expected_peer_relocation(None, Some(&invalid_absent_origin)).is_err());

    let mut missing_origin = valid.clone();
    missing_origin.downstream_origin = None;
    assert!(expected_peer_relocation(None, Some(&missing_origin)).is_err());

    let mut relocated = valid;
    relocated.root_prestate = ProxyRootPrestate::Absent;
    let mut changed_digest = relocated.clone();
    changed_digest.downstream.as_mut().unwrap().receipt =
        exact_owned(Sha256Hash::new("e".repeat(64)).unwrap(), "changed-peer");
    assert!(expected_peer_relocation(Some(&relocated), Some(&changed_digest)).is_err());

    let mut changed_id = relocated.clone();
    changed_id.id.push_str("-other");
    assert!(expected_peer_relocation(Some(&relocated), Some(&changed_id)).is_err());
}

#[test]
fn state_topology_guard_rejects_a_non_optiscaler_outer() {
    let state = transition_state();
    let mut topology = transition_topology();
    topology.outer.implementation = ProxyImplementation::ReShade;
    assert!(ensure_state_topology(Some(&state), Some(&topology), "test").is_err());
}

#[test]
fn peer_mutation_must_consume_the_exact_topology_descriptor() {
    let game = GameId::new("manual:peer-exact").expect("game");
    let addon_file = PathRef::new("C:/Games/Test/peer.addon64").expect("addon");
    let installed = Sha256Hash::new("1".repeat(64)).unwrap();
    let baseline = Sha256Hash::new("f".repeat(64)).unwrap();
    let source = PathRef::new("C:/Games/Test/dxgi.dll").unwrap();
    let destination = PathRef::new("C:/Games/Test/ReShade64.dll").unwrap();
    let expected = ExpectedPeerRelocation {
        source: source.clone(),
        destination: destination.clone(),
        sha256: installed.clone(),
        ownership: FileOwnership::Owned,
        direction: PeerTopologyDirection::IntoOptiTopology,
    };
    let before = InstalledAddon::new(game.clone(), AddonKind::RenoDx, addon_file.clone())
        .try_with_managed_files(vec![ManagedAddonFile::owned(
            source.clone(),
            ManagedFileBaseline::Present {
                sha256: baseline.clone(),
            },
            installed.clone(),
        )])
        .unwrap();
    let after = InstalledAddon::new(game.clone(), AddonKind::RenoDx, addon_file.clone())
        .try_with_managed_files(vec![ManagedAddonFile::owned(
            destination.clone(),
            ManagedFileBaseline::Present { sha256: baseline },
            installed.clone(),
        )])
        .unwrap();
    assert!(ensure_exact_peer_relocation(&before, &after, &expected).is_ok());
    let reversed = ExpectedPeerRelocation {
        source: destination.clone(),
        destination: source,
        sha256: installed.clone(),
        ownership: FileOwnership::Owned,
        direction: PeerTopologyDirection::IntoOptiTopology,
    };
    assert!(ensure_exact_peer_relocation(&before, &after, &reversed).is_err());
    let wrong_digest = ExpectedPeerRelocation {
        sha256: Sha256Hash::new("2".repeat(64)).unwrap(),
        ..expected.clone()
    };
    assert!(ensure_exact_peer_relocation(&before, &after, &wrong_digest).is_err());

    let reused_after = InstalledAddon::new(game.clone(), AddonKind::RenoDx, addon_file.clone())
        .try_with_managed_files(vec![ManagedAddonFile::reused(
            destination.clone(),
            installed.clone(),
        )])
        .unwrap();
    assert!(ensure_exact_peer_relocation(&before, &reused_after, &expected).is_err());

    let absent_baseline_after = InstalledAddon::new(game, AddonKind::RenoDx, addon_file)
        .try_with_managed_files(vec![ManagedAddonFile::owned(
            destination,
            ManagedFileBaseline::Absent,
            installed,
        )])
        .unwrap();
    assert!(ensure_exact_peer_relocation(&before, &absent_baseline_after, &expected).is_err());

    assert!(
        validate_peer_receipt_transition(
            None,
            OptiScalerPeerMutation::Replace {
                before: &before,
                after: &after,
            },
            false,
        )
        .is_err()
    );
    assert!(
        validate_peer_receipt_transition(Some(&expected), OptiScalerPeerMutation::Keep, false)
            .is_ok()
    );
    assert!(
        validate_peer_receipt_transition(Some(&expected), OptiScalerPeerMutation::Keep, true)
            .is_err()
    );
}

#[test]
fn generic_created_host_is_adopted_as_owned_managed_downstream() {
    let game = GameId::new("manual:generic-host-adoption").expect("game");
    let addon_file = PathRef::new("C:/Games/Test/peer.addon64").expect("addon");
    let source = PathRef::new("C:/Games/Test/dxgi.dll").expect("source");
    let destination = PathRef::new("C:/Games/Test/ReShade64.dll").expect("destination");
    let installed = Sha256Hash::new("1".repeat(64)).expect("host digest");
    let before = InstalledAddon::new(game.clone(), AddonKind::RenoDx, addon_file.clone())
        .with_created_file(source.clone());
    let after = InstalledAddon::new(game, AddonKind::RenoDx, addon_file)
        .try_with_managed_files(vec![ManagedAddonFile::owned(
            destination.clone(),
            ManagedFileBaseline::Absent,
            installed.clone(),
        )])
        .expect("canonical peer");
    let expected = ExpectedPeerRelocation {
        source,
        destination,
        sha256: installed,
        ownership: FileOwnership::Owned,
        direction: PeerTopologyDirection::IntoOptiTopology,
    };

    assert!(ensure_exact_peer_relocation(&before, &after, &expected).is_ok());

    let reused_topology = ExpectedPeerRelocation {
        ownership: FileOwnership::Reused,
        ..expected
    };
    assert!(ensure_exact_peer_relocation(&before, &after, &reused_topology).is_err());
}

#[test]
fn generic_host_conversion_rejects_wrong_identity_and_claim_shape() {
    let game = GameId::new("manual:generic-host-adoption-negative").expect("game");
    let addon_file = PathRef::new("C:/Games/Test/peer.addon64").expect("addon");
    let source = PathRef::new("C:/Games/Test/dxgi.dll").expect("source");
    let destination = PathRef::new("C:/Games/Test/ReShade64.dll").expect("destination");
    let installed = Sha256Hash::new("1".repeat(64)).expect("host digest");
    let expected = ExpectedPeerRelocation {
        source: source.clone(),
        destination: destination.clone(),
        sha256: installed.clone(),
        ownership: FileOwnership::Owned,
        direction: PeerTopologyDirection::IntoOptiTopology,
    };
    let before = InstalledAddon::new(game.clone(), AddonKind::RenoDx, addon_file.clone())
        .with_created_file(source.clone());
    let after = InstalledAddon::new(game.clone(), AddonKind::RenoDx, addon_file.clone())
        .try_with_managed_files(vec![ManagedAddonFile::owned(
            destination.clone(),
            ManagedFileBaseline::Absent,
            installed.clone(),
        )])
        .expect("canonical peer");

    let wrong_kind = InstalledAddon::new(game.clone(), AddonKind::OptiScaler, addon_file.clone())
        .try_with_managed_files(vec![ManagedAddonFile::owned(
            destination.clone(),
            ManagedFileBaseline::Absent,
            installed.clone(),
        )])
        .expect("wrong-kind peer fixture");
    assert!(ensure_exact_peer_relocation(&before, &wrong_kind, &expected).is_err());

    let wrong_addon_identity = InstalledAddon::new(
        game.clone(),
        AddonKind::RenoDx,
        PathRef::new("C:/Games/Test/other-peer.addon64").expect("other addon"),
    )
    .try_with_managed_files(vec![ManagedAddonFile::owned(
        destination.clone(),
        ManagedFileBaseline::Absent,
        installed.clone(),
    )])
    .expect("wrong-identity peer fixture");
    assert!(ensure_exact_peer_relocation(&before, &wrong_addon_identity, &expected).is_err());

    assert!(
        ensure_exact_peer_relocation(
            &before.clone().with_backed_up_file(source),
            &after,
            &expected,
        )
        .is_err()
    );

    assert!(
        ensure_exact_peer_relocation(
            &before.clone().with_created_file(destination.clone()),
            &after,
            &expected,
        )
        .is_err()
    );

    let managed_destination_before = before
        .clone()
        .try_with_managed_files(vec![ManagedAddonFile::owned(
            destination.clone(),
            ManagedFileBaseline::Absent,
            installed.clone(),
        )])
        .expect("managed destination occupancy");
    assert!(ensure_exact_peer_relocation(&managed_destination_before, &after, &expected,).is_err());

    let wrong_digest = ExpectedPeerRelocation {
        sha256: Sha256Hash::new("2".repeat(64)).expect("wrong digest"),
        ..expected.clone()
    };
    assert!(ensure_exact_peer_relocation(&before, &after, &wrong_digest).is_err());

    let extra_created = PathRef::new("C:/Games/Test/unrelated.dll").expect("extra created");
    assert!(
        ensure_exact_peer_relocation(
            &before.clone().with_created_file(extra_created),
            &after,
            &expected,
        )
        .is_err()
    );

    let unrelated = PathRef::new("C:/Games/Test/unrelated-managed.dll").expect("managed path");
    let before_with_unrelated = before
        .try_with_managed_files(vec![ManagedAddonFile::owned(
            unrelated.clone(),
            ManagedFileBaseline::Absent,
            Sha256Hash::new("3".repeat(64)).expect("unrelated digest"),
        )])
        .expect("unrelated before");
    let after_with_unrelated = InstalledAddon::new(game, AddonKind::RenoDx, addon_file)
        .try_with_managed_files(vec![
            ManagedAddonFile::owned(
                unrelated,
                ManagedFileBaseline::Absent,
                Sha256Hash::new("4".repeat(64)).expect("changed unrelated digest"),
            ),
            ManagedAddonFile::owned(destination, ManagedFileBaseline::Absent, installed),
        ])
        .expect("unrelated after");
    assert!(
        ensure_exact_peer_relocation(&before_with_unrelated, &after_with_unrelated, &expected,)
            .is_err()
    );
}

#[test]
fn luma_out_managed_to_native_projection_and_binding_cover_all_three_custodies() {
    fn case(
        mode: ManagedFileMode,
        baseline: ManagedFileBaseline,
        append_backed: bool,
    ) -> (
        GameProxyTopology,
        InstalledAddon,
        InstalledAddon,
        ExpectedPeerRelocation,
        PathRef,
        PathRef,
    ) {
        let game = GameId::new("manual:luma-out-projection").expect("game");
        let addon = PathRef::new("C:/Games/Test/luma.addon64").expect("addon");
        let source = PathRef::new("C:/Games/Test/ReShade64.dll").expect("source");
        let destination = PathRef::new("C:/Games/Test/dxgi.dll").expect("destination");
        let digest = Sha256Hash::new("d".repeat(64)).expect("digest");
        let before = InstalledAddon::new(game.clone(), AddonKind::Luma, addon.clone())
            .try_with_managed_files(vec![match mode {
                ManagedFileMode::Owned => {
                    ManagedAddonFile::owned(source.clone(), baseline, digest.clone())
                }
                ManagedFileMode::Reused => ManagedAddonFile::reused(source.clone(), digest.clone()),
            }])
            .expect("before Luma");
        let mut after = InstalledAddon::new(game, AddonKind::Luma, addon);
        if mode == ManagedFileMode::Owned {
            after = after.with_created_file(destination.clone());
            if append_backed {
                after = after.with_backed_up_file(destination.clone());
            }
        }
        let mut topology = topology_with_peer(
            destination.as_str(),
            source.as_str(),
            ProxyRootPrestate::RelocatedDownstream,
            'd',
        );
        let ownership = match mode {
            ManagedFileMode::Owned => FileOwnership::Owned,
            ManagedFileMode::Reused => {
                topology.downstream.as_mut().expect("downstream").receipt =
                    FileReceipt::reused("test:luma-downstream", digest.clone())
                        .expect("reused downstream");
                FileOwnership::Reused
            }
        };
        let expected = ExpectedPeerRelocation {
            source,
            destination,
            sha256: digest,
            ownership,
            direction: PeerTopologyDirection::OutOfOptiTopology,
        };
        let source = expected.source.clone();
        let destination = expected.destination.clone();
        (topology, before, after, expected, source, destination)
    }

    for (mode, baseline, append_backed) in [
        (ManagedFileMode::Owned, ManagedFileBaseline::Absent, false),
        (
            ManagedFileMode::Owned,
            ManagedFileBaseline::Present {
                sha256: Sha256Hash::new("f".repeat(64)).expect("baseline"),
            },
            true,
        ),
        (
            ManagedFileMode::Reused,
            ManagedFileBaseline::Present {
                sha256: Sha256Hash::new("d".repeat(64)).expect("baseline"),
            },
            false,
        ),
    ] {
        let (topology, before, after, expected, source, destination) =
            case(mode, baseline, append_backed);
        assert!(ensure_exact_peer_relocation(&before, &after, &expected).is_ok());
        let mut after_topology = topology.clone();
        after_topology.downstream = None;
        after_topology.downstream_origin = None;
        after_topology.root_prestate = ProxyRootPrestate::Absent;
        let derived = expected_peer_relocation(Some(&topology), Some(&after_topology))
            .expect("Some-to-Some removal projection")
            .expect("Some-to-Some removal relocation");
        assert_eq!(derived.direction, PeerTopologyDirection::OutOfOptiTopology);
        let binding = build_optiscaler_binding(
            None,
            None,
            Some(&topology),
            Some(&after_topology),
            PeerAggregateBinding {
                mutation: OptiScalerPeerMutation::Replace {
                    before: &before,
                    after: &after,
                },
                peer_relocation: Some(&derived),
            },
            &[],
            &[],
        )
        .expect("Luma reverse aggregate binding");
        let source_key = normalized_path_key(&format!("{}.bak", source.as_str()));
        let destination_key = normalized_path_key(&format!("{}.bak", destination.as_str()));
        if append_backed {
            assert!(binding.paths.contains_key(&source_key));
            assert!(binding.paths.contains_key(&destination_key));
            assert!(matches!(
                binding.paths[&source_key].transition,
                pending_file_mutations::OptiScalerBoundTransition::RelocatePeerBaseline { .. }
            ));
        } else {
            assert!(!binding.paths.contains_key(&source_key));
            assert!(!binding.paths.contains_key(&destination_key));
        }
    }
}

#[test]
fn luma_out_projection_rejects_wrong_kind_direction_and_claim_shape() {
    let game = GameId::new("manual:luma-out-negative").expect("game");
    let addon = PathRef::new("C:/Games/Test/luma.addon64").expect("addon");
    let source = PathRef::new("C:/Games/Test/ReShade64.dll").expect("source");
    let destination = PathRef::new("C:/Games/Test/dxgi.dll").expect("destination");
    let digest = Sha256Hash::new("d".repeat(64)).expect("digest");
    let expected = ExpectedPeerRelocation {
        source: source.clone(),
        destination: destination.clone(),
        sha256: digest.clone(),
        ownership: FileOwnership::Owned,
        direction: PeerTopologyDirection::OutOfOptiTopology,
    };
    let before = InstalledAddon::new(game.clone(), AddonKind::Luma, addon.clone())
        .try_with_managed_files(vec![ManagedAddonFile::owned(
            source.clone(),
            ManagedFileBaseline::Absent,
            digest.clone(),
        )])
        .expect("before Luma");
    let after = InstalledAddon::new(game.clone(), AddonKind::Luma, addon.clone())
        .with_created_file(destination.clone());
    assert!(ensure_exact_peer_relocation(&before, &after, &expected).is_ok());
    let wrong_direction = ExpectedPeerRelocation {
        direction: PeerTopologyDirection::IntoOptiTopology,
        ..expected.clone()
    };
    assert!(ensure_exact_peer_relocation(&before, &after, &wrong_direction).is_err());
    let reused_before = InstalledAddon::new(game.clone(), AddonKind::Luma, addon.clone())
        .try_with_managed_files(vec![ManagedAddonFile::reused(source.clone(), digest)])
        .expect("reused before Luma");
    let reused_after = InstalledAddon::new(game.clone(), AddonKind::Luma, addon.clone());
    let reused_expected = ExpectedPeerRelocation {
        ownership: FileOwnership::Reused,
        ..expected.clone()
    };
    assert!(ensure_exact_peer_relocation(&reused_before, &reused_after, &reused_expected).is_ok());
    assert!(
        ensure_exact_peer_relocation(
            &reused_before,
            &reused_after,
            &ExpectedPeerRelocation {
                direction: PeerTopologyDirection::IntoOptiTopology,
                ..reused_expected.clone()
            },
        )
        .is_err()
    );
    let executable_overlap_before = before.clone().with_registered_exe_path(source.clone());
    let executable_overlap_after = after.clone().with_registered_exe_path(source.clone());
    assert!(
        ensure_exact_peer_relocation(
            &executable_overlap_before,
            &executable_overlap_after,
            &expected,
        )
        .is_err()
    );

    let mut topology = topology_with_peer(
        destination.as_str(),
        source.as_str(),
        ProxyRootPrestate::RelocatedDownstream,
        'd',
    );
    let into_topology = topology.clone();
    assert!(
        build_optiscaler_binding(
            None,
            None,
            Some(&into_topology),
            Some(&into_topology),
            PeerAggregateBinding {
                mutation: OptiScalerPeerMutation::Replace {
                    before: &before,
                    after: &after,
                },
                peer_relocation: Some(&expected),
            },
            &[],
            &[],
        )
        .is_err()
    );
    assert!(
        build_optiscaler_binding(
            None,
            None,
            Some(&into_topology),
            Some(&into_topology),
            PeerAggregateBinding {
                mutation: OptiScalerPeerMutation::Replace {
                    before: &reused_before,
                    after: &reused_after,
                },
                peer_relocation: Some(&ExpectedPeerRelocation {
                    direction: PeerTopologyDirection::IntoOptiTopology,
                    ..reused_expected
                }),
            },
            &[],
            &[],
        )
        .is_err()
    );

    let wrong_kind = InstalledAddon::new(game.clone(), AddonKind::RenoDx, addon.clone())
        .with_created_file(destination);
    assert!(ensure_exact_peer_relocation(&before, &wrong_kind, &expected).is_err());

    let generic_before =
        InstalledAddon::new(game, AddonKind::Luma, addon).with_created_file(source);
    topology.outer.receipt = exact_owned(
        Sha256Hash::new("c".repeat(64)).expect("outer digest"),
        "outer",
    );
    assert!(
        build_optiscaler_binding(
            None,
            None,
            Some(&topology),
            None,
            PeerAggregateBinding {
                mutation: OptiScalerPeerMutation::Replace {
                    before: &generic_before,
                    after: &after,
                },
                peer_relocation: Some(&expected),
            },
            &[],
            &[],
        )
        .is_err()
    );
}

#[test]
fn managed_peer_relocation_emits_canonical_relocate_binding() {
    let game = GameId::new("manual:managed-peer-binding").expect("game");
    let addon_file = PathRef::new("C:/Games/Test/peer.addon64").expect("addon");
    let baseline = Sha256Hash::new("f".repeat(64)).expect("baseline");
    let installed = Sha256Hash::new("d".repeat(64)).expect("installed");
    let source = PathRef::new("C:/Games/Test/managed-old.dll").expect("source");
    let destination = PathRef::new("C:/Games/Test/managed-new.dll").expect("destination");
    let before = InstalledAddon::new(game.clone(), AddonKind::RenoDx, addon_file.clone())
        .try_with_managed_files(vec![ManagedAddonFile::owned(
            source.clone(),
            ManagedFileBaseline::Present {
                sha256: baseline.clone(),
            },
            installed.clone(),
        )])
        .expect("before peer");
    let after = InstalledAddon::new(game, AddonKind::RenoDx, addon_file)
        .try_with_managed_files(vec![ManagedAddonFile::owned(
            destination.clone(),
            ManagedFileBaseline::Present { sha256: baseline },
            installed.clone(),
        )])
        .expect("after peer");
    let expected = ExpectedPeerRelocation {
        source: source.clone(),
        destination: destination.clone(),
        sha256: installed,
        ownership: FileOwnership::Owned,
        direction: PeerTopologyDirection::IntoOptiTopology,
    };
    let before_topology = topology_with_peer(
        "C:/Games/Test/dxgi.dll",
        source.as_str(),
        ProxyRootPrestate::RelocatedDownstream,
        'd',
    );
    let after_topology = topology_with_peer(
        "C:/Games/Test/dxgi.dll",
        destination.as_str(),
        ProxyRootPrestate::RelocatedDownstream,
        'd',
    );
    let binding = build_optiscaler_binding(
        None,
        None,
        Some(&before_topology),
        Some(&after_topology),
        PeerAggregateBinding {
            mutation: OptiScalerPeerMutation::Replace {
                before: &before,
                after: &after,
            },
            peer_relocation: Some(&expected),
        },
        &[],
        &[],
    )
    .expect("managed peer aggregate binding");
    for path in [source.as_str(), destination.as_str()] {
        assert!(matches!(
            binding
                .paths
                .get(&normalized_path_key(path))
                .expect("relocation path")
                .transition,
            pending_file_mutations::OptiScalerBoundTransition::Relocate { .. }
        ));
    }

    let reversed = ExpectedPeerRelocation {
        source: destination,
        destination: source,
        sha256: expected.sha256.clone(),
        ownership: FileOwnership::Owned,
        direction: PeerTopologyDirection::IntoOptiTopology,
    };
    assert!(ensure_exact_peer_relocation(&before, &after, &reversed).is_err());
    let wrong_digest = ExpectedPeerRelocation {
        sha256: Sha256Hash::new("e".repeat(64)).expect("wrong digest"),
        ..expected
    };
    assert!(ensure_exact_peer_relocation(&before, &after, &wrong_digest).is_err());
}

#[test]
fn managed_peer_relocation_binds_an_occupied_destination_delete_then_relocate() {
    let game = GameId::new("manual:occupied-peer-binding").expect("game");
    let addon_file = PathRef::new("C:/Games/Test/peer.addon64").expect("addon");
    let baseline = Sha256Hash::new("f".repeat(64)).expect("baseline");
    let installed = Sha256Hash::new("d".repeat(64)).expect("installed");
    let source = PathRef::new("C:/Games/Test/managed-old.dll").expect("source");
    let destination = PathRef::new("C:/Games/Test/managed-new.dll").expect("destination");
    let before = InstalledAddon::new(game.clone(), AddonKind::RenoDx, addon_file.clone())
        .try_with_managed_files(vec![ManagedAddonFile::owned(
            source.clone(),
            ManagedFileBaseline::Present {
                sha256: baseline.clone(),
            },
            installed.clone(),
        )])
        .expect("before peer");
    let after = InstalledAddon::new(game, AddonKind::RenoDx, addon_file)
        .try_with_managed_files(vec![ManagedAddonFile::owned(
            destination.clone(),
            ManagedFileBaseline::Present { sha256: baseline },
            installed.clone(),
        )])
        .expect("after peer");
    let expected = ExpectedPeerRelocation {
        source: source.clone(),
        destination: destination.clone(),
        sha256: installed,
        ownership: FileOwnership::Owned,
        direction: PeerTopologyDirection::IntoOptiTopology,
    };
    let mut before_topology = topology_with_peer(
        "C:/Games/Test/dxgi.dll",
        source.as_str(),
        ProxyRootPrestate::RelocatedDownstream,
        'd',
    );
    before_topology.root_slot = destination.clone();
    before_topology.outer.path = destination.clone();
    before_topology.downstream_origin = Some(before_topology.root_slot.clone());
    before_topology.outer.receipt = exact_owned(
        Sha256Hash::new("c".repeat(64)).expect("occupied digest"),
        "old-optiscaler",
    );
    let after_topology = topology_with_peer(
        "C:/Games/Test/dxgi.dll",
        destination.as_str(),
        ProxyRootPrestate::RelocatedDownstream,
        'd',
    );
    let binding = build_optiscaler_binding(
        None,
        None,
        Some(&before_topology),
        Some(&after_topology),
        PeerAggregateBinding {
            mutation: OptiScalerPeerMutation::Replace {
                before: &before,
                after: &after,
            },
            peer_relocation: Some(&expected),
        },
        &[],
        &[],
    )
    .expect("occupied destination aggregate binding");
    assert!(matches!(
        binding
            .paths
            .get(&normalized_path_key(source.as_str()))
            .expect("source binding")
            .transition,
        pending_file_mutations::OptiScalerBoundTransition::Relocate { .. }
    ));
    match &binding
        .paths
        .get(&normalized_path_key(destination.as_str()))
        .expect("destination binding")
        .transition
    {
        pending_file_mutations::OptiScalerBoundTransition::RemoveOwnedFile {
            installed: prior,
            restoration: Some(restoration),
            allow_absent,
        } => {
            assert_eq!(prior.identity(), "test:old-optiscaler");
            assert_eq!(prior.digest(), &Sha256Hash::new("c".repeat(64)).unwrap());
            assert_eq!(restoration.digest(), &expected.sha256);
            assert!(allow_absent);
        }
        transition => panic!("unexpected occupied destination transition: {transition:?}"),
    }
}

#[test]
fn optiscaler_uninstall_binds_a_present_configuration_baseline_as_restore() {
    let (_game_id, _game, adopted, topology, _root) = adoption_fixture("restore-binding");
    let baseline = adopted.configuration_baseline().clone();
    let mut parts = renderpilot_domain::OptiScalerInstallStateParts::from(&adopted);
    let configuration = parts
        .release_files
        .first_mut()
        .expect("adopted configuration");
    configuration.installed = FileReceipt::owned(
        adopted.release_files[0].installed.identity().to_owned(),
        adopted.release_files[0].installed.digest().clone(),
    )
    .expect("owned configuration receipt");
    configuration.cleanup = OptiScalerFileCleanup::PreserveCurrentThenRestoreBaseline;
    let state = renderpilot_domain::from_persisted(parts, baseline.clone()).expect("state");
    let binding = build_optiscaler_binding(
        Some(&state),
        None,
        Some(&topology),
        None,
        PeerAggregateBinding {
            mutation: OptiScalerPeerMutation::Keep,
            peer_relocation: None,
        },
        &[],
        &[],
    )
    .expect("restore binding");
    let configuration_path = normalized_path_key(state.release_files[0].path.as_str());
    match &binding
        .paths
        .get(&configuration_path)
        .expect("configuration binding")
        .transition
    {
        pending_file_mutations::OptiScalerBoundTransition::RestoreOwnedFile {
            installed,
            restoration,
        } => {
            assert_eq!(installed, &state.release_files[0].installed);
            assert_eq!(restoration, baseline.receipt().expect("present baseline"));
        }
        transition => panic!("unexpected configuration transition: {transition:?}"),
    }
    let _ = std::fs::remove_dir_all(_root);
}

#[test]
fn optiscaler_reused_removal_is_derived_only_for_exact_artifact_and_outer_roles() {
    let mut state = transition_state();
    let runtime = state.runtime_bindings.first_mut().expect("runtime binding");
    runtime.installed = FileReceipt::reused(
        runtime.installed.identity().to_owned(),
        runtime.installed.digest().clone(),
    )
    .expect("reused runtime");
    runtime.baseline = OptiScalerFileBaseline::Present {
        receipt: runtime.installed.clone(),
    };
    let mut topology = transition_topology();
    topology.outer.receipt = FileReceipt::reused(
        topology.outer.receipt.identity().to_owned(),
        topology.outer.receipt.digest().clone(),
    )
    .expect("reused outer");

    let binding = build_optiscaler_binding(
        Some(&state),
        None,
        Some(&topology),
        None,
        PeerAggregateBinding {
            mutation: OptiScalerPeerMutation::Keep,
            peer_relocation: None,
        },
        &[],
        &[],
    )
    .expect("exact artifact removal binding");
    for path in ["C:/Games/Test/core.dll", "C:/Games/Test/dxgi.dll"] {
        assert!(matches!(
            binding
                .paths
                .get(&normalized_path_key(path))
                .expect("exact reused path")
                .transition,
            pending_file_mutations::OptiScalerBoundTransition::RemoveReusedFile {
                allow_absent: true,
                ..
            }
        ));
    }

    let (_game_id, _game, adopted, adopted_topology, root) = adoption_fixture("reused-role");
    let adopted_binding = build_optiscaler_binding(
        Some(&adopted),
        None,
        Some(&adopted_topology),
        None,
        PeerAggregateBinding {
            mutation: OptiScalerPeerMutation::Keep,
            peer_relocation: None,
        },
        &[],
        &[],
    )
    .expect("adopted configuration binding");
    assert!(matches!(
        adopted_binding
            .paths
            .get(&normalized_path_key(adopted.release_files[0].path.as_str()))
            .expect("configuration path")
            .transition,
        pending_file_mutations::OptiScalerBoundTransition::ObserveReusedConfiguration { .. }
    ));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn retargeted_reused_configuration_is_observed_at_its_old_path_and_claimed_absent_at_its_new_path()
{
    let (_game_id, _game, before, topology, root) = adoption_fixture("configuration-retarget");
    let old_path = before
        .configuration_receipt()
        .expect("old configuration")
        .path
        .clone();
    let target_dir = root.join("new-target");
    let destination = target_dir.join("OptiScaler.ini");
    let mut parts = renderpilot_domain::OptiScalerInstallStateParts::from(&before);
    parts.target_dir = PathRef::new(target_dir.to_string_lossy().into_owned()).expect("new target");
    parts.target_exe_path =
        PathRef::new(target_dir.join("Game.exe").to_string_lossy().into_owned()).expect("new exe");
    parts.release_files[0].path =
        PathRef::new(destination.to_string_lossy().into_owned()).expect("new configuration path");
    parts.release_files[0].installed = FileReceipt::owned(
        "retargeted-config",
        Sha256Hash::new("e".repeat(64)).expect("new digest"),
    )
    .expect("new configuration receipt");
    parts.release_files[0].cleanup = OptiScalerFileCleanup::RemoveIfUnchanged;
    parts.adoption_state = OptiScalerAdoptionState::Managed;
    let after = OptiScalerInstallState::from_existing_with_configuration_retarget(&before, parts)
        .expect("retargeted state");

    let binding = build_optiscaler_binding(
        Some(&before),
        Some(&after),
        Some(&topology),
        Some(&topology),
        PeerAggregateBinding {
            mutation: OptiScalerPeerMutation::Keep,
            peer_relocation: None,
        },
        &[],
        &[],
    )
    .expect("retarget binding");
    assert!(matches!(
        binding
            .paths
            .get(&normalized_path_key(old_path.as_str()))
            .expect("old configuration path")
            .transition,
        pending_file_mutations::OptiScalerBoundTransition::ObserveReusedConfiguration { .. }
    ));
    assert!(matches!(
        binding
            .paths
            .get(&normalized_path_key(&destination.to_string_lossy()))
            .expect("new configuration path")
            .transition,
        pending_file_mutations::OptiScalerBoundTransition::ClaimedFile { .. }
    ));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn exact_adoption_persists_state_and_topology_without_a_pending_row() {
    let storage = SqliteStorage::in_memory().expect("storage");
    let (game_id, game, state, topology, root) = adoption_fixture("success");
    storage.upsert_game(&game).expect("game");
    let ini_before = std::fs::read(root.join("OptiScaler.ini")).expect("initial config");

    storage
        .commit_game_mutation(GameMutationCommit {
            game_id: &game_id,
            component_set: None,
            baseline_mutations: &[],
            addon: InstalledAddonMutation::OptiScaler(
                OptiScalerAggregateMutation::AdoptExactMetadata {
                    state: &state,
                    topology: &topology,
                },
            ),
            mutation_id: None,
        })
        .expect("exact adoption");

    let persisted_state = storage
        .get_optiscaler_install_state(&game_id)
        .expect("state")
        .expect("adopted state");
    let mut expected_state = state;
    expected_state.created_at = persisted_state.created_at;
    expected_state.updated_at = persisted_state.updated_at;
    assert_eq!(persisted_state, expected_state);
    assert_eq!(
        storage
            .get_proxy_topology(&game_id)
            .expect("topology")
            .as_ref(),
        Some(&topology)
    );
    assert_eq!(
        std::fs::read(root.join("OptiScaler.ini")).expect("final config"),
        ini_before,
        "metadata-only adoption must not write the live filesystem"
    );
    assert!(
        storage
            .pending_file_mutations_for_game(&game_id)
            .expect("pending rows")
            .is_empty(),
        "metadata-only adoption must not reserve a pending mutation"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn exact_adoption_accepts_only_a_unique_owned_luma_or_renodx_managed_downstream_claim() {
    for kind in [AddonKind::Luma, AddonKind::RenoDx] {
        let storage = SqliteStorage::in_memory().expect("storage");
        let label = match kind {
            AddonKind::Luma => "owned-luma-downstream",
            AddonKind::RenoDx => "owned-renodx-downstream",
            _ => unreachable!("only supported peer kinds are exercised"),
        };
        let (game_id, game, state, mut topology, root) = adoption_fixture(label);
        let downstream_path = root.join("ReShade64.dll");
        std::fs::write(&downstream_path, b"owned peer host").expect("downstream host");
        let downstream_digest =
            renderpilot_detection::sha256_file(&downstream_path).expect("downstream digest");
        let downstream_ref =
            PathRef::new(downstream_path.to_string_lossy().into_owned()).expect("downstream path");
        topology.downstream = Some(ProxyLink {
            implementation: ProxyImplementation::ReShade,
            path: downstream_ref.clone(),
            receipt: FileReceipt::owned("owned-peer-host", downstream_digest.clone())
                .expect("owned downstream receipt"),
        });
        topology.downstream_origin = Some(topology.root_slot.clone());
        topology.root_prestate = ProxyRootPrestate::RelocatedDownstream;
        topology.validate().expect("owned downstream topology");
        storage.upsert_game(&game).expect("game");
        let peer = InstalledAddon::new(
            game_id.clone(),
            kind,
            PathRef::new(root.join("peer.addon64").to_string_lossy().into_owned())
                .expect("peer addon"),
        )
        .try_with_managed_files(vec![ManagedAddonFile::owned(
            downstream_ref,
            ManagedFileBaseline::Absent,
            downstream_digest,
        )])
        .expect("exact peer managed claim");
        storage.upsert_installed_addon(&peer).expect("seed peer");

        storage
            .commit_game_mutation(GameMutationCommit {
                game_id: &game_id,
                component_set: None,
                baseline_mutations: &[],
                addon: InstalledAddonMutation::OptiScaler(
                    OptiScalerAggregateMutation::AdoptExactMetadata {
                        state: &state,
                        topology: &topology,
                    },
                ),
                mutation_id: None,
            })
            .expect("unique owned managed peer claim authorizes adoption");
        assert!(
            storage
                .get_optiscaler_install_state(&game_id)
                .expect("state")
                .is_some()
        );
        assert!(
            storage
                .get_installed_addon(&game_id)
                .expect("peer")
                .is_some_and(|persisted| peer.eq_ignoring_persistence_timestamps(&persisted)),
            "metadata adoption must not mutate the existing peer"
        );
        let _ = std::fs::remove_dir_all(root);
    }
}

#[test]
fn exact_adoption_rejects_an_owned_downstream_without_a_unique_exact_managed_claim_atomically() {
    let storage = SqliteStorage::in_memory().expect("storage");
    let (game_id, game, state, mut topology, root) = adoption_fixture("owned-downstream-reject");
    let downstream_path = root.join("ReShade64.dll");
    std::fs::write(&downstream_path, b"owned peer host").expect("downstream host");
    let downstream_digest =
        renderpilot_detection::sha256_file(&downstream_path).expect("downstream digest");
    let downstream_ref =
        PathRef::new(downstream_path.to_string_lossy().into_owned()).expect("downstream path");
    topology.downstream = Some(ProxyLink {
        implementation: ProxyImplementation::ReShade,
        path: downstream_ref.clone(),
        receipt: FileReceipt::owned("owned-peer-host", downstream_digest)
            .expect("owned downstream receipt"),
    });
    topology.downstream_origin = Some(topology.root_slot.clone());
    topology.root_prestate = ProxyRootPrestate::RelocatedDownstream;
    topology.validate().expect("owned downstream topology");
    storage.upsert_game(&game).expect("game");
    let generic_peer = InstalledAddon::new(
        game_id.clone(),
        AddonKind::RenoDx,
        PathRef::new(root.join("peer.addon64").to_string_lossy().into_owned()).expect("peer addon"),
    )
    .with_created_file(downstream_ref);
    storage
        .upsert_installed_addon(&generic_peer)
        .expect("seed generic peer");

    assert!(
        storage
            .commit_game_mutation(GameMutationCommit {
                game_id: &game_id,
                component_set: None,
                baseline_mutations: &[],
                addon: InstalledAddonMutation::OptiScaler(
                    OptiScalerAggregateMutation::AdoptExactMetadata {
                        state: &state,
                        topology: &topology,
                    },
                ),
                mutation_id: None,
            })
            .is_err()
    );
    assert!(
        storage
            .get_optiscaler_install_state(&game_id)
            .expect("state")
            .is_none()
    );
    assert!(
        storage
            .get_proxy_topology(&game_id)
            .expect("topology")
            .is_none()
    );
    assert!(
        storage
            .get_installed_addon(&game_id)
            .expect("peer")
            .is_some_and(|persisted| generic_peer.eq_ignoring_persistence_timestamps(&persisted)),
        "rejected adoption must not mutate the peer record"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn independent_peer_commit_cannot_bypass_an_active_optiscaler_aggregate() {
    let storage = SqliteStorage::in_memory().expect("storage");
    let (game_id, game, state, topology, root) = adoption_fixture("peer-commit-fence");
    storage.upsert_game(&game).expect("game");
    storage
        .commit_game_mutation(GameMutationCommit {
            game_id: &game_id,
            component_set: None,
            baseline_mutations: &[],
            addon: InstalledAddonMutation::OptiScaler(
                OptiScalerAggregateMutation::AdoptExactMetadata {
                    state: &state,
                    topology: &topology,
                },
            ),
            mutation_id: None,
        })
        .expect("exact adoption");
    let peer = InstalledAddon::new(
        game_id.clone(),
        AddonKind::RenoDx,
        PathRef::new(root.join("peer.addon64").to_string_lossy().into_owned()).expect("peer path"),
    );

    let error = storage
        .commit_game_mutation(GameMutationCommit {
            game_id: &game_id,
            component_set: None,
            baseline_mutations: &[],
            addon: InstalledAddonMutation::Upsert(&peer),
            mutation_id: None,
        })
        .expect_err("independent peer receipt commit must be fenced");

    assert_eq!(
        error.kind(),
        &renderpilot_application::AppErrorKind::PeerTopologyConflict {
            peer_kind: AddonKind::RenoDx,
        }
    );
    assert!(storage.get_installed_addon(&game_id).unwrap().is_none());
    assert_eq!(
        storage.get_proxy_topology(&game_id).unwrap().as_ref(),
        Some(&topology),
        "the active OptiScaler aggregate must remain exact"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn exact_adoption_accepts_durable_receipts_without_storage_fs_reads_and_rejects_existing_aggregate()
{
    let storage = SqliteStorage::in_memory().expect("storage");
    let (game_id, game, state, topology, root) = adoption_fixture("drift");
    storage.upsert_game(&game).expect("game");
    std::fs::write(root.join("OptiScaler.ini"), b"foreign").expect("foreign edit");

    storage
        .commit_game_mutation(GameMutationCommit {
            game_id: &game_id,
            component_set: None,
            baseline_mutations: &[],
            addon: InstalledAddonMutation::OptiScaler(
                OptiScalerAggregateMutation::AdoptExactMetadata {
                    state: &state,
                    topology: &topology,
                },
            ),
            mutation_id: None,
        })
        .expect("durable adoption does not inspect live filesystem");
    let persisted_state = storage
        .get_optiscaler_install_state(&game_id)
        .expect("state")
        .expect("adopted state");
    assert_eq!(persisted_state.release_files, state.release_files);
    assert_eq!(
        storage
            .get_proxy_topology(&game_id)
            .expect("topology")
            .as_ref(),
        Some(&topology)
    );
    let _ = std::fs::remove_dir_all(root);

    let (game_id, game, state, topology, root) = adoption_fixture("existing");
    storage.upsert_game(&game).expect("game");
    storage
        .commit_game_mutation(GameMutationCommit {
            game_id: &game_id,
            component_set: None,
            baseline_mutations: &[],
            addon: InstalledAddonMutation::OptiScaler(
                OptiScalerAggregateMutation::AdoptExactMetadata {
                    state: &state,
                    topology: &topology,
                },
            ),
            mutation_id: None,
        })
        .expect("first adoption");
    storage
        .commit_game_mutation(GameMutationCommit {
            game_id: &game_id,
            component_set: None,
            baseline_mutations: &[],
            addon: InstalledAddonMutation::OptiScaler(
                OptiScalerAggregateMutation::AdoptExactMetadata {
                    state: &state,
                    topology: &topology,
                },
            ),
            mutation_id: None,
        })
        .expect_err("existing aggregate must reject adoption");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn filesystem_commit_rejects_an_incomplete_shared_journal_without_fs_reads() {
    let storage = SqliteStorage::in_memory().expect("storage");
    let (game_id, game, state, topology, root) = adoption_fixture("postimage-drift");
    storage.upsert_game(&game).expect("game");
    storage
        .commit_game_mutation(GameMutationCommit {
            game_id: &game_id,
            component_set: None,
            baseline_mutations: &[],
            addon: InstalledAddonMutation::OptiScaler(
                OptiScalerAggregateMutation::AdoptExactMetadata {
                    state: &state,
                    topology: &topology,
                },
            ),
            mutation_id: None,
        })
        .expect("initial adoption");

    let before_state = storage
        .get_optiscaler_install_state(&game_id)
        .expect("state")
        .expect("persisted state");
    let before_topology = storage
        .get_proxy_topology(&game_id)
        .expect("topology")
        .expect("persisted topology");
    let ini = root.join("OptiScaler.ini");
    let installed_identity = before_state.release_files[0]
        .installed
        .identity()
        .to_owned();
    let after_bytes = b"[OptiScaler]\nEnabled=0\n";
    std::fs::write(&ini, after_bytes).expect("applied postimage");
    let after_digest = renderpilot_detection::sha256_file(&ini).expect("after hash");
    let journal_after = FileReceipt::owned(installed_identity.clone(), after_digest)
        .expect("journal after receipt");
    let mut after_state = before_state.clone();
    after_state.release_files[0].installed = FileReceipt::owned(
        installed_identity,
        Sha256Hash::new("d".repeat(64)).expect("different state digest"),
    )
    .expect("after receipt");

    let mutation_id = "opti-postimage-drift";
    let ini_path = ini.to_string_lossy().into_owned();
    let root_path = root.to_string_lossy().into_owned();
    let manifest = applied_optiscaler_journal(
        &root_path,
        &ini_path,
        &before_state.release_files[0].installed,
        &journal_after,
    );
    storage
        .prepare_file_mutation(&PendingFileMutationRow {
            id: mutation_id.to_owned(),
            game_id: game_id.clone(),
            feature: renderpilot_domain::mutation_features::OPTISCALER_UPDATE.to_owned(),
            subject_id: Some(before_topology.id.clone()),
            state: PendingFileMutationState::Prepared,
            manifest_json: manifest,
        })
        .expect("prepared applied mutation");

    storage
        .with_transaction(|transaction| {
            observations::invalidate_game_authority_within_transaction(
                transaction,
                &game_id,
                "replacement_mutation",
                Some("other-mutation"),
            )
            .map(|_| ())
        })
        .expect("replace catalog authority");
    storage
        .commit_game_mutation(GameMutationCommit {
            game_id: &game_id,
            component_set: None,
            baseline_mutations: &[],
            addon: InstalledAddonMutation::OptiScaler(OptiScalerAggregateMutation::Filesystem {
                before_state: Some(&before_state),
                after_state: Some(&after_state),
                before_topology: Some(&before_topology),
                after_topology: Some(&before_topology),
                peer: OptiScalerPeerMutation::Keep,
                mutation_id,
            }),
            mutation_id: None,
        })
        .expect_err("replacement catalog authority must fence aggregate commit");
    assert_eq!(
        storage
            .get_optiscaler_install_state(&game_id)
            .expect("state after authority rejection")
            .as_ref(),
        Some(&before_state)
    );
    storage
        .with_transaction(|transaction| {
            observations::invalidate_game_authority_within_transaction(
                transaction,
                &game_id,
                "prepared_file_mutation",
                Some(mutation_id),
            )
            .map(|_| ())
        })
        .expect("restore matching catalog authority");

    std::fs::write(&ini, b"foreign replacement").expect("foreign replacement");
    storage
        .commit_game_mutation(GameMutationCommit {
            game_id: &game_id,
            component_set: None,
            baseline_mutations: &[],
            addon: InstalledAddonMutation::OptiScaler(OptiScalerAggregateMutation::Filesystem {
                before_state: Some(&before_state),
                after_state: Some(&after_state),
                before_topology: Some(&before_topology),
                after_topology: Some(&before_topology),
                peer: OptiScalerPeerMutation::Keep,
                mutation_id,
            }),
            mutation_id: None,
        })
        .expect_err("incomplete durable journal must fail closed without filesystem reads");

    assert_eq!(
        storage
            .get_optiscaler_install_state(&game_id)
            .expect("state after durable rejection")
            .as_ref(),
        Some(&before_state)
    );
    assert_eq!(
        storage
            .pending_file_mutations_for_game(&game_id)
            .expect("pending rows")
            .first()
            .map(|row| row.state),
        Some(PendingFileMutationState::Prepared)
    );
    let _ = std::fs::remove_dir_all(root);
}
