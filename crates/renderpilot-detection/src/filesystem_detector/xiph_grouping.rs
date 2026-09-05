//! Partitions colocated Xiph DLLs by their observed import graph.
//!
//! Canonical Xiph layouts retain their established naming-profile identity.
//! Vendor-suffixed runtime layouts deliberately use only a validated semantic
//! topology: basenames are opaque loader aliases, never an identity fallback.

use std::collections::{HashMap, HashSet};

use renderpilot_domain::{LibraryTechnology, normalized_path_key, xiph};

use super::DetectedLibraryFile;

/// Xiph facts used by the generic directory grouper.
///
/// `discriminators` deliberately retains the established per-directory
/// behavior. `closure_keys` only describes an independently authenticated
/// cross-directory deployment; it is never a basename-derived fallback.
#[derive(Debug, Default)]
pub(super) struct GroupingFacts {
    pub(super) discriminators: HashMap<usize, String>,
    pub(super) closure_keys: HashMap<usize, String>,
}

pub(super) fn grouping_facts(libraries: &[DetectedLibraryFile]) -> GroupingFacts {
    let mut facts = GroupingFacts {
        discriminators: discriminators(libraries),
        closure_keys: HashMap::new(),
    };
    assign_cross_directory_closures(libraries, &mut facts);
    facts
}

/// Existing, strictly local discriminator behavior. Keep this separate from
/// cross-directory closure discovery so every old same-directory result stays
/// byte-for-byte stable.
fn discriminators(libraries: &[DetectedLibraryFile]) -> HashMap<usize, String> {
    let mut by_directory = HashMap::<String, Vec<usize>>::new();
    for (index, library) in libraries.iter().enumerate() {
        if library.technology() == LibraryTechnology::XiphVorbis {
            by_directory
                .entry(parent_directory(library))
                .or_default()
                .push(index);
        }
    }

    let mut result = HashMap::new();
    for indices in by_directory.into_values() {
        assign_directory_discriminators(libraries, &indices, &mut result);
    }
    result
}

/// Finds only closures demonstrated by a bounded static import graph.
///
/// This is intentionally not a Windows loader-resolution proof: it proves an
/// exact, unambiguous association among the DLLs observed in this scan.
fn assign_cross_directory_closures(libraries: &[DetectedLibraryFile], facts: &mut GroupingFacts) {
    let xiph_indices = libraries
        .iter()
        .enumerate()
        .filter_map(|(index, file)| {
            (file.technology() == LibraryTechnology::XiphVorbis).then_some(index)
        })
        .collect::<Vec<_>>();
    if xiph_indices.len() < 2 {
        return;
    }

    let mut by_directory_and_name = HashMap::<String, HashMap<String, Vec<usize>>>::new();
    let mut by_name = HashMap::<String, Vec<usize>>::new();
    for index in &xiph_indices {
        let file = &libraries[*index];
        let name = file.file_name().to_ascii_lowercase();
        by_directory_and_name
            .entry(normalized_parent_directory(file))
            .or_default()
            .entry(name.clone())
            .or_default()
            .push(*index);
        by_name.entry(name).or_default().push(*index);
    }

    let mut position = HashMap::new();
    for (local, global) in xiph_indices.iter().copied().enumerate() {
        position.insert(global, local);
    }
    let mut disjoint = DisjointSet::new(xiph_indices.len());
    for global_index in &xiph_indices {
        let Some(imports) = libraries[*global_index]
            .pe_compatibility()
            .and_then(|profile| profile.imports())
        else {
            continue;
        };
        let directory = normalized_parent_directory(&libraries[*global_index]);
        for imported in imports.regular.names().iter().chain(imports.delay.names()) {
            if !matches!(xiph::parse_runtime_file_name(imported), Ok(Some(_))) {
                continue;
            }
            let name = imported.to_ascii_lowercase();
            let local = by_directory_and_name
                .get(&directory)
                .and_then(|files_by_name| files_by_name.get(&name));
            let resolved = match local {
                Some(matches) if matches.len() == 1 => Some(matches[0]),
                Some(_) => None,
                None => by_name
                    .get(&name)
                    .filter(|matches| matches.len() == 1)
                    .map(|matches| matches[0]),
            };
            if let Some(imported_index) = resolved {
                disjoint.union(position[global_index], position[&imported_index]);
            }
        }
    }

    for component in disjoint_components(&xiph_indices, &mut disjoint) {
        let parents = component
            .iter()
            .map(|index| normalized_parent_directory(&libraries[*index]))
            .collect::<HashSet<_>>();
        if parents.len() < 2
            || !cross_closure_is_authenticated(
                libraries,
                &component,
                &by_directory_and_name,
                &by_name,
            )
        {
            continue;
        }
        // The descriptor is internal-only. It replaces the directory key while
        // grouping; durable identity is constructed later from game-relative
        // exact paths, never from this absolute-path key.
        let mut paths = component
            .iter()
            .map(|index| libraries[*index].file_path().as_str().to_ascii_lowercase())
            .collect::<Vec<_>>();
        paths.sort_unstable();
        let closure_key = format!("xiph-closure:{}", paths.join("\0"));
        let files = component
            .iter()
            .map(|index| libraries[*index].component_file())
            .collect::<Vec<_>>();
        let Some(layout) = xiph::detect_layout(&files) else {
            continue;
        };
        let discriminator = if component.iter().any(|index| {
            xiph::parse_runtime_file_name(libraries[*index].file_name())
                .ok()
                .flatten()
                .is_some_and(|name| name.is_vendor())
        }) {
            layout.topology().vendor_discriminator()
        } else {
            layout.naming_profile().as_slug().to_owned()
        };
        for index in component {
            facts.closure_keys.insert(index, closure_key.clone());
            facts.discriminators.insert(index, discriminator.clone());
        }
    }
}

fn cross_closure_is_authenticated(
    libraries: &[DetectedLibraryFile],
    component: &[usize],
    by_directory_and_name: &HashMap<String, HashMap<String, Vec<usize>>>,
    by_name: &HashMap<String, Vec<usize>>,
) -> bool {
    if !has_complete_compatible_pe(libraries, component) {
        return false;
    }
    let component_set = component.iter().copied().collect::<HashSet<_>>();
    let mut members = HashSet::new();
    for index in component {
        let file = &libraries[*index];
        let Ok(Some(runtime)) = xiph::parse_runtime_file_name(file.file_name()) else {
            return false;
        };
        if !members.insert(runtime.member()) {
            return false;
        }
        let Some(imports) = file
            .pe_compatibility()
            .and_then(|profile| profile.imports())
        else {
            return false;
        };
        let directory = normalized_parent_directory(file);
        for imported in imports.regular.names().iter().chain(imports.delay.names()) {
            let Ok(parsed) = xiph::parse_runtime_file_name(imported) else {
                return false;
            };
            let Some(_) = parsed else {
                continue;
            };
            let name = imported.to_ascii_lowercase();
            let local = by_directory_and_name
                .get(&directory)
                .and_then(|files_by_name| files_by_name.get(&name));
            let resolved = match local {
                Some(matches) if matches.len() == 1 => Some(matches[0]),
                Some(_) => None,
                None => by_name
                    .get(&name)
                    .filter(|matches| matches.len() == 1)
                    .map(|matches| matches[0]),
            };
            if !resolved.is_some_and(|candidate| component_set.contains(&candidate)) {
                return false;
            }
        }
    }
    let files = component
        .iter()
        .map(|index| libraries[*index].component_file())
        .collect::<Vec<_>>();
    xiph::detect_layout(&files).is_some()
}

fn assign_directory_discriminators(
    libraries: &[DetectedLibraryFile],
    indices: &[usize],
    result: &mut HashMap<usize, String>,
) {
    let local_names = local_names(libraries, indices);
    let mut disjoint = DisjointSet::new(indices.len());
    for (local_index, global_index) in indices.iter().copied().enumerate() {
        let Some(imports) = libraries[global_index]
            .pe_compatibility()
            .and_then(|profile| profile.imports())
        else {
            continue;
        };
        for imported in imports.regular.names().iter().chain(imports.delay.names()) {
            let Some(matches) = local_names.get(imported) else {
                continue;
            };
            if matches.len() == 1 {
                disjoint.union(local_index, matches[0]);
            }
        }
    }

    let components = disjoint_components(indices, &mut disjoint);
    let canonical_components = components
        .iter()
        .filter(|component| !contains_vendor_runtime_name(libraries, component))
        .map(Vec::as_slice)
        .collect::<Vec<_>>();
    assign_canonical_discriminators(libraries, &canonical_components, result);

    let vendor_components = components
        .iter()
        .filter_map(|component| {
            vendor_discriminator(libraries, component, &local_names)
                .map(|discriminator| (component, discriminator))
        })
        .collect::<Vec<_>>();
    let mut counts = HashMap::<&str, usize>::new();
    for (_, discriminator) in &vendor_components {
        *counts.entry(discriminator.as_str()).or_default() += 1;
    }
    for (component, discriminator) in &vendor_components {
        // Two different local closures with the same topology cannot receive
        // distinct stable vendor IDs. Suppress both instead of reintroducing a
        // basename-derived fallback.
        if counts.get(discriminator.as_str()).copied() != Some(1) {
            continue;
        }
        for global_index in *component {
            result.insert(*global_index, discriminator.clone());
        }
    }
}

fn local_names(
    libraries: &[DetectedLibraryFile],
    indices: &[usize],
) -> HashMap<String, Vec<usize>> {
    let mut local_names = HashMap::<String, Vec<usize>>::new();
    for (local_index, global_index) in indices.iter().copied().enumerate() {
        local_names
            .entry(libraries[global_index].file_name().to_ascii_lowercase())
            .or_default()
            .push(local_index);
    }
    local_names
}

fn disjoint_components(indices: &[usize], disjoint: &mut DisjointSet) -> Vec<Vec<usize>> {
    let mut components = Vec::<Vec<usize>>::new();
    let mut component_by_root = HashMap::new();
    for (local_index, global_index) in indices.iter().copied().enumerate() {
        let root = disjoint.root(local_index);
        let component_index = *component_by_root.entry(root).or_insert_with(|| {
            components.push(Vec::new());
            components.len() - 1
        });
        components[component_index].push(global_index);
    }
    components
}

fn assign_canonical_discriminators(
    libraries: &[DetectedLibraryFile],
    components: &[&[usize]],
    result: &mut HashMap<usize, String>,
) {
    let bases = components
        .iter()
        .map(|component| discriminator_base(libraries, component))
        .collect::<Vec<_>>();
    let mut base_counts = HashMap::<&str, usize>::new();
    for base in &bases {
        *base_counts.entry(base.as_str()).or_default() += 1;
    }
    for (component, base) in components.iter().zip(&bases) {
        let discriminator = if base_counts.get(base.as_str()).copied().unwrap_or_default() == 1 {
            base.clone()
        } else {
            format!(
                "{base}-{:016x}",
                stable_canonical_name_hash(libraries, component)
            )
        };
        for global_index in *component {
            result.insert(*global_index, discriminator.clone());
        }
    }
}

fn vendor_discriminator(
    libraries: &[DetectedLibraryFile],
    component: &[usize],
    local_names: &HashMap<String, Vec<usize>>,
) -> Option<String> {
    if !contains_vendor_runtime_name(libraries, component)
        || !has_complete_compatible_pe(libraries, component)
        || !xiph_imports_resolve_uniquely(libraries, component, local_names)
    {
        return None;
    }

    let files = component
        .iter()
        .map(|index| libraries[*index].component_file())
        .collect::<Vec<_>>();
    let layout = xiph::detect_layout(&files)?;
    Some(layout.topology().vendor_discriminator())
}

fn contains_vendor_runtime_name(libraries: &[DetectedLibraryFile], component: &[usize]) -> bool {
    component.iter().any(|index| {
        xiph::parse_runtime_file_name(libraries[*index].file_name())
            .ok()
            .flatten()
            .is_some_and(|runtime_name| runtime_name.is_vendor())
    })
}

fn has_complete_compatible_pe(libraries: &[DetectedLibraryFile], component: &[usize]) -> bool {
    let mut architectures = HashSet::new();
    for index in component {
        let Some(profile) = libraries[*index].pe_compatibility() else {
            return false;
        };
        // `PeCompatibilityProfile` only exists after architecture, complete
        // named exports, and strict regular/delay import parsing all succeed.
        if profile.imports().is_none() {
            return false;
        }
        architectures.insert(profile.architecture());
    }
    architectures.len() == 1
}

fn xiph_imports_resolve_uniquely(
    libraries: &[DetectedLibraryFile],
    component: &[usize],
    local_names: &HashMap<String, Vec<usize>>,
) -> bool {
    component.iter().all(|index| {
        let Some(imports) = libraries[*index]
            .pe_compatibility()
            .and_then(|profile| profile.imports())
        else {
            return false;
        };
        imports
            .regular
            .names()
            .iter()
            .chain(imports.delay.names())
            .all(|imported| match xiph::parse_runtime_file_name(imported) {
                Ok(Some(_)) => local_names
                    .get(imported)
                    .is_some_and(|matches| matches.len() == 1),
                Ok(None) => true,
                Err(_) => false,
            })
    })
}

fn discriminator_base(libraries: &[DetectedLibraryFile], component: &[usize]) -> String {
    xiph::XiphNamingProfile::from_styles(
        component
            .iter()
            .filter_map(|index| xiph::classify_file_name(libraries[*index].file_name()))
            .map(|(_, style)| style),
    )
    .as_slug()
    .to_owned()
}

fn stable_canonical_name_hash(libraries: &[DetectedLibraryFile], component: &[usize]) -> u64 {
    let mut names = component
        .iter()
        .map(|index| libraries[*index].file_name().to_ascii_lowercase())
        .collect::<Vec<_>>();
    names.sort_unstable();
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in names.join("\0").bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn parent_directory(library: &DetectedLibraryFile) -> String {
    library.file_path().parent().unwrap_or_default().to_owned()
}

fn normalized_parent_directory(library: &DetectedLibraryFile) -> String {
    normalized_path_key(&parent_directory(library))
}

struct DisjointSet {
    parents: Vec<usize>,
}

impl DisjointSet {
    fn new(len: usize) -> Self {
        Self {
            parents: (0..len).collect(),
        }
    }

    fn root(&mut self, value: usize) -> usize {
        if self.parents[value] != value {
            self.parents[value] = self.root(self.parents[value]);
        }
        self.parents[value]
    }

    fn union(&mut self, left: usize, right: usize) {
        let left = self.root(left);
        let right = self.root(right);
        if left != right {
            self.parents[right] = left;
        }
    }
}

#[cfg(test)]
mod tests {
    use renderpilot_domain::{
        Architecture, ComponentKind, GameId, GameIdentity, GameInstallation, GameRuntime, Launcher,
        PathRef, PeCompatibilityProfile, PeExportSet, PeImportProfile, PeImportSet, Platform,
        Sha256Hash, Swappability,
    };

    use crate::{DetectionConfidence, VersionDetectionStatus};

    use super::*;

    const DIDE_DISCRIMINATOR: &str =
        "vendor-topology-v1-c792fb369a519e40bb3bda22747d014575a6d1139108e0d54dc2d4d7b7597734";

    #[test]
    fn validated_dide_vendor_closure_uses_semantic_topology_discriminator() {
        let libraries = dide("_vs2010_x64_rwdi");

        let discriminators = discriminators(&libraries);

        assert_eq!(discriminators.len(), 3);
        assert!(
            discriminators
                .values()
                .all(|value| value == DIDE_DISCRIMINATOR)
        );

        let components =
            super::super::group_into_components(&game(), &libraries).expect("vendor grouping");
        assert_eq!(components.len(), 1);
        assert_eq!(
            components[0].id().as_str(),
            format!("component:manual:xiph-grouping-test:xiph_vorbis:C:/game:{DIDE_DISCRIMINATOR}")
        );
        assert!(components[0].id().as_str().contains(DIDE_DISCRIMINATOR));
        assert_eq!(components[0].swappability(), Swappability::BundleOnly);
    }

    #[test]
    fn vendor_versions_are_opaque_to_the_topology_identity() {
        for suffix in ["_vs2008_x64_rwdi", "_vs2012_x64_rwdi"] {
            let discriminators = discriminators(&dide(suffix));
            assert!(
                discriminators
                    .values()
                    .all(|value| value == DIDE_DISCRIMINATOR)
            );
        }
    }

    #[test]
    fn vendor_closure_without_complete_pe_facts_is_not_authenticated() {
        let mut libraries = dide("_vs2010_x64_rwdi");
        libraries[1].pe_compatibility = None;

        assert!(discriminators(&libraries).is_empty());
    }

    #[test]
    fn invalid_vendor_import_graph_is_not_authenticated() {
        let mut libraries = dide("_vs2010_x64_rwdi");
        libraries[0].pe_compatibility = Some(profile(
            Architecture::X64,
            &[
                "vorbis_vs2010_x64_rwdi.dll".to_owned(),
                "ogg_vs2010_x64_rwdi.dll".to_owned(),
            ],
        ));
        libraries[1].pe_compatibility = Some(profile(
            Architecture::X64,
            &["vorbisfile_vs2010_x64_rwdi.dll".to_owned()],
        ));

        assert!(discriminators(&libraries).is_empty());
    }

    #[test]
    fn same_directory_duplicate_vendor_topologies_are_suppressed_without_name_hashes() {
        let mut libraries = dide("_vs2008_x64_rwdi");
        libraries.extend(dide("_vs2012_x64_rwdi"));

        assert!(
            discriminators(&libraries).is_empty(),
            "two independently valid closures must not receive duplicate topology IDs"
        );

        let components = super::super::group_into_components(&game(), &libraries)
            .expect("ambiguous vendor grouping");
        assert_eq!(components.len(), 1);
        assert_eq!(components[0].swappability(), Swappability::ReadOnly);
        assert!(!components[0].id().as_str().contains("vendor-topology-v1-"));
    }

    #[test]
    fn authenticated_split_closure_groups_exact_paths_into_one_bundle() {
        let libraries = split_canonical("C:/game");

        let facts = grouping_facts(&libraries);
        assert_eq!(facts.closure_keys.len(), 3);
        assert_eq!(facts.discriminators.len(), 3);

        let components =
            super::super::group_into_components(&game(), &libraries).expect("split Xiph grouping");
        assert_eq!(components.len(), 1);
        assert_eq!(components[0].files().len(), 3);
        assert_eq!(components[0].swappability(), Swappability::BundleOnly);
        assert!(components[0].id().as_str().contains(":layout-v2:"));
    }

    #[test]
    fn local_unique_import_wins_over_an_equally_named_global_candidate() {
        let mut libraries = local_canonical("C:/game/bin");
        libraries.extend(local_canonical("C:/game/other"));

        let facts = grouping_facts(&libraries);
        assert!(facts.closure_keys.is_empty());
    }

    #[test]
    fn ambiguous_global_import_fails_closed() {
        let libraries = vec![
            library_at(
                "C:/game/bin",
                "vorbisfile.dll",
                Architecture::X64,
                &["vorbis.dll".to_owned(), "ogg.dll".to_owned()],
            ),
            library_at(
                "C:/game/a",
                "vorbis.dll",
                Architecture::X64,
                &["ogg.dll".to_owned()],
            ),
            library_at(
                "C:/game/b",
                "vorbis.dll",
                Architecture::X64,
                &["ogg.dll".to_owned()],
            ),
            library_at("C:/game/a", "ogg.dll", Architecture::X64, &[]),
            library_at("C:/game/b", "ogg.dll", Architecture::X64, &[]),
        ];

        assert!(grouping_facts(&libraries).closure_keys.is_empty());
    }

    #[test]
    fn incomplete_or_mixed_architecture_split_closure_fails_closed() {
        let mut incomplete = split_canonical("C:/game");
        incomplete[1].pe_compatibility = None;
        assert!(grouping_facts(&incomplete).closure_keys.is_empty());

        let mut mixed_architecture = split_canonical("C:/game");
        mixed_architecture[1].pe_compatibility =
            Some(profile(Architecture::X86, &["ogg.dll".to_owned()]));
        assert!(grouping_facts(&mixed_architecture).closure_keys.is_empty());
    }

    #[test]
    fn cross_directory_identity_is_root_relative_and_topology_sensitive() {
        let first = split_canonical("C:/game");
        let moved = split_canonical("D:/moved-game");
        let changed = vec![
            library_at(
                "C:/game/Plugin",
                "vorbisfile.dll",
                Architecture::X64,
                &["vorbis.dll".to_owned(), "ogg.dll".to_owned()],
            ),
            library_at(
                "C:/game/Codec",
                "vorbis.dll",
                Architecture::X64,
                &["ogg.dll".to_owned()],
            ),
            library_at("C:/game/Elsewhere", "ogg.dll", Architecture::X64, &[]),
        ];

        let first_id = super::super::group_into_components(&game(), &first)
            .expect("first")
            .remove(0)
            .id()
            .clone();
        let moved_id = super::super::group_into_components(&game_at("D:/moved-game"), &moved)
            .expect("moved")
            .remove(0)
            .id()
            .clone();
        let changed_id = super::super::group_into_components(&game(), &changed)
            .expect("changed")
            .remove(0)
            .id()
            .clone();
        assert_eq!(first_id, moved_id);
        assert_ne!(first_id, changed_id);
    }

    fn dide(suffix: &str) -> Vec<DetectedLibraryFile> {
        vec![
            library(
                &format!("vorbisfile{suffix}.dll"),
                Architecture::X64,
                &[format!("vorbis{suffix}.dll"), format!("ogg{suffix}.dll")],
            ),
            library(
                &format!("vorbis{suffix}.dll"),
                Architecture::X64,
                &[format!("ogg{suffix}.dll")],
            ),
            library(&format!("ogg{suffix}.dll"), Architecture::X64, &[]),
        ]
    }

    fn split_canonical(root: &str) -> Vec<DetectedLibraryFile> {
        vec![
            library_at(
                &format!("{root}/Plugin"),
                "vorbisfile.dll",
                Architecture::X64,
                &["vorbis.dll".to_owned(), "ogg.dll".to_owned()],
            ),
            library_at(
                &format!("{root}/Codec"),
                "vorbis.dll",
                Architecture::X64,
                &["ogg.dll".to_owned()],
            ),
            library_at(
                &format!("{root}/Container"),
                "ogg.dll",
                Architecture::X64,
                &[],
            ),
        ]
    }

    fn local_canonical(directory: &str) -> Vec<DetectedLibraryFile> {
        vec![
            library_at(
                directory,
                "vorbisfile.dll",
                Architecture::X64,
                &["vorbis.dll".to_owned(), "ogg.dll".to_owned()],
            ),
            library_at(
                directory,
                "vorbis.dll",
                Architecture::X64,
                &["ogg.dll".to_owned()],
            ),
            library_at(directory, "ogg.dll", Architecture::X64, &[]),
        ]
    }

    fn library(name: &str, architecture: Architecture, imports: &[String]) -> DetectedLibraryFile {
        library_at("C:/game", name, architecture, imports)
    }

    fn library_at(
        directory: &str,
        name: &str,
        architecture: Architecture,
        imports: &[String],
    ) -> DetectedLibraryFile {
        DetectedLibraryFile {
            file_name: name.to_owned(),
            file_path: PathRef::new(format!("{directory}/{name}")).expect("path"),
            technology: LibraryTechnology::XiphVorbis,
            kind: ComponentKind::NativeLibrary,
            detection_confidence: DetectionConfidence::Medium,
            swappability: Swappability::ReadOnly,
            version: None,
            status: VersionDetectionStatus::UnknownVersion,
            sha256: Sha256Hash::new("0".repeat(64)).expect("sha"),
            observation: None,
            runtime_target: None,
            pe_compatibility: Some(profile(architecture, imports)),
        }
    }

    fn profile(architecture: Architecture, imports: &[String]) -> PeCompatibilityProfile {
        PeCompatibilityProfile::new(
            architecture,
            PeExportSet::from_observed_names(vec!["xiph_export".to_owned()]).expect("export set"),
        )
        .with_imports(PeImportProfile {
            regular: PeImportSet::from_observed_names(imports.to_vec()).expect("imports"),
            delay: PeImportSet::from_canonical_names(Vec::new()).expect("empty delay imports"),
        })
    }

    fn game() -> GameInstallation {
        game_at("C:/game")
    }

    fn game_at(root: &str) -> GameInstallation {
        let install_path = PathRef::new(root).expect("install path");
        let identity = GameIdentity::new(
            GameId::new("manual:xiph-grouping-test").expect("game id"),
            "Xiph grouping test",
            Launcher::Manual,
        )
        .expect("game identity");
        GameInstallation::new(
            identity,
            Platform::Windows,
            GameRuntime::NativeWindows,
            install_path,
        )
    }
}
