//! Centralized authority matrix policy (v1 without wildcards).

use crate::addons::game_analysis::evidence::{Authority, EvidenceScope, EvidenceSource};

/// Total pure function evaluating the authority tier of an evidence source and scope pair.
///
/// Every structurally invalid pair is explicitly mapped to `None` without wildcard fallback.
#[must_use]
pub const fn authority_for(source: EvidenceSource, scope: EvidenceScope) -> Option<Authority> {
    match (source, scope) {
        // Tier 4: Authoritative metadata sources
        (EvidenceSource::BuildVersionFile, EvidenceScope::EngineMetadata)
        | (EvidenceSource::TargetVersionFile, EvidenceScope::PrimaryExecutable) => {
            Some(Authority::Authoritative)
        }

        // Tier 3: Strong sources from the primary executable
        (EvidenceSource::CanonicalReleaseMarker, EvidenceScope::PrimaryExecutable) => {
            Some(Authority::Strong)
        }
        (EvidenceSource::CodeViewPdbPath, EvidenceScope::PrimaryExecutable) => {
            Some(Authority::Strong)
        }

        // Tier 2: Supporting sources from engine helpers
        (EvidenceSource::CanonicalReleaseMarker, EvidenceScope::EngineHelper) => {
            Some(Authority::Supporting)
        }

        // Tier 1: Weak / indirect markers
        (EvidenceSource::ProjectDescriptorFile, EvidenceScope::ProjectMetadata) => {
            Some(Authority::Weak)
        }
        (EvidenceSource::CodeViewPdbPath, EvidenceScope::EngineHelper) => Some(Authority::Weak),

        // Structurally prohibited combinations
        (EvidenceSource::BuildVersionFile, EvidenceScope::PrimaryExecutable)
        | (EvidenceSource::BuildVersionFile, EvidenceScope::EngineHelper)
        | (EvidenceSource::BuildVersionFile, EvidenceScope::ProjectMetadata)
        | (EvidenceSource::TargetVersionFile, EvidenceScope::EngineHelper)
        | (EvidenceSource::TargetVersionFile, EvidenceScope::EngineMetadata)
        | (EvidenceSource::TargetVersionFile, EvidenceScope::ProjectMetadata)
        | (EvidenceSource::CanonicalReleaseMarker, EvidenceScope::EngineMetadata)
        | (EvidenceSource::CanonicalReleaseMarker, EvidenceScope::ProjectMetadata)
        | (EvidenceSource::CodeViewPdbPath, EvidenceScope::EngineMetadata)
        | (EvidenceSource::CodeViewPdbPath, EvidenceScope::ProjectMetadata)
        | (EvidenceSource::ProjectDescriptorFile, EvidenceScope::PrimaryExecutable)
        | (EvidenceSource::ProjectDescriptorFile, EvidenceScope::EngineHelper)
        | (EvidenceSource::ProjectDescriptorFile, EvidenceScope::EngineMetadata) => None,
    }
}
