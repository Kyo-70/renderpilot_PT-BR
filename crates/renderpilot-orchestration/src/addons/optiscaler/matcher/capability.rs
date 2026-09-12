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
use super::compatibility::is_input_technology;
use crate::addons::matching::MatchFacts;

/// Returns whether OptiScaler should be visible for a game profile.
///
/// Visibility requires all hard platform gates and then either an exact local
/// compatibility rule or one of the supported graphics-input technologies
/// detected for this game. Explicit unsupported rules are evaluated first and
/// therefore cannot be bypassed by positive evidence.
pub(crate) fn capability_available(
    catalog: &OptiScalerCompatibilityCatalog,
    facts: &MatchFacts,
    components: &[LibraryComponent],
) -> bool {
    if !base_requirements_met(facts) {
        return false;
    }
    match compatibility_catalog::resolve(catalog, facts) {
        compatibility_catalog::ResolvedCompatibility::Match { entry, .. }
            if !matches!(
                entry.status,
                compatibility_catalog::CompatibilityStatus::Unsupported
            ) =>
        {
            true
        }
        compatibility_catalog::ResolvedCompatibility::Match { .. }
        | compatibility_catalog::ResolvedCompatibility::Conflict => components
            .iter()
            .any(|component| is_input_technology(component.technology())),
        compatibility_catalog::ResolvedCompatibility::NoMatch => components
            .iter()
            .any(|component| is_input_technology(component.technology())),
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
