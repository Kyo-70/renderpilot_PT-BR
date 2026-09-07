//! Durable, metadata-only aggregate transactions.
//!
//! This protocol is deliberately separate from physical peer programs.  A
//! metadata refresh and a reused-claim membership change both update only the
//! installed peer row, but they still reserve the complete game aggregate and
//! advance its generation atomically.

mod cleanup;
mod commit;
mod model;
mod preparation;
mod validation;

pub use model::{
    CommittedMetadataAggregate, MetadataAggregatePreparation, MetadataAggregateTransition,
    PreparedMetadataAggregateCommitPermit,
};

#[cfg(test)]
mod tests;
