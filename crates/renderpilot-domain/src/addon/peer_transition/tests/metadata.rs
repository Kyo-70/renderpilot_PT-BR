use super::*;
use crate::{InstalledAddonHostKind, TrackedSource, TrackedSourceRole};

fn source(role: TrackedSourceRole, url: &str, digest: &str, advisory: bool) -> TrackedSource {
    let source = TrackedSource::new(role, url, Some("etag".to_owned()), digest.to_owned())
        .with_last_modified(Some("date".to_owned()))
        .with_channel("stable");
    if advisory {
        source.with_advisory()
    } else {
        source
    }
}

fn luma(sources: Vec<TrackedSource>) -> InstalledAddon {
    InstalledAddon::new(game(), AddonKind::Luma, path("peer.addon64")).with_tracked_sources(sources)
}

#[test]
fn metadata_fields_and_luma_archive_digests_may_refresh() {
    let before = luma(vec![
        source(
            TrackedSourceRole::AddonPayload,
            "https://example.invalid/payload-old",
            "payload-old",
            false,
        ),
        source(
            TrackedSourceRole::HostBinary,
            "https://example.invalid/host-old",
            "host-old",
            false,
        ),
        source(
            TrackedSourceRole::DgVoodooWrapper,
            "https://example.invalid/dgvoodoo-old",
            "wrapper-old",
            true,
        ),
        source(
            TrackedSourceRole::DlssFix,
            "https://example.invalid/dlss-fix",
            "dlss-fixed",
            false,
        ),
    ])
    .with_addon_version("old")
    .with_reshade_channel("stable");
    let after = luma(vec![
        source(
            TrackedSourceRole::AddonPayload,
            "https://example.invalid/payload-new",
            "payload-new",
            false,
        )
        .with_last_modified(Some("new-date".to_owned()))
        .with_channel("nightly"),
        source(
            TrackedSourceRole::HostBinary,
            "https://example.invalid/host-new",
            "host-new",
            false,
        ),
        source(
            TrackedSourceRole::DgVoodooWrapper,
            "https://example.invalid/dgvoodoo-new",
            "wrapper-new",
            true,
        ),
        source(
            TrackedSourceRole::DlssFix,
            "https://example.invalid/dlss-fix-new",
            "dlss-fixed",
            true,
        ),
    ])
    .with_addon_version("new")
    .with_reshade_channel("nightly");

    validate_peer_metadata_only(Some(&before), Some(&after), None, None)
        .expect("metadata and Luma archive provenance may refresh");
}

#[test]
fn physical_or_ambiguous_digest_interpretations_are_immutable() {
    let cases = [
        (
            AddonKind::Luma,
            TrackedSourceRole::AddonPayload,
            true,
            false,
        ),
        (AddonKind::Luma, TrackedSourceRole::HostBinary, true, false),
        (AddonKind::Luma, TrackedSourceRole::DlssFix, false, false),
        (
            AddonKind::RenoDx,
            TrackedSourceRole::AddonPayload,
            false,
            false,
        ),
    ];

    for (kind, role, before_advisory, after_advisory) in cases {
        let before =
            InstalledAddon::new(game(), kind, path("peer.addon64")).with_tracked_source(source(
                role,
                "https://example.invalid/old",
                "old-digest",
                before_advisory,
            ));
        let after = before.clone().with_tracked_sources(vec![source(
            role,
            "https://example.invalid/new",
            "new-digest",
            after_advisory,
        )]);
        assert!(matches!(
            validate_peer_metadata_only(Some(&before), Some(&after), None, None),
            Err(PeerTransitionError::InvalidPeerSnapshot(_))
        ));
    }
}

#[test]
fn tracked_source_sequence_and_roles_are_exact() {
    let first = source(
        TrackedSourceRole::AddonPayload,
        "https://example.invalid/payload",
        "payload",
        false,
    );
    let second = source(
        TrackedSourceRole::HostBinary,
        "https://example.invalid/host",
        "host",
        false,
    );
    let before = luma(vec![first.clone(), second.clone()]);

    for sources in [
        vec![second.clone(), first.clone()],
        vec![first.clone()],
        vec![
            first,
            second.clone(),
            source(TrackedSourceRole::DlssFix, "u", "d", false),
        ],
        vec![
            source(
                TrackedSourceRole::DgVoodooWrapper,
                "https://example.invalid/payload",
                "payload",
                false,
            ),
            second,
        ],
    ] {
        let after = before.clone().with_tracked_sources(sources);
        assert!(matches!(
            validate_peer_metadata_only(Some(&before), Some(&after), None, None),
            Err(PeerTransitionError::InvalidPeerSnapshot(_))
        ));
    }
}

#[test]
fn every_physical_peer_claim_is_immutable() {
    let before = peer(&["payload.dll"], &["payload.dll.bak"], Vec::new());
    let physical = [
        before
            .clone()
            .with_addon_version("new")
            .with_created_file(path("new.dll")),
        before.clone().with_backed_up_file(path("new.bak")),
        before
            .clone()
            .try_with_managed_files(vec![managed(
                "managed.dll",
                ManagedFileMode::Owned,
                ManagedFileBaseline::Absent,
                'a',
            )])
            .expect("managed claim"),
        before.clone().with_host_kind(InstalledAddonHostKind::Proxy),
        before.clone().with_registered_exe_path(path("game.exe")),
        InstalledAddon::new(game(), AddonKind::RenoDx, path("other.addon64"))
            .with_created_file(path("payload.dll")),
    ];

    for after in physical {
        assert!(matches!(
            validate_peer_metadata_only(Some(&before), Some(&after), None, None),
            Err(PeerTransitionError::InvalidPeerSnapshot(_))
        ));
    }
}

#[test]
fn peer_presence_and_topology_must_remain_exact() {
    let before = peer(&[], &[], Vec::new());
    assert!(matches!(
        validate_peer_metadata_only(None, Some(&before), None, None),
        Err(PeerTransitionError::InvalidPeerSnapshot(_))
    ));
    assert!(matches!(
        validate_peer_metadata_only(Some(&before), None, None, None),
        Err(PeerTransitionError::InvalidPeerSnapshot(_))
    ));

    let topology = outer_topology();
    let mut changed_topology = topology.clone();
    changed_topology.id = "topology:changed".to_owned();
    assert!(
        validate_peer_metadata_only(
            Some(&before),
            Some(&before),
            Some(&topology),
            Some(&changed_topology),
        )
        .is_err()
    );

    validate_peer_metadata_only(None, None, None, None).expect("an unchanged absent peer is valid");
}
