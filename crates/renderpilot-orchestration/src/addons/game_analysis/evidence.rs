//! Validated evidence with guaranteed origin authenticity and privacy redaction.

use std::marker::PhantomData;
use std::path::{Path, PathBuf};

use crate::addons::game_analysis::context::{GameInstallationContext, InstallationId};
use crate::addons::game_analysis::parsers::tokens::{
    HelperRole, ParsedBuildVersion, ParsedCanonicalReleaseMarker, ParsedPdbPath,
    ParsedProjectDescriptor, ParsedTargetVersion, PrimaryRole, VersionClaim,
};

/// Origin source of evidence (generic PE VERSIONINFO is strictly excluded).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EvidenceSource {
    BuildVersionFile,
    TargetVersionFile,
    CanonicalReleaseMarker,
    CodeViewPdbPath,
    ProjectDescriptorFile,
}

/// Scope / domain of evidence discovery in an installation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EvidenceScope {
    PrimaryExecutable,
    EngineHelper,
    EngineMetadata,
    ProjectMetadata,
}

/// 4-tier hierarchy of evidence authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Authority {
    Weak = 1,
    Supporting = 2,
    Strong = 3,
    Authoritative = 4,
}

/// Validated evidence holding guaranteed origin authenticity.
///
/// Fields are private and constructible exclusively through typed factories.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ValidatedEvidence<'game> {
    installation_id: InstallationId,
    claim: VersionClaim,
    source: EvidenceSource,
    scope: EvidenceScope,
    file_path: PathBuf,
    file_offset: u64,
    _game: PhantomData<&'game GameInstallationContext>,
}

impl<'game> ValidatedEvidence<'game> {
    #[must_use]
    pub fn installation_id(&self) -> &InstallationId {
        &self.installation_id
    }

    #[must_use]
    pub fn claim(&self) -> VersionClaim {
        self.claim
    }

    #[must_use]
    pub fn source(&self) -> EvidenceSource {
        self.source
    }

    #[must_use]
    pub fn scope(&self) -> EvidenceScope {
        self.scope
    }

    #[must_use]
    pub fn file_path(&self) -> &Path {
        &self.file_path
    }

    #[must_use]
    pub fn file_offset(&self) -> u64 {
        self.file_offset
    }

    #[must_use]
    pub fn authority(&self) -> Authority {
        crate::addons::game_analysis::authority::authority_for(self.source, self.scope)
            .expect("ValidatedEvidence holds only structurally valid (source, scope) pairs")
    }

    pub fn from_primary_marker(token: ParsedCanonicalReleaseMarker<'game, PrimaryRole>) -> Self {
        let (installation_id, file_path, file_offset, claim, _raw) = token.into_parts();
        Self {
            installation_id,
            claim,
            source: EvidenceSource::CanonicalReleaseMarker,
            scope: EvidenceScope::PrimaryExecutable,
            file_path,
            file_offset,
            _game: PhantomData,
        }
    }

    pub fn from_helper_marker(token: ParsedCanonicalReleaseMarker<'game, HelperRole>) -> Self {
        let (installation_id, file_path, file_offset, claim, _raw) = token.into_parts();
        Self {
            installation_id,
            claim,
            source: EvidenceSource::CanonicalReleaseMarker,
            scope: EvidenceScope::EngineHelper,
            file_path,
            file_offset,
            _game: PhantomData,
        }
    }

    pub fn from_build_version(token: ParsedBuildVersion<'game>) -> Self {
        let (installation_id, file_path, claim) = token.into_parts();
        Self {
            installation_id,
            claim,
            source: EvidenceSource::BuildVersionFile,
            scope: EvidenceScope::EngineMetadata,
            file_path,
            file_offset: 0,
            _game: PhantomData,
        }
    }

    pub fn from_target_version(token: ParsedTargetVersion<'game>) -> Self {
        let (installation_id, file_path, claim) = token.into_parts();
        Self {
            installation_id,
            claim,
            source: EvidenceSource::TargetVersionFile,
            scope: EvidenceScope::PrimaryExecutable,
            file_path,
            file_offset: 0,
            _game: PhantomData,
        }
    }

    pub fn from_primary_pdb(token: ParsedPdbPath<'game, PrimaryRole>) -> (Self, String) {
        let (installation_id, file_path, file_offset, claim, pdb_path_fragment) =
            token.into_parts();
        (
            Self {
                installation_id,
                claim,
                source: EvidenceSource::CodeViewPdbPath,
                scope: EvidenceScope::PrimaryExecutable,
                file_path,
                file_offset,
                _game: PhantomData,
            },
            pdb_path_fragment,
        )
    }

    pub fn from_helper_pdb(token: ParsedPdbPath<'game, HelperRole>) -> (Self, String) {
        let (installation_id, file_path, file_offset, claim, pdb_path_fragment) =
            token.into_parts();
        (
            Self {
                installation_id,
                claim,
                source: EvidenceSource::CodeViewPdbPath,
                scope: EvidenceScope::EngineHelper,
                file_path,
                file_offset,
                _game: PhantomData,
            },
            pdb_path_fragment,
        )
    }

    pub fn from_project_descriptor(token: ParsedProjectDescriptor<'game>) -> Self {
        let (installation_id, file_path, claim) = token.into_parts();
        Self {
            installation_id,
            claim,
            source: EvidenceSource::ProjectDescriptorFile,
            scope: EvidenceScope::ProjectMetadata,
            file_path,
            file_offset: 0,
            _game: PhantomData,
        }
    }

    #[cfg(test)]
    pub fn synthetic(
        installation_id: InstallationId,
        claim: VersionClaim,
        source: EvidenceSource,
        scope: EvidenceScope,
        file_path: PathBuf,
        file_offset: u64,
    ) -> Self {
        Self {
            installation_id,
            claim,
            source,
            scope,
            file_path,
            file_offset,
            _game: PhantomData,
        }
    }
}
