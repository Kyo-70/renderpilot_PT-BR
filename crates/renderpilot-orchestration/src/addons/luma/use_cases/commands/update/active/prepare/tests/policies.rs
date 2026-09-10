use renderpilot_domain::TrackedSourceRole;

use super::{dgvoodoo, host, payload, sources};
use super::{phase1, source};
use crate::addons::luma::types::{LumaExternalRequirement, ManagedArchiveSource};
use crate::addons::luma::use_cases::commands::update::active::model::DgVoodooLocalDecision;

#[tokio::test]
async fn local_payload_matrix_selects_full_without_a_head_request() {
    let mut advisory = phase1();
    advisory.record = advisory.record.with_tracked_sources(vec![
        source(TrackedSourceRole::AddonPayload, "https://old.invalid").with_advisory(),
    ]);
    assert_eq!(
        payload::plan(&advisory, false).await,
        Ok(payload::Plan::Full)
    );

    let mut url_drift = phase1();
    url_drift.record = url_drift.record.with_tracked_sources(vec![source(
        TrackedSourceRole::AddonPayload,
        "https://old.invalid",
    )]);
    assert_eq!(
        payload::plan(&url_drift, false).await,
        Ok(payload::Plan::Full)
    );

    let mut filename_drift = phase1();
    filename_drift.target.addon_file = "Other.addon64".to_owned();
    assert_eq!(
        payload::plan(&filename_drift, false).await,
        Ok(payload::Plan::Full)
    );
}

#[test]
fn head_validator_matrix_is_closed_and_uses_cache_fallback() {
    assert_eq!(
        payload::classify_head(Some("etag"), Some("etag")),
        payload::Plan::Preserve
    );
    assert_eq!(
        payload::classify_head(Some("last-modified"), Some("last-modified")),
        payload::Plan::Preserve
    );
    assert_eq!(
        payload::classify_head(Some("old"), Some("new")),
        payload::Plan::Full
    );
    assert_eq!(
        payload::classify_head(Some("stored"), None),
        payload::Plan::Full
    );
    assert_eq!(
        payload::classify_head(None, Some("returned")),
        payload::Plan::Full
    );
}

#[test]
fn identical_download_digest_stays_a_full_non_advisory_source() {
    let mut tracked = vec![source(
        TrackedSourceRole::AddonPayload,
        "https://old.invalid",
    )];
    let payload = crate::addons::luma::fetch::types::LumaPayload {
        files: Vec::new(),
        main_addon_rel: "Luma.addon64".to_owned(),
        zip_digest: "digest".to_owned(),
        etag: Some("fresh-etag".to_owned()),
        last_modified: Some("fresh-date".to_owned()),
        build_number: None,
    };
    sources::replace_payload(&mut tracked, "Luma-Game.zip", &payload).expect("source");
    assert_eq!(
        payload::classify_head(Some("old-digest"), Some("fresh-validator")),
        payload::Plan::Full
    );
    assert_eq!(tracked[0].digest(), "digest");
    assert_eq!(tracked[0].etag(), Some("fresh-etag"));
    assert_eq!(tracked[0].last_modified(), Some("fresh-date"));
    assert!(!tracked[0].is_advisory());
}

#[test]
fn required_host_replacement_rejects_duplicate_provenance_before_fetch() {
    let mut phase = phase1();
    phase.host_replacement_required = true;
    phase.record = phase
        .record
        .with_tracked_source(source(TrackedSourceRole::HostBinary, "https://one.invalid"))
        .with_tracked_source(source(TrackedSourceRole::HostBinary, "https://two.invalid"));
    assert!(host::plan(&phase).is_err());
}

fn requirement() -> LumaExternalRequirement {
    LumaExternalRequirement::Dgvoodoo2 {
        version: "2.79.3".to_owned(),
        accepted_detected_apis: Vec::new(),
        reshade_proxy_dll: "dxgi.dll".to_owned(),
        source: ManagedArchiveSource {
            url: "https://dgvoodoo.invalid/archive.zip".to_owned(),
            sha256: "a".repeat(64),
            size: 1,
        },
        install_map: Vec::new(),
        config_file: "dgVoodoo.conf".to_owned(),
        config: Vec::new(),
    }
}

#[test]
fn dgvoodoo_plan_matrix_keeps_full_replacement_deferred() {
    let mut preserve = phase1();
    preserve.target.external_requirement = Some(requirement());
    assert_eq!(
        dgvoodoo::plan(&preserve, false),
        Ok(dgvoodoo::Plan::Preserve)
    );

    let mut replace = phase1();
    replace.target.external_requirement = Some(requirement());
    replace.dgvoodoo = DgVoodooLocalDecision::Replace { config_owned: true };
    replace.record = replace.record.with_tracked_source(source(
        TrackedSourceRole::DgVoodooWrapper,
        "https://old.invalid/dg.zip",
    ));
    assert_eq!(dgvoodoo::plan(&replace, false), Ok(dgvoodoo::Plan::Replace));

    let mut deferred = replace;
    deferred.dgvoodoo = DgVoodooLocalDecision::ReplaceOnFull { config_owned: true };
    assert_eq!(
        dgvoodoo::plan(&deferred, false),
        Ok(dgvoodoo::Plan::Preserve)
    );
    assert_eq!(dgvoodoo::plan(&deferred, true), Ok(dgvoodoo::Plan::Replace));

    let mut remove = phase1();
    remove.dgvoodoo = DgVoodooLocalDecision::Remove;
    assert_eq!(dgvoodoo::plan(&remove, true), Ok(dgvoodoo::Plan::Remove));
}

#[test]
fn dgvoodoo_missing_inputs_and_duplicate_sources_fail_closed() {
    let mut missing_source = phase1();
    missing_source.target.external_requirement = Some(requirement());
    missing_source.dgvoodoo = DgVoodooLocalDecision::Replace { config_owned: true };
    assert!(dgvoodoo::plan(&missing_source, true).is_err());

    let mut missing_requirement = phase1();
    missing_requirement.dgvoodoo = DgVoodooLocalDecision::Replace { config_owned: true };
    missing_requirement.record = missing_requirement.record.with_tracked_source(source(
        TrackedSourceRole::DgVoodooWrapper,
        "https://old.invalid/dg.zip",
    ));
    assert!(dgvoodoo::plan(&missing_requirement, true).is_err());

    let mut duplicate = missing_requirement;
    duplicate.record = duplicate.record.with_tracked_source(source(
        TrackedSourceRole::DgVoodooWrapper,
        "https://other.invalid/dg.zip",
    ));
    assert!(dgvoodoo::plan(&duplicate, true).is_err());
}

#[test]
fn host_insertion_propagates_missing_payload_instead_of_falling_back() {
    let mut sources = vec![source(TrackedSourceRole::DlssFix, "https://dlss.invalid")];
    let error = sources::replace_host(
        &mut sources,
        source(TrackedSourceRole::HostBinary, "https://host.invalid"),
    )
    .expect_err("missing payload is not a valid insertion anchor");
    assert!(matches!(error, crate::ServiceError::InvalidInput(_)));
}

#[cfg(windows)]
#[test]
fn non_utf8_dependency_path_is_rejected_without_replacement_text() {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use std::path::PathBuf;

    let path = PathBuf::from(OsString::from_wide(&[0xd800]));
    let error = sources::convert_dependency_paths(&[path]).expect_err("non-UTF-8 path");
    assert!(matches!(error, crate::ServiceError::InvalidInput(_)));
}
