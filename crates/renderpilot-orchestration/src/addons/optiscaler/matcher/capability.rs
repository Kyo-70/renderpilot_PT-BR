//! Compatibility-catalog-backed OptiScaler profile visibility policy.
//!
//! Catalog visibility answers a different question from install planning. It
//! must remain available when the remote compatibility catalog is offline, so
//! this policy uses only immutable executable facts, detected components, and
//! a deliberately small set of exact code-owned exceptions. Release metadata,
//! archive hashes, and configuration migrations remain manifest-backed in the
//! lifecycle paths.

use renderpilot_domain::{Architecture, GraphicsApi, LibraryComponent};

use super::super::compatibility_catalog::{self, OptiScalerCompatibilityCatalog};
use crate::addons::matching::MatchFacts;

/// Returns whether OptiScaler should be visible for a game profile.
///
/// Visibility requires the hard platform gates. The compatibility catalogue
/// can refine the card's install state, but an absent catalogue entry must not
/// hide a supported game. An explicit unsupported rule is the one catalogue
/// decision that suppresses a fresh capability, even when component scanning
/// found a plausible input technology.
pub(crate) fn capability_available(
    catalog: &OptiScalerCompatibilityCatalog,
    facts: &MatchFacts,
    _components: &[LibraryComponent],
) -> bool {
    if !base_requirements_met(facts) {
        return false;
    }
    match compatibility_catalog::resolve(catalog, facts) {
        compatibility_catalog::ResolvedCompatibility::Match { entry, .. } => !matches!(
            entry.status,
            compatibility_catalog::CompatibilityStatus::Unsupported
        ),
        compatibility_catalog::ResolvedCompatibility::Conflict
        | compatibility_catalog::ResolvedCompatibility::NoMatch => true,
    }
}

fn base_requirements_met(facts: &MatchFacts) -> bool {
    facts.graphics.architecture() == Some(Architecture::X64)
        && facts.graphics.apis().iter().any(|api| {
            matches!(
                api,
                GraphicsApi::D3D11 | GraphicsApi::D3D12 | GraphicsApi::Vulkan
            )
        })
}
