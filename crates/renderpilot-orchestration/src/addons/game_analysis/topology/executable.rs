//! Verified executable bindings: Primary Executable and Engine Helpers.

use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};

#[cfg(windows)]
use std::os::windows::fs::OpenOptionsExt;
#[cfg(windows)]
use windows_sys::Win32::Storage::FileSystem::{
    FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
};

use renderpilot_detection::pe::PeSectionHeader;

use crate::addons::game_analysis::context::GameInstallationContext;

/// Target platform architecture detection verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TargetPlatformDetection {
    /// Supported Windows 64-bit AMD64 (PE32+, Machine 0x8664).
    Win64Amd64,
    /// Supported Windows 32-bit x86 (PE32, Machine 0x014C).
    Win32X86,
    /// Unsupported target architecture (e.g. ARM64, Itanium).
    Unsupported { machine: u16, is_64bit: bool },
    /// Unknown target platform architecture (e.g. unreadable executable or malformed PE).
    Unknown,
}

#[derive(Debug, PartialEq, Eq)]
pub enum TopologyError {
    OutsideInstallationContext(PathBuf),
    EngineBinariesDisallowed(PathBuf),
    EngineServiceHelperDisallowed(PathBuf),
    SymlinkDisallowed(PathBuf),
    NotInsideEngineBinaries(PathBuf),
    InvalidMetadataLocation(PathBuf),
    InvalidExtension(PathBuf),
    Io {
        kind: io::ErrorKind,
        message: String,
    },
    MalformedPe(String),
    UnsupportedArchitecture {
        machine: u16,
        is_64bit: bool,
    },
    ArchitectureMismatch {
        expected: renderpilot_domain::Architecture,
        actual: renderpilot_domain::Architecture,
    },
}

impl From<io::Error> for TopologyError {
    fn from(err: io::Error) -> Self {
        Self::Io {
            kind: err.kind(),
            message: err.to_string(),
        }
    }
}

pub(crate) fn open_shared_file(path: &Path) -> io::Result<File> {
    #[cfg(windows)]
    {
        std::fs::OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
            .open(path)
    }
    #[cfg(not(windows))]
    {
        File::open(path)
    }
}

/// Checks if an executable path is located inside `<game_root>/Engine/Binaries/...`.
pub fn is_inside_engine_binaries(path: &Path, root: &Path) -> bool {
    if let Ok(rel) = path.strip_prefix(root) {
        let mut comps = rel.components().map(|c| c.as_os_str().to_str());
        if let (Some(Some(first)), Some(Some(second))) = (comps.next(), comps.next()) {
            return first.eq_ignore_ascii_case("engine") && second.eq_ignore_ascii_case("binaries");
        }
    }
    false
}

/// Checks if a file is a backup sidecar created by external tools or D3D12 mutation guard.
pub fn is_backup_sidecar(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| {
            ext.eq_ignore_ascii_case("bak")
                || ext.eq_ignore_ascii_case("old")
                || ext.eq_ignore_ascii_case("orig")
                || ext.eq_ignore_ascii_case("rp-backup")
        })
}

/// Checks if an executable is a known Unreal service helper that must never be bound as Primary.
pub fn is_known_service_helper(file_name: &str) -> bool {
    let stem = Path::new(file_name)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(file_name);
    let stem_bytes = stem.as_bytes();
    if stem_bytes
        .get(..17)
        .is_some_and(|p| p.eq_ignore_ascii_case(b"crashreportclient"))
    {
        return true;
    }
    stem.eq_ignore_ascii_case("unrealcefsubprocess") || stem.eq_ignore_ascii_case("epicwebhelper")
}

/// Topologically verified Primary Executable of the game.
pub struct BoundPrimaryExecutable<'game> {
    context: &'game GameInstallationContext,
    canonical_path: PathBuf,
    file: File,
    header_info: renderpilot_detection::pe::PeHeaderInfo,
    architecture: renderpilot_domain::Architecture,
}

impl<'game> BoundPrimaryExecutable<'game> {
    /// Proof constructor: accepts exclusively the global SSOT `ResolvedExecutable`.
    ///
    /// Canonicalizes the path, verifies installation context boundaries, disallows
    /// known engine service helpers inside Engine/Binaries, opens with shared access,
    /// parses PE headers, and verifies target architecture.
    pub fn from_resolved(
        context: &'game GameInstallationContext,
        resolved: &crate::game_executable::ResolvedExecutable,
    ) -> Result<Self, TopologyError> {
        let path = Path::new(resolved.path.as_str());
        if is_backup_sidecar(path) {
            return Err(TopologyError::Io {
                kind: io::ErrorKind::InvalidInput,
                message: "Backup sidecar disallowed as Primary".to_string(),
            });
        }

        let canonical_path = std::fs::canonicalize(path)?;
        if !canonical_path.starts_with(context.root_path()) {
            return Err(TopologyError::OutsideInstallationContext(canonical_path));
        }

        let in_engine_binaries = is_inside_engine_binaries(&canonical_path, context.root_path());
        let file_name = canonical_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        if in_engine_binaries && is_known_service_helper(file_name) {
            return Err(TopologyError::EngineServiceHelperDisallowed(canonical_path));
        }

        let mut file = open_shared_file(&canonical_path)?;

        let header_info = renderpilot_detection::pe::parse_pe_headers(&mut file)
            .map_err(|e| TopologyError::MalformedPe(e.to_string()))?;

        let architecture = renderpilot_detection::pe::validate_pe_target_architecture(&header_info)
            .map_err(|e| match e {
                renderpilot_detection::pe::TargetPlatformError::UnsupportedArchitecture {
                    machine,
                    is_64bit,
                } => TopologyError::UnsupportedArchitecture { machine, is_64bit },
            })?;

        Ok(Self {
            context,
            canonical_path,
            file,
            header_info,
            architecture,
        })
    }

    #[must_use]
    pub fn context(&self) -> &'game GameInstallationContext {
        self.context
    }

    #[must_use]
    pub fn architecture(&self) -> renderpilot_domain::Architecture {
        self.architecture
    }

    #[must_use]
    pub fn target_platform(&self) -> TargetPlatformDetection {
        match self.architecture {
            renderpilot_domain::Architecture::X64 => TargetPlatformDetection::Win64Amd64,
            renderpilot_domain::Architecture::X86 => TargetPlatformDetection::Win32X86,
        }
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.canonical_path
    }

    pub fn scan_parts(
        &mut self,
    ) -> (
        &mut File,
        &renderpilot_detection::pe::PeHeaderInfo,
        &[PeSectionHeader],
        &Path,
    ) {
        (
            &mut self.file,
            &self.header_info,
            &self.header_info.sections,
            &self.canonical_path,
        )
    }
}

/// Topologically verified Engine Helper executable.
pub struct BoundEngineHelper<'game> {
    context: &'game GameInstallationContext,
    canonical_path: PathBuf,
    file: File,
    header_info: renderpilot_detection::pe::PeHeaderInfo,
}

impl<'game> BoundEngineHelper<'game> {
    pub fn open(
        context: &'game GameInstallationContext,
        path: &Path,
        expected_architecture: renderpilot_domain::Architecture,
    ) -> Result<Self, TopologyError> {
        if is_backup_sidecar(path) {
            return Err(TopologyError::Io {
                kind: io::ErrorKind::InvalidInput,
                message: "Backup sidecar disallowed as helper".to_string(),
            });
        }

        let canonical_path = std::fs::canonicalize(path)?;
        if !canonical_path.starts_with(context.root_path()) {
            return Err(TopologyError::OutsideInstallationContext(canonical_path));
        }
        if !is_inside_engine_binaries(&canonical_path, context.root_path()) {
            return Err(TopologyError::NotInsideEngineBinaries(canonical_path));
        }

        let mut file = open_shared_file(&canonical_path)?;

        let header_info = renderpilot_detection::pe::parse_pe_headers(&mut file)
            .map_err(|e| TopologyError::MalformedPe(e.to_string()))?;

        let architecture = renderpilot_detection::pe::validate_pe_target_architecture(&header_info)
            .map_err(|e| match e {
                renderpilot_detection::pe::TargetPlatformError::UnsupportedArchitecture {
                    machine,
                    is_64bit,
                } => TopologyError::UnsupportedArchitecture { machine, is_64bit },
            })?;

        if architecture != expected_architecture {
            return Err(TopologyError::ArchitectureMismatch {
                expected: expected_architecture,
                actual: architecture,
            });
        }

        Ok(Self {
            context,
            canonical_path,
            file,
            header_info,
        })
    }

    #[must_use]
    pub fn context(&self) -> &'game GameInstallationContext {
        self.context
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.canonical_path
    }

    pub fn scan_parts(
        &mut self,
    ) -> (
        &mut File,
        &renderpilot_detection::pe::PeHeaderInfo,
        &[PeSectionHeader],
        &Path,
    ) {
        (
            &mut self.file,
            &self.header_info,
            &self.header_info.sections,
            &self.canonical_path,
        )
    }
}

/// Helper discovery: discovers and prioritizes engine helper candidates matching Primary architecture.
pub fn discover_engine_helpers<'game>(
    context: &'game GameInstallationContext,
    primary_arch: renderpilot_domain::Architecture,
    max_helpers: usize,
    exclude_canonical: Option<&Path>,
) -> Vec<BoundEngineHelper<'game>> {
    let subfolder = match primary_arch {
        renderpilot_domain::Architecture::X64 => "Win64",
        renderpilot_domain::Architecture::X86 => "Win32",
    };

    let binaries_dir = context
        .root_path()
        .join("Engine")
        .join("Binaries")
        .join(subfolder);

    let entries = match std::fs::read_dir(&binaries_dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };

    let mut tier1 = Vec::new();
    let mut tier2 = Vec::new();
    let mut tier3 = Vec::new();

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() || is_backup_sidecar(&path) {
            continue;
        }
        if !path
            .extension()
            .and_then(std::ffi::OsStr::to_str)
            .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
        {
            continue;
        }

        let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

        let starts_with_crc = file_name
            .as_bytes()
            .get(..17)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(b"crashreportclient"));
        if starts_with_crc {
            tier1.push(path);
        } else if file_name.eq_ignore_ascii_case("unrealcefsubprocess.exe")
            || file_name.eq_ignore_ascii_case("epicwebhelper.exe")
        {
            tier2.push(path);
        } else {
            tier3.push(path);
        }
    }

    tier1.sort();
    tier2.sort();
    tier3.sort();

    let mut helpers = Vec::new();
    let candidates = tier1.into_iter().chain(tier2).chain(tier3);

    for candidate in candidates {
        if helpers.len() >= max_helpers {
            break;
        }
        let Ok(canonical) = std::fs::canonicalize(&candidate) else {
            continue;
        };
        if exclude_canonical.is_some_and(|ex| canonical == ex) {
            continue;
        }
        if let Ok(helper) = BoundEngineHelper::open(context, &candidate, primary_arch) {
            helpers.push(helper);
        }
    }

    helpers
}
