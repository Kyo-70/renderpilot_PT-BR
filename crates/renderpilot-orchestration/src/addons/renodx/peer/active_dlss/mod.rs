//! Pure active-topology RenoDX DLSS-Fix projection.
//!
//! This boundary consumes only sealed records, endpoint snapshots, and
//! prepared bytes. It emits a closed endpoint program or an exact claim-only
//! projection; filesystem, storage, and command orchestration remain outside.

mod compose;
mod effects;
mod error;
mod model;

#[cfg(test)]
mod tests;

pub(crate) use compose::compose_active_dlss;
pub(crate) use error::ActiveDlssError;
pub(crate) use model::{
    ActiveDlssComposition, ActiveDlssEffect, ActiveDlssEndpointInput, ActiveDlssInput,
};
