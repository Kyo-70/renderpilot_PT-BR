use super::*;
use crate::{TrackedSource, TrackedSourceRole};

fn managed_record(kind: AddonKind, files: Vec<ManagedAddonFile>) -> InstalledAddon {
    InstalledAddon::new(game(), kind, path("peer.addon64"))
        .try_with_managed_files(files)
        .expect("managed record")
}

fn reused(name: &str, digest: char) -> ManagedAddonFile {
    ManagedAddonFile::reused(path(name), hash(digest))
}

fn owned(name: &str, digest: char) -> ManagedAddonFile {
    ManagedAddonFile::owned(path(name), ManagedFileBaseline::Absent, hash(digest))
}

#[test]
fn derives_single_and_multiple_removals_with_canonical_guards() {
    let first = reused("zeta.dll", 'z');
    let second = reused("Alpha.dll", 'a');
    let third = reused("middle.dll", 'm');
    let before = managed_record(
        AddonKind::RenoDx,
        vec![first.clone(), second.clone(), third.clone()],
    );

    let single = PeerReusedClaimMembershipContract::derive(
        &before,
        &managed_record(AddonKind::RenoDx, vec![first.clone(), third]),
        &outer_topology(),
    )
    .expect("single removal");
    assert_eq!(single.read_guards().len(), 1);
    assert_eq!(single.read_guards()[0].path(), second.path());
    assert_eq!(
        single.read_guards()[0].sources(),
        &[PeerReadGuardSource::ManagedReusedLive]
    );
    assert_eq!(
        single.read_guards()[0].expectation(),
        &PeerReadGuardExpectation::Digest {
            sha256: second.installed_sha256().clone()
        }
    );

    let multiple = PeerReusedClaimMembershipContract::derive(
        &before,
        &managed_record(AddonKind::RenoDx, vec![first]),
        &outer_topology(),
    )
    .expect("multiple removals");
    assert_eq!(
        multiple
            .read_guards()
            .iter()
            .map(|guard| guard.path().as_str())
            .collect::<Vec<_>>(),
        vec!["C:/Games/Test/Alpha.dll", "C:/Games/Test/middle.dll"]
    );
}

#[test]
fn retains_exact_managed_order_and_rejects_zero_delta() {
    let first = reused("first.dll", 'a');
    let second = reused("second.dll", 'b');
    let third = reused("third.dll", 'c');
    let before = managed_record(
        AddonKind::RenoDx,
        vec![first.clone(), second.clone(), third.clone()],
    );

    PeerReusedClaimMembershipContract::derive(
        &before,
        &managed_record(AddonKind::RenoDx, vec![first.clone(), third]),
        &outer_topology(),
    )
    .expect("retained entries may skip a removed claim");

    assert!(
        PeerReusedClaimMembershipContract::derive(
            &before,
            &managed_record(
                AddonKind::RenoDx,
                vec![first, second, reused("third.dll", 'c')]
            ),
            &outer_topology(),
        )
        .is_err()
    );
}

#[test]
fn accepts_addition_and_rejects_change_reorder_and_owned_removal() {
    let first = reused("first.dll", 'a');
    let second = reused("second.dll", 'b');
    let before = managed_record(AddonKind::RenoDx, vec![first.clone(), second.clone()]);

    let additions = managed_record(
        AddonKind::RenoDx,
        vec![first.clone(), second.clone(), reused("third.dll", 'c')],
    );
    let addition =
        PeerReusedClaimMembershipContract::derive(&before, &additions, &outer_topology())
            .expect("reused membership addition");
    assert_eq!(addition.read_guards().len(), 1);
    assert_eq!(
        addition.read_guards()[0].path(),
        additions.managed_files()[2].path()
    );

    let changed = managed_record(
        AddonKind::RenoDx,
        vec![first.clone(), reused("second.dll", 'x')],
    );
    assert!(
        PeerReusedClaimMembershipContract::derive(&before, &changed, &outer_topology()).is_err()
    );

    let reordered = managed_record(AddonKind::RenoDx, vec![second.clone(), first]);
    assert!(
        PeerReusedClaimMembershipContract::derive(&before, &reordered, &outer_topology()).is_err()
    );

    let owned_before = managed_record(
        AddonKind::RenoDx,
        vec![owned("owned.dll", 'o'), second.clone()],
    );
    let owned_removed = managed_record(AddonKind::RenoDx, vec![second]);
    assert!(matches!(
        PeerReusedClaimMembershipContract::derive(&owned_before, &owned_removed, &outer_topology()),
        Err(PeerTransitionError::InvalidManagedModeTransition(_))
    ));
}

#[test]
fn multiple_additions_are_canonical_and_owned_or_mode_changes_are_rejected() {
    let retained = reused("retained.dll", 'r');
    let before = managed_record(AddonKind::Luma, vec![retained.clone()]);
    let after = managed_record(
        AddonKind::Luma,
        vec![
            retained.clone(),
            reused("zeta.dll", 'z'),
            reused("Alpha.dll", 'a'),
        ],
    );
    let contract = PeerReusedClaimMembershipContract::derive(&before, &after, &outer_topology())
        .expect("multiple reused additions");
    assert_eq!(
        contract
            .read_guards()
            .iter()
            .map(|guard| guard.path().as_str())
            .collect::<Vec<_>>(),
        vec!["C:/Games/Test/Alpha.dll", "C:/Games/Test/zeta.dll"]
    );

    let owned_addition = managed_record(AddonKind::Luma, vec![retained, owned("owned.dll", 'o')]);
    assert!(matches!(
        PeerReusedClaimMembershipContract::derive(&before, &owned_addition, &outer_topology()),
        Err(PeerTransitionError::InvalidManagedModeTransition(_))
    ));

    let owned_before = managed_record(AddonKind::Luma, vec![owned("owned.dll", 'o')]);
    let owned_changed = managed_record(AddonKind::Luma, vec![owned("owned.dll", 'x')]);
    assert!(matches!(
        PeerReusedClaimMembershipContract::derive(&owned_before, &owned_changed, &outer_topology()),
        Err(PeerTransitionError::InvalidPeerSnapshot(
            "retained managed claim changed"
        ))
    ));

    let mode_changed = managed_record(AddonKind::Luma, vec![owned("retained.dll", 'r')]);
    assert!(matches!(
        PeerReusedClaimMembershipContract::derive(&before, &mode_changed, &outer_topology()),
        Err(PeerTransitionError::InvalidManagedModeTransition(_))
    ));
}

#[test]
fn accepts_simultaneous_add_and_remove_with_canonical_union_guards() {
    let removed = reused("old.dll", 'o');
    let retained = reused("retained.dll", 'r');
    let added = reused("new.dll", 'n');
    let before = managed_record(AddonKind::Luma, vec![removed.clone(), retained.clone()]);
    let after = managed_record(AddonKind::Luma, vec![retained, added.clone()]);

    let contract = PeerReusedClaimMembershipContract::derive(&before, &after, &outer_topology())
        .expect("simultaneous membership change");
    assert_eq!(
        contract
            .read_guards()
            .iter()
            .map(|guard| guard.path().as_str())
            .collect::<Vec<_>>(),
        vec!["C:/Games/Test/new.dll", "C:/Games/Test/old.dll"]
    );
    assert_eq!(
        contract.read_guards()[0].expectation(),
        &PeerReadGuardExpectation::Digest {
            sha256: added.installed_sha256().clone()
        }
    );
    assert_eq!(
        contract.read_guards()[1].expectation(),
        &PeerReadGuardExpectation::Digest {
            sha256: removed.installed_sha256().clone()
        }
    );
}

#[test]
fn rejects_kind_game_outer_and_topology_mismatches() {
    let before = managed_record(AddonKind::RenoDx, vec![reused("shared.dll", 'r')]);
    let after = managed_record(AddonKind::RenoDx, Vec::new());

    let other_kind = managed_record(AddonKind::Luma, Vec::new());
    assert!(
        PeerReusedClaimMembershipContract::derive(&before, &other_kind, &outer_topology()).is_err()
    );

    let other_game = GameId::new("manual:other-membership-game").expect("game");
    let other_game_after = InstalledAddon::new(other_game, AddonKind::RenoDx, path("peer.addon64"));
    assert!(
        PeerReusedClaimMembershipContract::derive(&before, &other_game_after, &outer_topology())
            .is_err()
    );

    let mut non_optiscaler = outer_topology();
    non_optiscaler.outer.implementation = ProxyImplementation::ReShade;
    assert!(PeerReusedClaimMembershipContract::derive(&before, &after, &non_optiscaler).is_err());

    let mut invalid = outer_topology();
    invalid.root_slot = path("different-root.dll");
    assert!(PeerReusedClaimMembershipContract::derive(&before, &after, &invalid).is_err());
}

#[test]
fn rejects_physical_changes_and_preserves_existing_luma_refresh_policy() {
    let source_before = TrackedSource::new(
        TrackedSourceRole::AddonPayload,
        "https://example.invalid/old",
        None,
        "old-digest",
    );
    let source_after = TrackedSource::new(
        TrackedSourceRole::AddonPayload,
        "https://example.invalid/new",
        None,
        "new-digest",
    );
    let before = managed_record(AddonKind::Luma, vec![reused("shared.dll", 'r')])
        .with_tracked_source(source_before);
    let after = managed_record(AddonKind::Luma, Vec::new())
        .with_tracked_source(source_after)
        .with_addon_version("new");
    PeerReusedClaimMembershipContract::derive(&before, &after, &outer_topology())
        .expect("existing Luma source refresh policy");

    let physical_change = after.with_created_file(path("new.dll"));
    assert!(
        PeerReusedClaimMembershipContract::derive(&before, &physical_change, &outer_topology())
            .is_err()
    );

    let renodx_before = managed_record(AddonKind::RenoDx, vec![reused("shared.dll", 'r')])
        .with_tracked_source(TrackedSource::new(
            TrackedSourceRole::AddonPayload,
            "https://example.invalid/old",
            None,
            "old-digest",
        ));
    let renodx_after =
        managed_record(AddonKind::RenoDx, Vec::new()).with_tracked_source(TrackedSource::new(
            TrackedSourceRole::AddonPayload,
            "https://example.invalid/new",
            None,
            "new-digest",
        ));
    assert!(
        PeerReusedClaimMembershipContract::derive(&renodx_before, &renodx_after, &outer_topology())
            .is_err()
    );
}

#[test]
fn rejects_duplicate_and_overlapping_deserialized_claims() {
    let valid = managed_record(AddonKind::RenoDx, vec![reused("shared.dll", 'r')]);
    let mut duplicate_json = serde_json::to_value(&valid).expect("serialize");
    let claim = serde_json::to_value(valid.managed_files()[0].clone()).expect("claim");
    duplicate_json["managed_files"] = serde_json::json!([claim.clone(), claim]);
    let duplicate: InstalledAddon = serde_json::from_value(duplicate_json).expect("deserialize");
    assert!(
        PeerReusedClaimMembershipContract::derive(&duplicate, &valid, &outer_topology()).is_err()
    );

    let mut overlap_json = serde_json::to_value(&valid).expect("serialize");
    overlap_json["created_files"] = serde_json::json!([
        valid.addon_file().as_str(),
        valid.managed_files()[0].path().as_str()
    ]);
    let overlap: InstalledAddon = serde_json::from_value(overlap_json).expect("deserialize");
    assert!(
        PeerReusedClaimMembershipContract::derive(&overlap, &valid, &outer_topology()).is_err()
    );
}

#[test]
fn rejects_malformed_reused_baseline_and_same_key_mutations() {
    let valid = managed_record(AddonKind::Luma, vec![reused("shared.dll", 'r')]);

    let mut malformed_json = serde_json::to_value(&valid).expect("serialize");
    malformed_json["managed_files"][0]["baseline"] = serde_json::json!({ "state": "absent" });
    let malformed: InstalledAddon = serde_json::from_value(malformed_json).expect("deserialize");
    assert!(
        PeerReusedClaimMembershipContract::derive(&valid, &malformed, &outer_topology()).is_err()
    );

    let mut spelling_json = serde_json::to_value(&valid).expect("serialize");
    spelling_json["managed_files"][0]["path"] = serde_json::json!("C:/Games/Test/SHARED.dll");
    let spelling: InstalledAddon = serde_json::from_value(spelling_json).expect("deserialize");
    assert!(
        PeerReusedClaimMembershipContract::derive(&valid, &spelling, &outer_topology()).is_err()
    );

    let mut digest_json = serde_json::to_value(&valid).expect("serialize");
    digest_json["managed_files"][0]["installed_sha256"] = serde_json::json!(hash('x').as_str());
    let digest: InstalledAddon = serde_json::from_value(digest_json).expect("deserialize");
    assert!(PeerReusedClaimMembershipContract::derive(&valid, &digest, &outer_topology()).is_err());
}
