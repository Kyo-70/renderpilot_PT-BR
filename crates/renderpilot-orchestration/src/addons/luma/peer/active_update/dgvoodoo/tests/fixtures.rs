use std::path::Path;

use renderpilot_domain::{AddonKind, GameId, InstalledAddon, PathRef, TrackedSource};

use crate::addons::luma::{
    dgvoodoo::{PreparedDgVoodoo, PreparedDgVoodooFile},
    peer::{
        active_update::{
            dgvoodoo::project_dgvoodoo,
            model::{DgVoodooProjection, LumaActiveUpdateDgVoodooInput},
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

pub(super) fn record(root: &Path, created: &[&str], backed: &[&str]) -> InstalledAddon {
    let game_id = GameId::new(format!(
        "manual:luma-active-dgvoodoo:{}",
        ulid::Ulid::generate()
    ))
    .expect("game id");
    let mut result = InstalledAddon::new(game_id, AddonKind::Luma, path(root, "luma.addon64"));
    for relative in created {
        result = result.with_created_file(path(root, relative));
    }
    for relative in backed {
        result = result.with_backed_up_file(path(root, relative));
    }
    result
}

pub(super) fn prepared(files: &[(&str, &[u8])]) -> PreparedDgVoodoo {
    PreparedDgVoodoo {
        version: "2.82".to_owned(),
        files: files
            .iter()
            .map(|(dest, bytes)| PreparedDgVoodooFile {
                dest: (*dest).to_owned(),
                bytes: (*bytes).to_vec(),
            })
            .collect(),
        config_file: "dgVoodoo.conf".to_owned(),
        config_default: "[General]\r\nOutputAPI = d3d11\r\n".to_owned(),
        config_sections: vec![crate::addons::engine::IniSection {
            name: "General".to_owned(),
            keys: vec![("OutputAPI".to_owned(), "d3d11".to_owned())],
        }],
        source_url: "https://example.test/dgvoodoo.zip".to_owned(),
        source_etag: Some("etag".to_owned()),
        source_last_modified: Some("today".to_owned()),
        archive_digest: "archive-digest".to_owned(),
    }
}

pub(super) fn run(
    root: &Path,
    before: &InstalledAddon,
    input: LumaActiveUpdateDgVoodooInput,
    dependencies: &[PathRef],
    sources: &[TrackedSource],
) -> Result<
    (DgVoodooProjection, Option<LumaPeerEffects>),
    crate::addons::luma::peer::active_update::error::LumaActiveUpdateError,
> {
    let mut accumulator = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);
    let projection = project_dgvoodoo(
        before,
        &authority(root),
        input,
        dependencies,
        sources,
        &mut accumulator,
    )?;
    let effects = accumulator.finalize().expect("effect finalization");
    Ok((projection, effects))
}

pub(super) fn source(prepared: &PreparedDgVoodoo) -> TrackedSource {
    prepared.tracked_source()
}
