//! Classic `.bak` baseline resolution and conflict vocabulary.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};

use renderpilot_application::AppError;
use renderpilot_domain::{
    ComponentFile, LibraryTechnology, ManagedAddonFile, ManagedFileBaseline, ManagedFileMode,
    PathRef, Sha256Hash, normalized_path_key, xiph,
};

/// The trustworthy source selected for one pre-mutation file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ResolvedBaseline {
    /// A catalog baseline already exists and was verified against disk.
    RecordedBaseline(ComponentFile),
    /// An unrecorded classic sidecar was verified and adopted.
    ExistingSidecarBaseline(ComponentFile),
    /// No sidecar exists; the current live bytes become the first baseline.
    FreshLiveBaseline(ComponentFile),
    /// An add-on owns the live file and recorded that the path was originally absent.
    AddonOwnedAbsent,
}

impl ResolvedBaseline {
    pub(crate) fn file(&self) -> Option<&ComponentFile> {
        match self {
            Self::RecordedBaseline(file)
            | Self::ExistingSidecarBaseline(file)
            | Self::FreshLiveBaseline(file) => Some(file),
            Self::AddonOwnedAbsent => None,
        }
    }
}

/// Typed reason why disk state cannot safely be interpreted as a baseline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BaselineConflict {
    OutsideGameRoot(PathBuf),
    NotAFile(PathBuf),
    Empty(PathBuf),
    Unreadable(PathBuf, String),
    MissingRecordedBytes(PathBuf),
    MissingRecordedHash(PathBuf),
    HashMismatch {
        path: PathBuf,
        expected: Sha256Hash,
        actual: Sha256Hash,
    },
    UnexpectedSidecarForAbsentBaseline(PathBuf),
    InvalidPath(String),
    MissingActiveFile(PathBuf),
    MissingActiveHash(PathBuf),
    ActiveHashMismatch {
        path: PathBuf,
        catalog: Sha256Hash,
        managed: Option<Sha256Hash>,
        actual: Sha256Hash,
    },
    /// A persisted Xiph baseline does not cover exactly the component paths
    /// it would be allowed to restore.
    XiphBaselineCoverage(String),
    /// A persisted Xiph baseline cannot be interpreted as one complete,
    /// validated Xiph topology.
    InvalidXiphBaselineLayout,
}

impl fmt::Display for BaselineConflict {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutsideGameRoot(path) => write!(
                formatter,
                "baseline path is outside the game root: {}",
                path.display()
            ),
            Self::NotAFile(path) => {
                write!(
                    formatter,
                    "baseline path is not a regular file: {}",
                    path.display()
                )
            }
            Self::Empty(path) => write!(formatter, "baseline file is empty: {}", path.display()),
            Self::Unreadable(path, detail) => {
                write!(
                    formatter,
                    "cannot read baseline {}: {detail}",
                    path.display()
                )
            }
            Self::MissingRecordedBytes(path) => write!(
                formatter,
                "recorded baseline bytes are missing for {}",
                path.display()
            ),
            Self::MissingRecordedHash(path) => write!(
                formatter,
                "recorded baseline has no integrity hash for {}",
                path.display()
            ),
            Self::HashMismatch {
                path,
                expected,
                actual,
            } => write!(
                formatter,
                "baseline hash mismatch for {}: expected {expected}, got {actual}",
                path.display()
            ),
            Self::UnexpectedSidecarForAbsentBaseline(path) => write!(
                formatter,
                "{} exists although the recorded pre-owner baseline was absent",
                path.display()
            ),
            Self::InvalidPath(detail) => formatter.write_str(detail),
            Self::MissingActiveFile(path) => {
                write!(
                    formatter,
                    "active component file is missing: {}",
                    path.display()
                )
            }
            Self::MissingActiveHash(path) => write!(
                formatter,
                "active component has no recorded hash for {}",
                path.display()
            ),
            Self::ActiveHashMismatch {
                path,
                catalog,
                managed,
                actual,
            } => {
                write!(
                    formatter,
                    "active file hash mismatch for {}: catalog expected {catalog}",
                    path.display()
                )?;
                if let Some(managed) = managed {
                    write!(formatter, ", managed owner expected {managed}")?;
                }
                write!(formatter, ", got {actual}")
            }
            Self::XiphBaselineCoverage(detail) => {
                write!(
                    formatter,
                    "Xiph rollback baseline coverage conflict: {detail}"
                )
            }
            Self::InvalidXiphBaselineLayout => formatter
                .write_str("Xiph rollback baseline does not form a complete valid runtime layout"),
        }
    }
}

impl std::error::Error for BaselineConflict {}

impl From<BaselineConflict> for AppError {
    fn from(error: BaselineConflict) -> Self {
        AppError::invalid_input(error.to_string())
    }
}

/// Resolves one live path against catalog and add-on ownership facts.
pub(crate) struct BaselineResolver<'a> {
    game_root: &'a Path,
    managed_files: &'a [ManagedAddonFile],
    technology: LibraryTechnology,
}

impl<'a> BaselineResolver<'a> {
    pub(crate) fn new(
        game_root: &'a Path,
        managed_files: &'a [ManagedAddonFile],
        technology: LibraryTechnology,
    ) -> Self {
        Self {
            game_root,
            managed_files,
            technology,
        }
    }

    pub(crate) fn resolve(
        &self,
        live_path: &Path,
        recorded: Option<&ComponentFile>,
    ) -> Result<ResolvedBaseline, BaselineConflict> {
        self.require_inside_game(live_path)?;
        let sidecar = crate::fs::backup_path(live_path)
            .map_err(|error| BaselineConflict::InvalidPath(error.to_string()))?;
        self.require_inside_game(&sidecar)?;

        let binding = self.binding_for(live_path);
        if let Some(binding) = binding {
            Self::validate_binding_sidecar(binding, &sidecar)?;
        }

        if let Some(recorded) = recorded {
            self.resolve_recorded(live_path, &sidecar, recorded, binding)
        } else if binding.is_some_and(|entry| {
            entry.mode() == ManagedFileMode::Owned
                && matches!(entry.baseline(), ManagedFileBaseline::Absent)
        }) {
            Ok(ResolvedBaseline::AddonOwnedAbsent)
        } else if sidecar.exists() {
            let file = component_file_from_disk(&sidecar, live_path, self.technology)?;
            Ok(ResolvedBaseline::ExistingSidecarBaseline(file))
        } else {
            let file = component_file_from_disk(live_path, live_path, self.technology)?;
            Ok(ResolvedBaseline::FreshLiveBaseline(file))
        }
    }

    fn resolve_recorded(
        &self,
        live_path: &Path,
        sidecar: &Path,
        recorded: &ComponentFile,
        binding: Option<&ManagedAddonFile>,
    ) -> Result<ResolvedBaseline, BaselineConflict> {
        let expected = recorded
            .sha256()
            .cloned()
            .ok_or_else(|| BaselineConflict::MissingRecordedHash(live_path.to_path_buf()))?;

        let bytes_path = if sidecar.exists() {
            sidecar
        } else if binding.is_some_and(|entry| {
            entry.mode() == ManagedFileMode::Reused
                || matches!(entry.baseline(), ManagedFileBaseline::Absent)
        }) {
            return Err(BaselineConflict::MissingRecordedBytes(
                sidecar.to_path_buf(),
            ));
        } else {
            live_path
        };
        let actual = verified_hash(bytes_path)?;
        if actual != expected {
            return Err(BaselineConflict::HashMismatch {
                path: bytes_path.to_path_buf(),
                expected,
                actual,
            });
        }

        let mut refreshed = ComponentFile::new(recorded.path().clone()).with_sha256(actual);
        if let Some(install_as) = recorded.install_as() {
            refreshed = refreshed.with_install_as(install_as);
        }
        Ok(ResolvedBaseline::RecordedBaseline(
            super::with_observed_metadata(refreshed, self.technology, bytes_path),
        ))
    }

    fn validate_binding_sidecar(
        binding: &ManagedAddonFile,
        sidecar: &Path,
    ) -> Result<(), BaselineConflict> {
        if binding.mode() != ManagedFileMode::Owned {
            return Ok(());
        }

        match binding.baseline() {
            ManagedFileBaseline::Absent if sidecar.exists() => Err(
                BaselineConflict::UnexpectedSidecarForAbsentBaseline(sidecar.to_path_buf()),
            ),
            ManagedFileBaseline::Absent => Ok(()),
            ManagedFileBaseline::Present { sha256 } => {
                if !sidecar.exists() {
                    return Err(BaselineConflict::MissingRecordedBytes(
                        sidecar.to_path_buf(),
                    ));
                }
                let actual = verified_hash(sidecar)?;
                if &actual != sha256 {
                    return Err(BaselineConflict::HashMismatch {
                        path: sidecar.to_path_buf(),
                        expected: sha256.clone(),
                        actual,
                    });
                }
                Ok(())
            }
        }
    }

    fn binding_for(&self, path: &Path) -> Option<&ManagedAddonFile> {
        self.managed_files
            .iter()
            .find(|binding| crate::paths::same_path(Path::new(binding.path().as_str()), path))
    }

    fn require_inside_game(&self, path: &Path) -> Result<(), BaselineConflict> {
        let root = crate::paths::canonical_candidate(self.game_root).map_err(|error| {
            BaselineConflict::Unreadable(self.game_root.to_path_buf(), error.to_string())
        })?;
        let candidate = crate::paths::canonical_candidate(path)
            .map_err(|error| BaselineConflict::Unreadable(path.to_path_buf(), error.to_string()))?;
        if crate::paths::is_within(&candidate, &root) {
            Ok(())
        } else {
            Err(BaselineConflict::OutsideGameRoot(path.to_path_buf()))
        }
    }
}

/// Resolves the complete immutable baseline for a component.
pub(crate) fn resolve_component_baseline(
    game_root: &Path,
    technology: LibraryTechnology,
    current: &[ComponentFile],
    recorded: Option<&[ComponentFile]>,
    managed_files: &[ManagedAddonFile],
) -> Result<Vec<ComponentFile>, BaselineConflict> {
    let resolver = BaselineResolver::new(game_root, managed_files, technology);
    if let Some(recorded) = recorded {
        validate_recorded_xiph_baseline(technology, current, recorded, managed_files)?;
        return recorded
            .iter()
            .map(|file| {
                let path = Path::new(file.path().as_str());
                resolver
                    .resolve(path, Some(file))?
                    .file()
                    .cloned()
                    .ok_or_else(|| BaselineConflict::MissingRecordedBytes(path.to_path_buf()))
            })
            .collect();
    }

    current
        .iter()
        .filter_map(
            |file| match resolver.resolve(Path::new(file.path().as_str()), None) {
                Ok(ResolvedBaseline::AddonOwnedAbsent) => None,
                Ok(resolved) => resolved.file().cloned().map(Ok),
                Err(error) => Some(Err(error)),
            },
        )
        .collect()
}

/// Validates the durable topology contract before a recorded Xiph baseline is
/// allowed to select live or sidecar bytes. This guard is shared by swap,
/// rollback, recovery, and the pre-persistence admission check.
pub(crate) fn validate_recorded_xiph_baseline(
    technology: LibraryTechnology,
    current: &[ComponentFile],
    recorded: &[ComponentFile],
    managed_files: &[ManagedAddonFile],
) -> Result<(), BaselineConflict> {
    if technology != LibraryTechnology::XiphVorbis {
        return Ok(());
    }

    let current_by_member = xiph_files_by_member("current", current, true)?;
    let recorded_by_member = xiph_files_by_member("recorded baseline", recorded, false)?;
    // These are an independent path-safety invariant. They are deliberately
    // not the baseline coverage relation: a vendor original can legitimately
    // restore to a different canonical active path for the same member.
    files_by_normalized_path("current", current)?;
    files_by_normalized_path("recorded baseline", recorded)?;

    let absent_members = current_by_member
        .iter()
        .filter(|(_, current)| {
            let current_path = normalized_path_key(current.path().as_str());
            managed_files.iter().any(|binding| {
                binding.mode() == ManagedFileMode::Owned
                    && matches!(binding.baseline(), ManagedFileBaseline::Absent)
                    && normalized_path_key(binding.path().as_str()) == current_path
            })
        })
        .map(|(member, _)| *member)
        .collect::<BTreeSet<_>>();
    let covered_members = recorded_by_member
        .keys()
        .copied()
        .collect::<BTreeSet<_>>()
        .union(&absent_members)
        .copied()
        .collect::<BTreeSet<_>>();
    let current_members = current_by_member.keys().copied().collect::<BTreeSet<_>>();
    if covered_members != current_members {
        return Err(BaselineConflict::XiphBaselineCoverage(
            "recorded members plus exact owned-absent current members do not exactly equal the current Xiph semantic set"
                .to_owned(),
        ));
    }

    // The parse passes above intentionally run even when an owned-absent
    // exception supplies a member. A corrupted record must never be hidden by
    // add-on ownership. Layout validation uses the current metadata only for
    // that exact, explicitly absent path; it is never a fallback for a missing
    // ordinary immutable baseline member.
    let mut layout_files = Vec::with_capacity(current_by_member.len());
    for member in current_by_member.keys() {
        if let Some(file) = recorded_by_member.get(member) {
            layout_files.push(*file);
        } else if absent_members.contains(member) {
            layout_files.push(current_by_member[member]);
        } else {
            return Err(BaselineConflict::XiphBaselineCoverage(
                "current Xiph semantic member has no immutable baseline identity".to_owned(),
            ));
        }
    }
    let layout = xiph::detect_layout_with_file_names(layout_files.iter().map(|file| {
        (
            xiph_runtime_name(file).expect("Xiph member was parsed above"),
            *file,
        )
    }));
    if layout.is_none()
        || current_by_member.len() != layout_files.len()
        || recorded_by_member.len() > current_by_member.len()
    {
        return Err(BaselineConflict::InvalidXiphBaselineLayout);
    }
    Ok(())
}

fn xiph_files_by_member<'a>(
    label: &str,
    files: &'a [ComponentFile],
    require_nonempty: bool,
) -> Result<BTreeMap<xiph::XiphMember, &'a ComponentFile>, BaselineConflict> {
    let mut result = BTreeMap::new();
    for file in files {
        let name = xiph_runtime_name(file).ok_or_else(|| {
            BaselineConflict::XiphBaselineCoverage(format!(
                "{label} Xiph file has no runtime basename"
            ))
        })?;
        let member = xiph::parse_runtime_file_name(name)
            .ok()
            .flatten()
            .ok_or_else(|| {
                BaselineConflict::XiphBaselineCoverage(format!(
                    "{label} contains an unsupported Xiph runtime basename: {name}"
                ))
            })?
            .member();
        if result.insert(member, file).is_some() {
            return Err(BaselineConflict::XiphBaselineCoverage(format!(
                "{label} contains duplicate Xiph semantic member {}",
                member.as_slug()
            )));
        }
    }
    if require_nonempty && result.is_empty() {
        return Err(BaselineConflict::XiphBaselineCoverage(format!(
            "{label} contains no Xiph semantic members"
        )));
    }
    Ok(result)
}

fn files_by_normalized_path<'a>(
    label: &str,
    files: &'a [ComponentFile],
) -> Result<BTreeMap<String, &'a ComponentFile>, BaselineConflict> {
    let mut result = BTreeMap::new();
    for file in files {
        let path = normalized_path_key(file.path().as_str());
        if result.insert(path.clone(), file).is_some() {
            return Err(BaselineConflict::XiphBaselineCoverage(format!(
                "{label} contains duplicate normalized path {path}"
            )));
        }
    }
    Ok(result)
}

fn xiph_runtime_name(file: &ComponentFile) -> Option<&str> {
    file.install_as().or_else(|| file.path().file_name())
}

fn component_file_from_disk(
    bytes_path: &Path,
    live_path: &Path,
    technology: LibraryTechnology,
) -> Result<ComponentFile, BaselineConflict> {
    let sha256 = verified_hash(bytes_path)?;
    let path = PathRef::new(live_path.to_string_lossy().as_ref())
        .map_err(|error| BaselineConflict::InvalidPath(error.to_string()))?;
    let file = ComponentFile::new(path).with_sha256(sha256);
    Ok(super::with_observed_metadata(file, technology, bytes_path))
}

/// Maps [`crate::fs::sha256_of_non_empty_file`] into [`BaselineConflict`] vocabulary.
pub(crate) fn verified_hash(path: &Path) -> Result<Sha256Hash, BaselineConflict> {
    crate::fs::sha256_of_non_empty_file(path).map_err(|error| match error {
        crate::fs::NonEmptyFileError::Unreadable { path, detail } => {
            BaselineConflict::Unreadable(path, detail)
        }
        crate::fs::NonEmptyFileError::NotAFile(path) => BaselineConflict::NotAFile(path),
        crate::fs::NonEmptyFileError::Empty(path) => BaselineConflict::Empty(path),
        crate::fs::NonEmptyFileError::HashFailed(detail) => {
            BaselineConflict::Unreadable(path.to_path_buf(), detail)
        }
    })
}
