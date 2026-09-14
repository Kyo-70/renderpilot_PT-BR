//! Candidate visibility and automatic-selection policy for Xiph bundles.

use std::collections::BTreeMap;

use renderpilot_domain::{
    Architecture, ArtifactId, ArtifactMetadata, ArtifactTrustLevel, ComponentFile, ComponentId,
    ComponentKind, GameId, LibraryArtifact, LibraryComponent, LibraryTechnology, PackageVersion,
    PathRef, PeCompatibilityProfile, PeExportSet, PeImportProfile, PeImportSet, RuntimeTarget,
    Sha256Hash, Swappability, Version,
};

use super::{active_catalog_for, test_catalog_receipt};
use crate::{CandidateContext, find_replacement_candidate_selection};

#[test]
fn gothic_cross_directory_bundle_is_visible_but_manual_only() {
    let component = gothic_component("component:gothic:xiph", false);
    let artifact = canonical_artifact();
    let context = CandidateContext::new(
        std::collections::HashSet::new(),
        active_catalog_for(std::slice::from_ref(&artifact)),
    );

    let selection = find_replacement_candidate_selection(
        std::slice::from_ref(&component),
        std::slice::from_ref(&artifact),
        &context,
    );
    assert_eq!(selection.groups().len(), 1);
    assert_eq!(selection.groups()[0].candidates().len(), 1);
    assert_eq!(
        selection.groups()[0].automatic_candidate_artifact_id(),
        None,
        "a cross-directory Xiph candidate is visible for explicit manual selection only"
    );

    let same_directory = gothic_component("component:gothic:xiph-same-directory", true);
    let same_directory_selection = find_replacement_candidate_selection(
        std::slice::from_ref(&same_directory),
        std::slice::from_ref(&artifact),
        &context,
    );
    assert_eq!(same_directory_selection.groups().len(), 1);
    assert_eq!(
        same_directory_selection.groups()[0].automatic_candidate_artifact_id(),
        Some(&ArtifactId::new("artifact:xiph-canonical").expect("artifact id")),
        "same-directory Xiph keeps the existing automatic-selection policy"
    );
}

fn gothic_component(component_id: &str, same_directory: bool) -> LibraryComponent {
    let vorbis_root = "C:/Games/Gothic/Engine/Binaries/ThirdParty/Vorbis/Win64";
    let ogg_root = if same_directory {
        vorbis_root
    } else {
        "C:/Games/Gothic/Engine/Binaries/ThirdParty/Ogg/Win64"
    };
    [
        (
            format!("{vorbis_root}/libvorbisfile_64.dll"),
            vec!["libvorbis_64.dll", "libogg_64.dll"],
            "ov_open",
            1,
            "1.3.5",
        ),
        (
            format!("{vorbis_root}/libvorbis_64.dll"),
            vec!["libogg_64.dll"],
            "vorbis_info_init",
            2,
            "1.3.5",
        ),
        (
            format!("{ogg_root}/libogg_64.dll"),
            Vec::new(),
            "ogg_sync_init",
            3,
            "1.3.6",
        ),
    ]
    .into_iter()
    .map(|(path, imports, export, hash, version)| {
        xiph_member(&path, &imports, export, hash)
            .with_version(Version::parse(version).expect("version"))
    })
    .fold(
        LibraryComponent::new(
            ComponentId::new(component_id).expect("component id"),
            GameId::new("game:gothic").expect("game id"),
            ComponentKind::NativeLibrary,
            LibraryTechnology::XiphVorbis,
            Swappability::BundleOnly,
        ),
        LibraryComponent::with_file,
    )
}

fn canonical_artifact() -> LibraryArtifact {
    LibraryArtifact::new(
        ArtifactId::new("artifact:xiph-canonical").expect("artifact id"),
        LibraryTechnology::XiphVorbis,
        "vorbisfile.dll",
        vec![
            xiph_member(
                "C:/Library/vorbisfile.dll",
                &["vorbis.dll", "ogg.dll"],
                "ov_open",
                16,
            ),
            xiph_member(
                "C:/Library/vorbis.dll",
                &["ogg.dll"],
                "vorbis_info_init",
                17,
            ),
            xiph_member("C:/Library/ogg.dll", &[], "ogg_sync_init", 18),
        ],
        ArtifactTrustLevel::CatalogDownloaded,
    )
    .expect("artifact")
    .with_metadata(
        ArtifactMetadata::default()
            .with_runtime_target(RuntimeTarget::new(Architecture::X64))
            .with_catalog_package_receipt({
                let mut receipt = test_catalog_receipt("xiph-vorbis", "xiph_vorbis", "1.3.7", None);
                receipt.release.components = BTreeMap::from([
                    (
                        "ogg".to_owned(),
                        PackageVersion::parse("1.3.7").expect("Ogg version"),
                    ),
                    (
                        "vorbis".to_owned(),
                        PackageVersion::parse("1.3.7").expect("Vorbis version"),
                    ),
                ]);
                receipt
            }),
    )
}

fn xiph_member(path: &str, imports: &[&str], export: &str, hash: u8) -> ComponentFile {
    ComponentFile::new(PathRef::new(path).expect("path"))
        .with_sha256(Sha256Hash::new(format!("{hash:02x}").repeat(32)).expect("hash"))
        .with_pe_compatibility(
            PeCompatibilityProfile::new(
                Architecture::X64,
                PeExportSet::from_observed_names(vec![export.to_owned()]).expect("exports"),
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
