//! Assembling [`MatchFacts`] for a game and analyzing Unreal Engine installations.
//!
//! Bridges an installed game, low-level PE/metadata parsing, and the matching layer.
//! The game's rendering executable is resolved by the shared [`game_executable`]
//! resolver, and the engine is detected via hierarchical authority resolution
//! (`analyze_unreal_installation`).

pub mod authority;
pub mod budget;
pub mod context;
pub mod evidence;
pub mod evidence_set;
pub mod facade;
pub mod forensics;
pub mod parsers;
pub mod renodx_matcher;
pub mod resolver;
pub mod topology;

#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};

use renderpilot_domain::{ExeGraphicsInfo, GameInstallation, PathRef};

use super::errors::invalid;
use crate::ServiceError;
use crate::addons::matching::{Engine, MatchFacts, UnrealVersion};
use crate::game_executable::{self, ResolvedExecutable};
use renderpilot_platform_windows::{EngineLayoutRequest, analyze_engine_layout};

use self::budget::AnalysisBudget;
use self::context::GameInstallationContext;
use self::facade::{UnrealDetection, analyze_unreal_installation};

pub use facade::EngineDetection;
pub use renodx_matcher::{RenoDxCompatibility, evaluate_ue_extended_fallback_compatibility};
pub use topology::executable::TargetPlatformDetection;

/// Result of inspecting a game: the facts the matcher needs plus the chosen
/// rendering executable, whose folder is where ReShade and the add-on install.
#[derive(Debug, Clone)]
pub struct GameAnalysis {
    /// Facts to resolve against the manifest.
    pub facts: MatchFacts,
    /// The executable selected as the game's renderer, when one was found.
    pub primary_executable: Option<PathRef>,
    /// Exact Unreal project identity proven from the executable topology.
    pub unreal_project: Option<UnrealProjectIdentity>,
}

impl GameAnalysis {
    /// Returns the exact Unreal project proof for bounded Engine.ini
    /// resolution.  Keeping this accessor beside the analysis result makes
    /// it difficult for a caller to substitute a marketing name or arbitrary
    /// executable path.
    #[must_use]
    pub(crate) fn unreal_project_identity(&self) -> Option<&UnrealProjectIdentity> {
        self.unreal_project.as_ref()
    }
}

/// Exact project root/name proven by `<Project>/Binaries` + `<Project>/Content`
/// and the selected executable being inside that Binaries tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnrealProjectIdentity {
    /// Proven project root.
    pub project_root: PathBuf,
    /// Final project-root component used for LocalAppData resolution.
    pub project_name: String,
}

/// Inspects an installed game and assembles its [`GameAnalysis`].
///
/// `override_path` is the user's pinned executable (if any); it wins over
/// auto-detection. Pass `None` to auto-detect.
#[must_use]
pub fn analyze_game(install: &GameInstallation, override_path: Option<&Path>) -> GameAnalysis {
    let install_dir = Path::new(install.install_path().as_str());
    let primary = game_executable::resolve_primary_executable(install_dir, override_path, true);
    let facts = assemble_facts(install, primary.as_ref());
    let unreal_project = primary.as_ref().and_then(|resolved| {
        let executable = PathBuf::from(resolved.path.as_str());
        analyze_engine_layout(&EngineLayoutRequest {
            candidate: install_dir,
            accepted_executables: std::slice::from_ref(&executable),
        })
        .into_iter()
        .find_map(|evidence| {
            let root = evidence.project_root()?;
            let name = root.file_name()?.to_string_lossy();
            (!name.is_empty()).then(|| UnrealProjectIdentity {
                project_root: root.to_path_buf(),
                project_name: name.into_owned(),
            })
        })
    });
    GameAnalysis {
        facts,
        primary_executable: primary.map(|resolved| resolved.path),
        unreal_project,
    }
}

/// The folder an add-on installs into: the resolved rendering executable's
/// directory. Shared by the install, update, and availability flows so they agree
/// on the target location.
pub fn install_target_dir(analysis: &GameAnalysis) -> Result<PathBuf, ServiceError> {
    let executable = analysis
        .primary_executable
        .as_ref()
        .ok_or_else(|| invalid("no rendering executable found for this game".to_owned()))?;
    Path::new(executable.as_str())
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| invalid("rendering executable has no parent directory".to_owned()))
}

/// Resolves the same install roots that install/update/exclusivity use for on-disk
/// scans: the rendering executable's parent plus any split `AddonPath` from
/// `ReShade.ini`. Returns `None` when no executable is known yet so callers can
/// fall back to a DB-only exclusivity check.
#[must_use]
pub fn install_roots_for_analysis(
    analysis: &GameAnalysis,
) -> Option<crate::addons::reshade::InstallRoots> {
    install_target_dir(analysis)
        .ok()
        .map(|dir| crate::addons::reshade::InstallRoots::resolve_from_ini(&dir))
}

/// Assembles [`MatchFacts`] from a game and its already-resolved primary
/// executable.
#[must_use]
pub fn assemble_facts(
    install: &GameInstallation,
    primary: Option<&ResolvedExecutable>,
) -> MatchFacts {
    let exe_file_name = primary.map(|resolved| resolved.file_name.clone());
    let graphics = primary.map_or_else(
        || ExeGraphicsInfo::new(Vec::new(), None),
        |resolved| resolved.graphics.clone(),
    );

    let install_dir = Path::new(install.install_path().as_str());
    let (engine, unreal_version, unreal_detection, target_platform) =
        detect_engine_and_version(install_dir, primary);

    MatchFacts {
        launcher: install.identity().launcher(),
        external_id: install.identity().external_id().map(str::to_owned),
        exe_file_name,
        engine,
        unreal_version,
        graphics,
        unreal_detection,
        target_platform,
    }
}

fn detect_engine_and_version(
    install_dir: &Path,
    primary: Option<&ResolvedExecutable>,
) -> (
    Option<Engine>,
    Option<UnrealVersion>,
    Option<EngineDetection>,
    Option<TargetPlatformDetection>,
) {
    if install_dir.join("UnityPlayer.dll").is_file() || has_unity_data_dir(install_dir) {
        return (Some(Engine::Unity), None, None, None);
    }

    if let Ok(context) = GameInstallationContext::new(install_dir) {
        let mut budget = AnalysisBudget::default();
        let report = analyze_unreal_installation(&context, primary, &mut budget);
        let platform = Some(report.platform);
        let unreal_version = match &report.engine {
            EngineDetection::Unreal(detection) => match detection {
                UnrealDetection::Exact {
                    major,
                    minor,
                    patch,
                } => Some(UnrealVersion {
                    major: *major,
                    minor: *minor,
                    patch: Some(*patch),
                }),
                UnrealDetection::MajorMinor { major, minor } => Some(UnrealVersion {
                    major: *major,
                    minor: *minor,
                    patch: None,
                }),
                UnrealDetection::Generation { major: 3 } => Some(UnrealVersion {
                    major: 3,
                    minor: 0,
                    patch: None,
                }),
                _ => None,
            },
            EngineDetection::UnknownEngine { .. } => None,
        };
        let is_unreal = matches!(report.engine, EngineDetection::Unreal(_));
        let engine = if is_unreal {
            Some(Engine::Unreal)
        } else {
            None
        };
        let engine_det = Some(report.engine);
        return (engine, unreal_version, engine_det, platform);
    }

    (None, None, None, None)
}

fn has_unity_data_dir(install_dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(install_dir) else {
        return false;
    };
    entries.flatten().any(|entry| {
        entry.file_type().is_ok_and(|kind| kind.is_dir())
            && entry.file_name().to_str().is_some_and(|name| {
                name.as_bytes()
                    .get(name.len().saturating_sub(5)..)
                    .is_some_and(|suffix| suffix.eq_ignore_ascii_case(b"_data"))
            })
    })
}
