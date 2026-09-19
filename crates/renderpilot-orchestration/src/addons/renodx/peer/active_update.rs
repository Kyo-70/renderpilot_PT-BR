//! Pure active-topology RenoDX update composition.
//!
//! This boundary consumes only immutable records, topology, sealed endpoint
//! observations, and prepared bytes. It never reads storage or the filesystem,
//! and never performs a mutation. Runtime code owns the later execution of the
//! returned exact program.

mod compose;
mod error;
mod model;
mod paths;
mod record;
mod validation;

#[cfg(test)]
mod tests;

pub(crate) use compose::compose_active_update;
pub(crate) use model::{
    RenoDxActiveUpdateComposition, RenoDxActiveUpdateConfigInput, RenoDxActiveUpdateHostInput,
    RenoDxActiveUpdateInput,
};
