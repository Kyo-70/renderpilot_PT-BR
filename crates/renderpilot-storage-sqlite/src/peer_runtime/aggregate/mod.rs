//! Storage-owned value contract for coordinated game aggregate commits.
//!
//! It carries only the common game participants and generation fence used by
//! OptiScaler and the retained peer routes; strict FilePeer endpoint programs
//! are deliberately not represented here.

mod model;

pub use model::{
    AggregateAfter, AggregateBefore, AggregateGeneration, CommittedAggregate,
    GameAggregateMutation, MAX_METADATA_BYTES, PlannedAggregateAfter,
};
