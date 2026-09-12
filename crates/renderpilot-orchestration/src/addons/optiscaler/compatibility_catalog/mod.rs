//! Independently admitted OptiScaler compatibility knowledge.
//!
//! Release bytes and their immutable history live in `manifest_store`.  This
//! catalogue deliberately contains only revisable knowledge about individual
//! games: it can correct compatibility policy without changing release
//! identity or recovery semantics.

mod model;
mod resolution;
mod store;
mod validation;

pub(crate) use model::{
    CatalogGuidance, CompatibilityEntry, CompatibilityPrerequisite, CompatibilityStatus,
    DeclaredInput, EntryIdentityKind, LaunchRequirement, OptiPatcherPolicy,
    OptiScalerCompatibilityCatalog, ProxyPolicy, ResolvedCompatibility, ResolvedVariant,
    WireLaunchPolicy,
};
pub(crate) use resolution::resolve;
pub(crate) use store::get_or_fetch_catalog;
#[cfg(test)]
pub(crate) use store::parse_catalog;
