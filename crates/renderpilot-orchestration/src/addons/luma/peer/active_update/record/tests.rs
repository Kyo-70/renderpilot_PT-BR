use renderpilot_domain::{
    AddonKind, GameId, InstalledAddon, ManagedAddonFile, ManagedFileBaseline, PathRef, Sha256Hash,
    TrackedSource, TrackedSourceRole,
};

use super::super::model::{
    DgVoodooProjection, DlssProjection, LumaActiveUpdateClaimDelta, PayloadRecordProjection,
};
use super::{claims, rebuild_record};

fn hash(byte: char) -> Sha256Hash {
    Sha256Hash::new(format!("{:x}", (byte as u8) % 16).repeat(64)).expect("hash")
}

fn path(value: &str) -> PathRef {
    PathRef::new(format!("C:/Games/Test/{value}")).expect("path")
}

fn game() -> GameId {
    GameId::new("manual:active-luma-record").expect("game")
}

fn sources(host: Option<char>) -> Vec<TrackedSource> {
    let mut result = vec![TrackedSource::new(
        TrackedSourceRole::AddonPayload,
        "https://example.invalid/luma.zip",
        None,
        hash('p').as_str(),
    )];
    if let Some(host) = host {
        result.push(TrackedSource::new(
            TrackedSourceRole::HostBinary,
            "https://example.invalid/reshade.dll",
            None,
            hash(host).as_str(),
        ));
    }
    result
}

fn record(host: ManagedAddonFile, dlss: Option<ManagedAddonFile>) -> InstalledAddon {
    let result = InstalledAddon::new(game(), AddonKind::Luma, path("luma.addon64"))
        .with_created_file(path("old.bin"))
        .with_backed_up_file(path("old.bin"))
        .with_timestamps(Some(11), Some(22));
    let mut managed = vec![host];
    if let Some(dlss) = dlss {
        managed.push(dlss);
    }
    result
        .try_with_managed_files(managed)
        .expect("managed record")
}

fn projection_payload(
    claims: LumaActiveUpdateClaimDelta,
    addon_file: &str,
) -> PayloadRecordProjection {
    PayloadRecordProjection::new(path(addon_file), claims)
}

fn empty_dgvoodoo() -> DgVoodooProjection {
    DgVoodooProjection::new(LumaActiveUpdateClaimDelta::default())
}

#[test]
fn preserve_rebuild_keeps_claim_order_metadata_and_owned_channel() {
    let before = record(
        ManagedAddonFile::owned(path("dxgi.dll"), ManagedFileBaseline::Absent, hash('h')),
        None,
    );
    let after = rebuild_record(
        &before,
        projection_payload(LumaActiveUpdateClaimDelta::default(), "luma.addon64"),
        empty_dgvoodoo(),
        &ManagedAddonFile::owned(path("dxgi.dll"), ManagedFileBaseline::Absent, hash('h')),
        DlssProjection::new(None),
        sources(Some('h')),
        Some("nightly-build".to_owned()),
    )
    .expect("record");

    assert_eq!(after.created_files(), before.created_files());
    assert_eq!(after.backed_up_files(), before.backed_up_files());
    assert_eq!(after.installed_at(), Some(11));
    assert_eq!(after.updated_at(), Some(22));
    assert_eq!(
        after.host_kind(),
        Some(renderpilot_domain::InstalledAddonHostKind::Proxy)
    );
    assert_eq!(after.reshade_channel(), Some("nightly"));
    assert!(after.registered_exe_path().is_none());
    assert_eq!(after.addon_version(), Some("nightly-build"));
}

#[test]
fn claim_merge_preserves_survivors_and_sorts_only_new_paths() {
    let before = record(
        ManagedAddonFile::owned(path("dxgi.dll"), ManagedFileBaseline::Absent, hash('h')),
        None,
    );
    let delta = LumaActiveUpdateClaimDelta::new(
        vec![path("zeta.bin"), path("alpha.bin")],
        vec![path("old.bin")],
        vec![],
        vec![path("old.bin")],
    );
    let (created, backed) = claims::merge(
        &before,
        delta,
        LumaActiveUpdateClaimDelta::default(),
        &path("luma.addon64"),
    )
    .expect("claims");

    assert_eq!(
        created.iter().map(PathRef::as_str).collect::<Vec<_>>(),
        vec![
            "C:/Games/Test/luma.addon64",
            "C:/Games/Test/alpha.bin",
            "C:/Games/Test/zeta.bin"
        ]
    );
    assert!(backed.is_empty());
}

#[test]
fn claim_merge_rejects_alias_duplicates_overlap_and_orphan_backups() {
    let before = record(
        ManagedAddonFile::owned(path("dxgi.dll"), ManagedFileBaseline::Absent, hash('h')),
        None,
    );
    let duplicate = LumaActiveUpdateClaimDelta::new(
        vec![path("new.bin"), path("NEW.BIN")],
        vec![],
        vec![],
        vec![],
    );
    assert!(
        claims::merge(
            &before,
            duplicate,
            LumaActiveUpdateClaimDelta::default(),
            &path("luma.addon64")
        )
        .is_err()
    );

    let overlap = LumaActiveUpdateClaimDelta::new(
        vec![path("old.bin")],
        vec![path("old.bin")],
        vec![],
        vec![],
    );
    assert!(
        claims::merge(
            &before,
            overlap,
            LumaActiveUpdateClaimDelta::default(),
            &path("luma.addon64")
        )
        .is_err()
    );

    let orphan = LumaActiveUpdateClaimDelta::new(vec![], vec![], vec![path("orphan.bin")], vec![]);
    assert!(
        claims::merge(
            &before,
            orphan,
            LumaActiveUpdateClaimDelta::default(),
            &path("luma.addon64")
        )
        .is_err()
    );
}

#[test]
fn reused_host_has_no_host_provenance_and_retains_dlss_position() {
    let host = ManagedAddonFile::reused(path("dxgi.dll"), hash('h'));
    let dlss = ManagedAddonFile::reused(path("nvngx_dlss.dll"), hash('d'));
    let before = record(host.clone(), Some(dlss.clone()));
    let after = rebuild_record(
        &before,
        projection_payload(LumaActiveUpdateClaimDelta::default(), "luma.addon64"),
        empty_dgvoodoo(),
        &host,
        DlssProjection::new(Some(dlss.clone())),
        sources(None),
        None,
    )
    .expect("record");

    assert_eq!(
        after.managed_files(),
        &[after.managed_files()[0].clone(), dlss]
    );
    assert_eq!(after.reshade_channel(), None);
}

#[test]
fn owned_dlss_release_omits_binding_and_new_binding_is_appended() {
    let host = ManagedAddonFile::owned(path("dxgi.dll"), ManagedFileBaseline::Absent, hash('h'));
    let old_dlss = ManagedAddonFile::owned(
        path("nvngx_dlss.dll"),
        ManagedFileBaseline::Absent,
        hash('d'),
    );
    let before = record(host.clone(), Some(old_dlss));
    let after = rebuild_record(
        &before,
        projection_payload(LumaActiveUpdateClaimDelta::default(), "luma.addon64"),
        empty_dgvoodoo(),
        &host,
        DlssProjection::new(None),
        sources(Some('h')),
        None,
    )
    .expect("release record");
    assert_eq!(after.managed_files(), std::slice::from_ref(&host));

    let new_dlss = ManagedAddonFile::owned(
        path("nvngx_dlss.dll"),
        ManagedFileBaseline::Absent,
        hash('n'),
    );
    let after = rebuild_record(
        &after,
        projection_payload(LumaActiveUpdateClaimDelta::default(), "luma.addon64"),
        empty_dgvoodoo(),
        &host,
        DlssProjection::new(Some(new_dlss.clone())),
        sources(Some('h')),
        None,
    )
    .expect("acquire record");
    assert_eq!(after.managed_files(), &[host, new_dlss]);
}

#[test]
fn provenance_rejects_invalid_roles_advisory_payload_and_host_digest_drift() {
    let host = ManagedAddonFile::owned(path("dxgi.dll"), ManagedFileBaseline::Absent, hash('h'));
    let before = record(host.clone(), None);
    let duplicate = vec![
        TrackedSource::new(
            TrackedSourceRole::AddonPayload,
            "https://a",
            None,
            hash('p').as_str(),
        ),
        TrackedSource::new(
            TrackedSourceRole::AddonPayload,
            "https://b",
            None,
            hash('p').as_str(),
        ),
    ];
    assert!(
        rebuild_record(
            &before,
            projection_payload(LumaActiveUpdateClaimDelta::default(), "luma.addon64"),
            empty_dgvoodoo(),
            &host,
            DlssProjection::new(None),
            duplicate,
            None,
        )
        .is_err()
    );

    let advisory = vec![
        TrackedSource::new(
            TrackedSourceRole::AddonPayload,
            "https://a",
            None,
            hash('p').as_str(),
        )
        .with_advisory(),
    ];
    assert!(
        rebuild_record(
            &before,
            projection_payload(LumaActiveUpdateClaimDelta::default(), "luma.addon64"),
            empty_dgvoodoo(),
            &host,
            DlssProjection::new(None),
            advisory,
            None,
        )
        .is_err()
    );

    assert!(
        rebuild_record(
            &before,
            projection_payload(LumaActiveUpdateClaimDelta::default(), "luma.addon64"),
            empty_dgvoodoo(),
            &host,
            DlssProjection::new(None),
            sources(Some('y')),
            None,
        )
        .is_err()
    );
}

#[test]
fn reused_host_rejects_host_provenance_and_dlss_fix_is_never_retained() {
    let host = ManagedAddonFile::reused(path("dxgi.dll"), hash('h'));
    let before = record(host.clone(), None);
    assert!(
        rebuild_record(
            &before,
            projection_payload(LumaActiveUpdateClaimDelta::default(), "luma.addon64"),
            empty_dgvoodoo(),
            &host,
            DlssProjection::new(None),
            sources(Some('h')),
            None,
        )
        .is_err()
    );

    let mut invalid_sources = sources(None);
    invalid_sources.push(TrackedSource::new(
        TrackedSourceRole::DlssFix,
        "https://example.invalid/dlss-fix",
        None,
        hash('f').as_str(),
    ));
    assert!(
        rebuild_record(
            &before,
            projection_payload(LumaActiveUpdateClaimDelta::default(), "luma.addon64"),
            empty_dgvoodoo(),
            &host,
            DlssProjection::new(None),
            invalid_sources,
            None,
        )
        .is_err()
    );
}
