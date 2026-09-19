//! Shared, fail-closed Unreal `Engine.ini` configuration mechanics.
//!
//! This module is deliberately independent from RenoDX and Luma.  A tool may
//! expose a typed recipe, but only the typed recipe can authorize a mutation;
//! rendered guidance text is never parsed.  The editor operates on byte spans
//! so comments, ordering, unrelated values, line endings, and foreign edits
//! remain intact.

pub mod publication;
pub mod service;

mod document;
mod editor;
mod recipes;
mod resolution;

pub(crate) use recipes::ascii_key;
pub use recipes::{
    EngineIniEntry, EngineIniRecipe, EngineIniRecipeSet, EngineIniSource, IniEncoding,
};
pub(crate) use resolution::same_path_identity;
pub use resolution::{EngineIniResolution, UNREAL_CONFIG_PLATFORMS, resolve_unreal_engine_ini};

#[cfg(test)]
pub(super) use document::encode_text;
pub(crate) use editor::reconcile_pending_transition;
pub use editor::{
    EngineIniContribution, EngineIniEdit, EngineIniError, EngineIniReceipt, EngineIniRecovery,
    EngineIniRelease, apply_engine_ini, reconcile_engine_ini, release_engine_ini_report,
};

#[cfg(test)]
mod tests;
