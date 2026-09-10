use std::path::Path;

use renderpilot_domain::{
    AddonKind, GameId, InstalledAddon, PathRef, TrackedSource, TrackedSourceRole,
};
use tempfile::TempDir;

use crate::addons::luma::{
    fetch::types::{LumaPayload, LumaPayloadFile},
    peer::{
        active_update::{
            error::LumaActiveUpdateError,
            model::{LumaActiveUpdatePayloadInput, PayloadProjection},
        },
        effects::{LumaPeerEffectAccumulator, LumaPeerEffects, LumaPeerOperationOrder},
        root_authority::LumaPeerRootAuthority,
    },
};

pub(super) fn path(root: &Path, relative: &str) -> PathRef {
    PathRef::new(root.join(relative).to_string_lossy().into_owned()).expect("path")
}

pub(super) fn authority(root: &Path) -> LumaPeerRootAuthority {
    LumaPeerRootAuthority::resolve(root, &root.join("dxgi.dll")).expect("authority")
}

pub(super) fn record(
    root: &Path,
    main: &str,
    created: &[&str],
    backed_up: &[&str],
) -> InstalledAddon {
    let game_id =
        GameId::new(format!("manual:luma-payload:{}", ulid::Ulid::generate())).expect("game id");
    let mut record = InstalledAddon::new(game_id, AddonKind::Luma, path(root, main));
    for relative in created {
        record = record.with_created_file(path(root, relative));
    }
    for relative in backed_up {
        record = record.with_backed_up_file(path(root, relative));
    }
    record
}

pub(super) fn payload(main: &str, files: &[(&str, &[u8])]) -> LumaPayload {
    LumaPayload {
        files: files
            .iter()
            .map(|(relative, bytes)| LumaPayloadFile {
                relative_path: (*relative).to_owned(),
                bytes: (*bytes).to_vec(),
            })
            .collect(),
        main_addon_rel: main.to_owned(),
        zip_digest: "zip-digest".to_owned(),
        etag: Some("etag".to_owned()),
        last_modified: Some("last-modified".to_owned()),
        build_number: None,
    }
}

pub(super) fn source(advisory: bool) -> TrackedSource {
    let source = TrackedSource::new(
        TrackedSourceRole::AddonPayload,
        "https://example.invalid/luma.zip",
        Some("etag".to_owned()),
        "zip-digest",
    )
    .with_last_modified(Some("last-modified".to_owned()));
    if advisory {
        source.with_advisory()
    } else {
        source
    }
}

pub(super) fn run(
    root: &Path,
    before: &InstalledAddon,
    payload: LumaPayload,
    dependencies: &[PathRef],
    sources: &[TrackedSource],
) -> Result<(PayloadProjection, LumaPeerEffects), LumaActiveUpdateError> {
    let (projection, effects) = run_optional(root, before, payload, dependencies, sources)?;
    Ok((
        projection,
        effects.expect("scenario emits a generic effect"),
    ))
}

pub(super) fn run_optional(
    root: &Path,
    before: &InstalledAddon,
    payload: LumaPayload,
    dependencies: &[PathRef],
    sources: &[TrackedSource],
) -> Result<(PayloadProjection, Option<LumaPeerEffects>), LumaActiveUpdateError> {
    let mut accumulator = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);
    let projection = super::super::project_payload(
        before,
        &authority(root),
        LumaActiveUpdatePayloadInput::Full(payload),
        dependencies,
        sources,
        &mut accumulator,
    )?;
    Ok((
        projection,
        accumulator.finalize().expect("effect finalization"),
    ))
}

pub(super) fn run_preserve(
    root: &Path,
    before: &InstalledAddon,
) -> Result<(PayloadProjection, Option<LumaPeerEffects>), LumaActiveUpdateError> {
    let mut accumulator = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);
    let projection = super::super::project_payload(
        before,
        &authority(root),
        LumaActiveUpdatePayloadInput::Preserve,
        &[],
        &[],
        &mut accumulator,
    )?;
    Ok((
        projection,
        accumulator.finalize().expect("effect finalization"),
    ))
}

pub(super) fn run_invalid(
    root: &Path,
    before: &InstalledAddon,
    payload: LumaPayload,
    dependencies: &[PathRef],
    sources: &[TrackedSource],
) -> Result<(PayloadProjection, Option<LumaPeerEffects>), LumaActiveUpdateError> {
    let mut accumulator = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);
    let result = super::super::project_payload(
        before,
        &authority(root),
        LumaActiveUpdatePayloadInput::Full(payload),
        dependencies,
        sources,
        &mut accumulator,
    );
    let effects = accumulator.finalize().expect("effect finalization");
    result.map(|projection| (projection, effects))
}

pub(super) fn temp() -> TempDir {
    tempfile::tempdir().expect("root")
}
