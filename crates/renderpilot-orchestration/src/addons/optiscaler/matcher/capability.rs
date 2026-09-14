//! Compatibility-catalog-backed OptiScaler profile visibility policy.
//!
//! Visibility is evidence-driven: a supported input component is sufficient,
//! and any exact compatibility-catalog result keeps the profile visible so
//! the UI can present the catalog-authored status. Executable architecture and
//! imported graphics APIs are not availability evidence.

use renderpilot_domain::LibraryComponent;

use super::super::compatibility_catalog::{self, OptiScalerCompatibilityCatalog};
use crate::addons::matching::MatchFacts;

/// Returns whether OptiScaler should be visible for a game profile.
///
/// A supported input or any exact catalog result makes the profile visible.
///
/// A catalog match remains visible even when its status is `unsupported` (or
/// when identity resolution reports a conflict), because compatibility is
/// catalog-authored and the UI must be able to show that result. A no-match
/// game is visible only when component scanning found a supported input.
pub(crate) fn capability_available(
    catalog: &OptiScalerCompatibilityCatalog,
    facts: &MatchFacts,
    components: &[LibraryComponent],
) -> bool {
    let has_supported_input = components
        .iter()
        .any(|component| super::compatibility::is_input_technology(component.technology()));
    has_supported_input
        || !matches!(
            compatibility_catalog::resolve(catalog, facts),
            compatibility_catalog::ResolvedCompatibility::NoMatch
        )
}
