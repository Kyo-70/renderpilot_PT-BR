//! Shared test-only Xiph component fixtures.

use renderpilot_domain::{
    Architecture, ComponentFile, ComponentId, ComponentKind, GameId, GameInstallation,
    LibraryComponent, LibraryTechnology, PathRef, PeCompatibilityProfile, PeExportSet,
    PeImportProfile, PeImportSet, Sha256Hash, Swappability,
};

pub(crate) fn split_component(game: &GameInstallation) -> LibraryComponent {
    split_component_at(game.id().as_str(), game.install_path().as_str())
}

pub(crate) fn complete_split_component(game: &GameInstallation) -> LibraryComponent {
    let root = game.install_path().as_str();
    [
        xiph_file(
            &format!("{root}/Plugin/vorbisfile.dll"),
            &["vorbis.dll", "ogg.dll"],
            'a',
        ),
        xiph_file(&format!("{root}/Codec/vorbis.dll"), &["ogg.dll"], 'b'),
        xiph_file(&format!("{root}/Container/ogg.dll"), &[], 'c'),
    ]
    .into_iter()
    .fold(
        LibraryComponent::new(
            ComponentId::new(format!("component:{}:split", game.id())).expect("component"),
            game.id().clone(),
            ComponentKind::NativeLibrary,
            LibraryTechnology::XiphVorbis,
            Swappability::BundleOnly,
        ),
        LibraryComponent::with_file,
    )
}

pub(crate) fn vendor_xiph_files(root: &str) -> Vec<ComponentFile> {
    vec![
        xiph_file(
            &format!("{root}/Plugin/vorbisfile_vs2010_x64_rwdi.dll"),
            &["vorbis_vs2010_x64_rwdi.dll", "ogg_vs2010_x64_rwdi.dll"],
            'a',
        ),
        xiph_file(
            &format!("{root}/Codec/vorbis_vs2010_x64_rwdi.dll"),
            &["ogg_vs2010_x64_rwdi.dll"],
            'b',
        ),
        xiph_file(
            &format!("{root}/Container/ogg_vs2010_x64_rwdi.dll"),
            &[],
            'c',
        ),
    ]
}

pub(crate) fn split_component_at(game_id: &str, root: &str) -> LibraryComponent {
    [
        format!("{root}/Plugin/vorbisfile.dll"),
        format!("{root}/Codec/vorbis.dll"),
        format!("{root}/Container/ogg.dll"),
    ]
    .into_iter()
    .enumerate()
    .fold(
        LibraryComponent::new(
            ComponentId::new(format!("component:{game_id}:split")).expect("component id"),
            GameId::new(game_id).expect("game id"),
            ComponentKind::NativeLibrary,
            LibraryTechnology::XiphVorbis,
            Swappability::BundleOnly,
        ),
        |component, (index, path)| {
            component.with_file(file(
                &path,
                char::from(b'a' + u8::try_from(index).unwrap_or(0)),
            ))
        },
    )
}

pub(crate) fn file(path: &str, hash: char) -> ComponentFile {
    ComponentFile::new(PathRef::new(path).expect("path")).with_sha256(
        Sha256Hash::new(std::iter::repeat_n(hash, 64).collect::<String>()).expect("hash"),
    )
}

fn xiph_file(path: &str, imports: &[&str], hash: char) -> ComponentFile {
    file(path, hash).with_pe_compatibility(
        PeCompatibilityProfile::new(
            Architecture::X64,
            PeExportSet::from_observed_names(vec!["xiph_export".to_owned()]).expect("exports"),
        )
        .with_imports(PeImportProfile {
            regular: PeImportSet::from_observed_names(
                imports.iter().map(|name| (*name).to_owned()).collect(),
            )
            .expect("imports"),
            delay: PeImportSet::default(),
        }),
    )
}
