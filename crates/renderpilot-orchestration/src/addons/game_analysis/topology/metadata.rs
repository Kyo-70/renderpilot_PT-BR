//! Topologically bound metadata files (`Build.version` and `.uproject`).

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::marker::PhantomData;
use std::path::{Path, PathBuf};

use super::executable::{
    BoundPrimaryExecutable, TopologyError, is_inside_engine_binaries, open_shared_file,
};
use crate::addons::game_analysis::budget::MAX_METADATA_FILE_BYTES;
use crate::addons::game_analysis::context::GameInstallationContext;

pub struct EngineMetadataScope;
pub struct ProjectMetadataScope;
pub struct TargetMetadataScope;

/// Origin-bound validated metadata text.
pub struct BoundMetadataText<'game, Scope> {
    context: &'game GameInstallationContext,
    path: PathBuf,
    text: String,
    _scope: PhantomData<Scope>,
}

impl<'game, Scope> BoundMetadataText<'game, Scope> {
    #[must_use]
    pub fn context(&self) -> &'game GameInstallationContext {
        self.context
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.text
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MetadataReadError {
    Io {
        kind: io::ErrorKind,
        message: String,
    },
    TooLarge {
        actual_bytes: u64,
        max_bytes: usize,
    },
    InvalidUtf8,
}

impl From<io::Error> for MetadataReadError {
    fn from(err: io::Error) -> Self {
        Self::Io {
            kind: err.kind(),
            message: err.to_string(),
        }
    }
}

/// Standard bounded UTF-8 reader with automatic BOM stripping.
///
/// Reads up to `max_bytes + 1`, rejects files exceeding `max_bytes`, strips optional UTF-8 BOM
/// via `crate::fs::strip_utf8_bom`, and validates UTF-8 validity.
pub fn read_capped_utf8<R: Read + Seek>(
    reader: &mut R,
    max_bytes: usize,
) -> Result<String, MetadataReadError> {
    reader.seek(SeekFrom::Start(0))?;
    let mut bytes = Vec::with_capacity(max_bytes.saturating_add(1));
    reader
        .take((max_bytes as u64) + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > max_bytes {
        return Err(MetadataReadError::TooLarge {
            actual_bytes: bytes.len() as u64,
            max_bytes,
        });
    }

    let clean_len = crate::fs::strip_utf8_bom(&bytes).len();
    let bom_len = bytes.len() - clean_len;
    if bom_len > 0 {
        bytes.drain(..bom_len);
    }
    String::from_utf8(bytes).map_err(|_| MetadataReadError::InvalidUtf8)
}

/// Topologically verified descriptor for `<game_root>/Engine/Build/Build.version`.
pub struct BoundEngineMetadata<'game> {
    context: &'game GameInstallationContext,
    canonical_path: PathBuf,
    file: File,
}

impl<'game> BoundEngineMetadata<'game> {
    /// Proof constructor: verifies exact canonical location `<game_root>/Engine/Build/Build.version`.
    pub fn open(
        context: &'game GameInstallationContext,
        path: &Path,
    ) -> Result<Self, TopologyError> {
        let canonical_path = std::fs::canonicalize(path)?;
        if !canonical_path.starts_with(context.root_path()) {
            return Err(TopologyError::OutsideInstallationContext(canonical_path));
        }
        let expected = context
            .root_path()
            .join("Engine")
            .join("Build")
            .join("Build.version");
        if canonical_path != expected {
            return Err(TopologyError::InvalidMetadataLocation(canonical_path));
        }

        let file = open_shared_file(&canonical_path)?;
        Ok(Self {
            context,
            canonical_path,
            file,
        })
    }

    pub fn read_manifest(
        &mut self,
    ) -> Result<BoundMetadataText<'game, EngineMetadataScope>, MetadataReadError> {
        let text = read_capped_utf8(&mut self.file, MAX_METADATA_FILE_BYTES)?;
        Ok(BoundMetadataText {
            context: self.context,
            path: self.canonical_path.clone(),
            text,
            _scope: PhantomData,
        })
    }
}

/// Topologically verified descriptor for strictly bound Primary `<target>.version` file.
pub struct BoundTargetMetadata<'game> {
    context: &'game GameInstallationContext,
    canonical_path: PathBuf,
    file: File,
}

impl<'game> BoundTargetMetadata<'game> {
    /// Proof constructor: strictly probes `<primary_canonical_without_ext>.version`
    /// directly bound to the verified `BoundPrimaryExecutable`.
    pub fn from_primary(
        primary: &BoundPrimaryExecutable<'game>,
    ) -> Result<Option<Self>, TopologyError> {
        let context = primary.context();
        let candidate = primary.path().with_extension("version");
        let symlink_meta = match std::fs::symlink_metadata(&candidate) {
            Ok(meta) => meta,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => {
                return Err(TopologyError::Io {
                    kind: e.kind(),
                    message: e.to_string(),
                });
            }
        };

        if symlink_meta.file_type().is_symlink() {
            return Err(TopologyError::SymlinkDisallowed(candidate));
        }
        if !symlink_meta.file_type().is_file() {
            return Ok(None);
        }

        let canonical_path = std::fs::canonicalize(&candidate)?;
        if canonical_path != candidate {
            return Err(TopologyError::SymlinkDisallowed(canonical_path));
        }
        if !canonical_path.starts_with(context.root_path()) {
            return Err(TopologyError::OutsideInstallationContext(canonical_path));
        }

        let file = open_shared_file(&canonical_path)?;
        Ok(Some(Self {
            context,
            canonical_path,
            file,
        }))
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.canonical_path
    }

    pub fn read_manifest(
        &mut self,
    ) -> Result<BoundMetadataText<'game, TargetMetadataScope>, MetadataReadError> {
        let text = read_capped_utf8(&mut self.file, MAX_METADATA_FILE_BYTES)?;
        Ok(BoundMetadataText {
            context: self.context,
            path: self.canonical_path.clone(),
            text,
            _scope: PhantomData,
        })
    }
}

/// Topologically verified descriptor for project `.uproject` file.
pub struct BoundProjectMetadata<'game> {
    context: &'game GameInstallationContext,
    canonical_path: PathBuf,
    file: File,
}

impl<'game> BoundProjectMetadata<'game> {
    pub fn open(
        context: &'game GameInstallationContext,
        path: &Path,
    ) -> Result<Self, TopologyError> {
        let canonical_path = std::fs::canonicalize(path)?;
        if !canonical_path.starts_with(context.root_path()) {
            return Err(TopologyError::OutsideInstallationContext(canonical_path));
        }
        if is_inside_engine_binaries(&canonical_path, context.root_path()) {
            return Err(TopologyError::EngineBinariesDisallowed(canonical_path));
        }
        let is_uproject = canonical_path
            .extension()
            .and_then(std::ffi::OsStr::to_str)
            .is_some_and(|ext| ext.eq_ignore_ascii_case("uproject"));
        if !is_uproject {
            return Err(TopologyError::InvalidExtension(canonical_path));
        }

        let file = open_shared_file(&canonical_path)?;
        Ok(Self {
            context,
            canonical_path,
            file,
        })
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.canonical_path
    }

    pub fn read_manifest(
        &mut self,
    ) -> Result<BoundMetadataText<'game, ProjectMetadataScope>, MetadataReadError> {
        let text = read_capped_utf8(&mut self.file, MAX_METADATA_FILE_BYTES)?;
        Ok(BoundMetadataText {
            context: self.context,
            path: self.canonical_path.clone(),
            text,
            _scope: PhantomData,
        })
    }
}

fn strip_ascii_case_suffix<'a>(s: &'a str, suffix: &str) -> Option<&'a str> {
    let suffix_bytes = suffix.as_bytes();
    let s_bytes = s.as_bytes();
    if s_bytes.len() >= suffix_bytes.len() {
        let split_idx = s_bytes.len() - suffix_bytes.len();
        if s_bytes[split_idx..].eq_ignore_ascii_case(suffix_bytes) && s.is_char_boundary(split_idx)
        {
            return Some(&s[..split_idx]);
        }
    }
    None
}

const UE_EXE_SUFFIXES: &[&str] = &[
    "-Win64-Shipping",
    "-Win64-Development",
    "-Win64-Test",
    "-Win64-Debug",
    "-Win32-Shipping",
    "-Win32-Development",
    "-Win32-Test",
    "-Win32-Debug",
    "_Win64_Shipping",
    "_Win64_Development",
    "_Win32_Shipping",
    "_Win32_Development",
    "-Shipping",
    "-Development",
    "-Test",
    "-Debug",
    "_Shipping",
    "_Development",
    "_Test",
    "_Debug",
    "-Win64",
    "-Win32",
    "_Win64",
    "_Win32",
    "Shipping",
    "Development",
    "Win64",
    "Win32",
];

/// Normalizes an executable stem by stripping standard Unreal build suffixes.
fn strip_ue_exe_suffixes(stem: &str) -> &str {
    let mut s = stem;
    let mut changed = true;
    while changed {
        changed = false;
        for suffix in UE_EXE_SUFFIXES {
            if let Some(stripped) = strip_ascii_case_suffix(s, suffix).filter(|s| !s.is_empty()) {
                s = stripped;
                changed = true;
                break;
            }
        }
    }
    s
}

#[derive(Debug)]
pub enum ProjectMetadataBindError {
    Ambiguous(Vec<PathBuf>),
    Topology(TopologyError),
}

/// Locates and binds `.uproject` metadata associated with the given Primary executable.
///
/// Returns:
/// - `Ok(Some(bound))` when an unambiguous matching `.uproject` is bound.
/// - `Ok(None)` when no `.uproject` exists.
/// - `Err(ProjectMetadataBindError::Ambiguous(conflicting_paths))` when ambiguous descriptors are detected.
/// - `Err(ProjectMetadataBindError::Topology(e))` on underlying topology errors.
pub fn find_and_bind_project_metadata<'game>(
    context: &'game GameInstallationContext,
    primary_path: &Path,
) -> Result<Option<BoundProjectMetadata<'game>>, ProjectMetadataBindError> {
    let mut candidate_dirs: Vec<&Path> = Vec::new();

    // 1. Traverse up from primary binary directory to project root and install root
    let mut curr = primary_path.parent();
    while let Some(dir) = curr {
        if !dir.starts_with(context.root_path()) {
            break;
        }
        // Never look inside Engine directories
        if is_inside_engine_binaries(dir, context.root_path()) {
            curr = dir.parent();
            continue;
        }
        if !candidate_dirs.contains(&dir) {
            candidate_dirs.push(dir);
        }
        if dir == context.root_path() {
            break;
        }
        curr = dir.parent();
    }

    // Also include root_path if not already included
    if !candidate_dirs.contains(&context.root_path()) {
        candidate_dirs.push(context.root_path());
    }

    // 2. Discover all .uproject files in candidate directories (non-recursive)
    let mut discovered: Vec<PathBuf> = Vec::new();
    for dir in &candidate_dirs {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                let is_uproject = p
                    .extension()
                    .and_then(std::ffi::OsStr::to_str)
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("uproject"));
                if !p.is_file() || !is_uproject {
                    continue;
                }
                if let Some(canonical) = std::fs::canonicalize(&p)
                    .ok()
                    .filter(|c| !discovered.contains(c))
                {
                    discovered.push(canonical);
                }
            }
        }
    }

    if discovered.is_empty() {
        return Ok(None);
    }

    if discovered.len() == 1 {
        let bound = BoundProjectMetadata::open(context, &discovered[0])
            .map_err(ProjectMetadataBindError::Topology)?;
        return Ok(Some(bound));
    }

    // 3. Stem matching with Primary executable name and directory names
    let exe_stem = primary_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    let clean_exe_stem = strip_ue_exe_suffixes(exe_stem);

    let mut single_match: Option<&Path> = None;
    let mut ambiguous = false;
    for uproj in &discovered {
        let stem = uproj.file_stem().and_then(|s| s.to_str()).unwrap_or("");

        // Matches if stem equals cleaned exe stem (e.g. ShooterGame.uproject == ShooterGame-Win64-Shipping.exe)
        let matches_stem = !clean_exe_stem.is_empty() && stem.eq_ignore_ascii_case(clean_exe_stem);
        // Or stem equals parent dir name (e.g. ShooterGame/ShooterGame.uproject)
        let matches_parent = uproj
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .is_some_and(|parent_dir_name| stem.eq_ignore_ascii_case(parent_dir_name));

        if matches_stem || matches_parent {
            if single_match.is_some() {
                ambiguous = true;
                break;
            }
            single_match = Some(uproj.as_path());
        }
    }

    if let Some(target) = single_match.filter(|_| !ambiguous) {
        let bound = BoundProjectMetadata::open(context, target)
            .map_err(ProjectMetadataBindError::Topology)?;
        Ok(Some(bound))
    } else {
        // AmbiguousProjectHierarchy: multiple candidates without unique match
        Err(ProjectMetadataBindError::Ambiguous(discovered))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_ue_exe_suffixes_variants() {
        assert_eq!(
            strip_ue_exe_suffixes("ShooterGame-Win64-Shipping"),
            "ShooterGame"
        );
        assert_eq!(strip_ue_exe_suffixes("ShooterGame-Win64"), "ShooterGame");
        assert_eq!(strip_ue_exe_suffixes("ShooterGame-win64"), "ShooterGame");
        assert_eq!(
            strip_ue_exe_suffixes("ShooterGame_WIN64_SHIPPING"),
            "ShooterGame"
        );
        assert_eq!(
            strip_ue_exe_suffixes("ShooterGame-Win64-Development"),
            "ShooterGame"
        );
        assert_eq!(
            strip_ue_exe_suffixes("ShooterGame-Win32-Shipping"),
            "ShooterGame"
        );
        assert_eq!(strip_ue_exe_suffixes("ShooterGame-Win32"), "ShooterGame");
        assert_eq!(strip_ue_exe_suffixes("ShooterGame-Shipping"), "ShooterGame");
        assert_eq!(strip_ue_exe_suffixes("ShooterGame"), "ShooterGame");
        assert_eq!(strip_ue_exe_suffixes("Win64"), "Win64");
        assert_eq!(strip_ue_exe_suffixes("Shipping"), "Shipping");
        assert_eq!(strip_ue_exe_suffixes("Game-Win64-Shipping"), "Game");
    }
}
