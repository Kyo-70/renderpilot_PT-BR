//! Role-typed tokens ensuring proven source origin and precision.

use std::marker::PhantomData;
use std::path::{Path, PathBuf};

use crate::addons::game_analysis::context::{GameInstallationContext, InstallationId};
use crate::addons::game_analysis::topology::executable::{
    BoundEngineHelper, BoundPrimaryExecutable,
};
use crate::addons::game_analysis::topology::metadata::{
    BoundMetadataText, EngineMetadataScope, ProjectMetadataScope, TargetMetadataScope,
};

pub const MAX_MARKER_LEN: usize = 64;
// SSOT: re-export scanner buffer and overlap constants directly from detection PE scanner.
pub use renderpilot_detection::pe::{OVERLAP_SIZE, STREAM_CHUNK_SIZE};
// SSOT: re-export section streaming budget from budget specification.
pub use crate::addons::game_analysis::budget::MAX_STREAM_READ_PER_INSTALLATION as VERSION_SECTION_SCAN_BUDGET;

const _: () = assert!(OVERLAP_SIZE > MAX_MARKER_LEN);
const _: () = assert!(STREAM_CHUNK_SIZE >= OVERLAP_SIZE);
const _: () = assert!(VERSION_SECTION_SCAN_BUDGET >= STREAM_CHUNK_SIZE as u64);

/// Immutable, guaranteed valid UTF-8 slice of raw marker bytes up to 64 bytes.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BoundedMarker {
    bytes: [u8; MAX_MARKER_LEN],
    len: u8,
}

impl BoundedMarker {
    pub fn try_from_bytes(slice: &[u8]) -> Option<Self> {
        if slice.is_empty() || slice.len() > MAX_MARKER_LEN {
            return None;
        }
        std::str::from_utf8(slice).ok()?;
        let mut bytes = [0u8; MAX_MARKER_LEN];
        bytes[..slice.len()].copy_from_slice(slice);
        Some(Self {
            bytes,
            len: slice.len() as u8,
        })
    }
}

/// Version claim extracted from an installation artifact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum VersionClaim {
    Generation { major: u32 },
    MajorMinor { major: u32, minor: u32 },
    Exact { major: u32, minor: u32, patch: u32 },
}

impl VersionClaim {
    #[must_use]
    pub const fn major(&self) -> u32 {
        match *self {
            Self::Generation { major }
            | Self::MajorMinor { major, .. }
            | Self::Exact { major, .. } => major,
        }
    }

    #[must_use]
    pub const fn minor(&self) -> Option<u32> {
        match *self {
            Self::Generation { .. } => None,
            Self::MajorMinor { minor, .. } | Self::Exact { minor, .. } => Some(minor),
        }
    }

    #[must_use]
    pub const fn patch(&self) -> Option<u32> {
        match *self {
            Self::Generation { .. } | Self::MajorMinor { .. } => None,
            Self::Exact { patch, .. } => Some(patch),
        }
    }
}

pub trait ExecutableRole: private::Sealed {}

pub struct PrimaryRole;
impl ExecutableRole for PrimaryRole {}
pub struct HelperRole;
impl ExecutableRole for HelperRole {}

mod private {
    pub trait Sealed {}
    impl Sealed for super::PrimaryRole {}
    impl Sealed for super::HelperRole {}
}

/// Token of a canonical release marker (`++UE4+Release-X.Y` or `++UE5+Release-X.Y`).
#[derive(Debug, Clone)]
pub struct ParsedCanonicalReleaseMarker<'game, Role: ExecutableRole> {
    installation_id: InstallationId,
    file_path: PathBuf,
    file_offset: u64,
    claim: VersionClaim,
    raw: BoundedMarker,
    _role: PhantomData<(&'game GameInstallationContext, Role)>,
}

impl<'game> ParsedCanonicalReleaseMarker<'game, PrimaryRole> {
    pub(in crate::addons::game_analysis::parsers) fn from_primary(
        source: &BoundPrimaryExecutable<'game>,
        file_offset: u64,
        major: u32,
        minor: u32,
        patch: Option<u32>,
        raw: BoundedMarker,
    ) -> Self {
        let claim = match patch {
            Some(p) => VersionClaim::Exact {
                major,
                minor,
                patch: p,
            },
            None => VersionClaim::MajorMinor { major, minor },
        };
        Self {
            installation_id: source.context().id().clone(),
            file_path: source.path().to_path_buf(),
            file_offset,
            claim,
            raw,
            _role: PhantomData,
        }
    }
}

impl<'game> ParsedCanonicalReleaseMarker<'game, HelperRole> {
    pub(in crate::addons::game_analysis::parsers) fn from_helper(
        source: &BoundEngineHelper<'game>,
        file_offset: u64,
        major: u32,
        minor: u32,
        patch: Option<u32>,
        raw: BoundedMarker,
    ) -> Self {
        let claim = match patch {
            Some(p) => VersionClaim::Exact {
                major,
                minor,
                patch: p,
            },
            None => VersionClaim::MajorMinor { major, minor },
        };
        Self {
            installation_id: source.context().id().clone(),
            file_path: source.path().to_path_buf(),
            file_offset,
            claim,
            raw,
            _role: PhantomData,
        }
    }
}

impl<'game, Role: ExecutableRole> ParsedCanonicalReleaseMarker<'game, Role> {
    pub(in crate::addons::game_analysis) fn into_parts(
        self,
    ) -> (InstallationId, PathBuf, u64, VersionClaim, BoundedMarker) {
        (
            self.installation_id,
            self.file_path,
            self.file_offset,
            self.claim,
            self.raw,
        )
    }
}

/// Token of a parsed Build.version file.
#[derive(Debug, Clone)]
pub struct ParsedBuildVersion<'game> {
    installation_id: InstallationId,
    file_path: PathBuf,
    claim: VersionClaim,
    _marker: PhantomData<&'game GameInstallationContext>,
}

impl<'game> ParsedBuildVersion<'game> {
    pub(in crate::addons::game_analysis::parsers) fn from_source(
        source: &BoundMetadataText<'game, EngineMetadataScope>,
        major: u32,
        minor: u32,
        patch: Option<u32>,
    ) -> Self {
        let claim = match patch {
            Some(p) => VersionClaim::Exact {
                major,
                minor,
                patch: p,
            },
            None => VersionClaim::MajorMinor { major, minor },
        };
        Self {
            installation_id: source.context().id().clone(),
            file_path: source.path().to_path_buf(),
            claim,
            _marker: PhantomData,
        }
    }

    pub(in crate::addons::game_analysis) fn into_parts(
        self,
    ) -> (InstallationId, PathBuf, VersionClaim) {
        (self.installation_id, self.file_path, self.claim)
    }
}

/// Token of a parsed Primary companion .version file.
#[derive(Debug, Clone)]
pub struct ParsedTargetVersion<'game> {
    installation_id: InstallationId,
    file_path: PathBuf,
    claim: VersionClaim,
    _marker: PhantomData<&'game GameInstallationContext>,
}

impl<'game> ParsedTargetVersion<'game> {
    pub(in crate::addons::game_analysis::parsers) fn from_source(
        source: &BoundMetadataText<'game, TargetMetadataScope>,
        major: u32,
        minor: u32,
        patch: u32,
    ) -> Self {
        Self {
            installation_id: source.context().id().clone(),
            file_path: source.path().to_path_buf(),
            claim: VersionClaim::Exact {
                major,
                minor,
                patch,
            },
            _marker: PhantomData,
        }
    }

    pub(in crate::addons::game_analysis) fn into_parts(
        self,
    ) -> (InstallationId, PathBuf, VersionClaim) {
        (self.installation_id, self.file_path, self.claim)
    }
}

/// Token of CodeView PDB path.
#[derive(Debug, Clone)]
pub struct ParsedPdbPath<'game, Role: ExecutableRole> {
    installation_id: InstallationId,
    file_path: PathBuf,
    file_offset: u64,
    claim: VersionClaim,
    pdb_path_fragment: String,
    _role: PhantomData<(&'game GameInstallationContext, Role)>,
}

impl<'game> ParsedPdbPath<'game, PrimaryRole> {
    pub(in crate::addons::game_analysis::parsers) fn from_primary(
        source: &BoundPrimaryExecutable<'game>,
        file_offset: u64,
        major: u32,
        minor: u32,
        pdb_path_fragment: String,
    ) -> Self {
        Self {
            installation_id: source.context().id().clone(),
            file_path: source.path().to_path_buf(),
            file_offset,
            claim: VersionClaim::MajorMinor { major, minor },
            pdb_path_fragment,
            _role: PhantomData,
        }
    }
}

impl<'game> ParsedPdbPath<'game, HelperRole> {
    pub(in crate::addons::game_analysis::parsers) fn from_helper(
        source: &BoundEngineHelper<'game>,
        file_offset: u64,
        major: u32,
        minor: u32,
        pdb_path_fragment: String,
    ) -> Self {
        Self {
            installation_id: source.context().id().clone(),
            file_path: source.path().to_path_buf(),
            file_offset,
            claim: VersionClaim::MajorMinor { major, minor },
            pdb_path_fragment,
            _role: PhantomData,
        }
    }
}

impl<'game, Role: ExecutableRole> ParsedPdbPath<'game, Role> {
    #[must_use]
    pub fn file_path(&self) -> &Path {
        &self.file_path
    }

    pub(in crate::addons::game_analysis) fn into_parts(
        self,
    ) -> (InstallationId, PathBuf, u64, VersionClaim, String) {
        (
            self.installation_id,
            self.file_path,
            self.file_offset,
            self.claim,
            self.pdb_path_fragment,
        )
    }
}

/// Token of a parsed project descriptor (.uproject).
#[derive(Debug, Clone)]
pub struct ParsedProjectDescriptor<'game> {
    installation_id: InstallationId,
    file_path: PathBuf,
    claim: VersionClaim,
    _marker: PhantomData<&'game GameInstallationContext>,
}

impl<'game> ParsedProjectDescriptor<'game> {
    pub(in crate::addons::game_analysis::parsers) fn from_source(
        source: &BoundMetadataText<'game, ProjectMetadataScope>,
        major: u32,
        minor: Option<u32>,
    ) -> Self {
        let claim = match minor {
            Some(m) => VersionClaim::MajorMinor { major, minor: m },
            None => VersionClaim::Generation { major },
        };
        Self {
            installation_id: source.context().id().clone(),
            file_path: source.path().to_path_buf(),
            claim,
            _marker: PhantomData,
        }
    }

    pub(in crate::addons::game_analysis) fn into_parts(
        self,
    ) -> (InstallationId, PathBuf, VersionClaim) {
        (self.installation_id, self.file_path, self.claim)
    }
}
