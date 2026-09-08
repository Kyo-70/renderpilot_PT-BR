//! Durable preparation and commit protocol for an OptiScaler journal mutation.
//!
//! The preparation phase owns the pending journal row, and the commit phase
//! publishes that row together with OptiScaler state, proxy topology, and the
//! optional installed peer under one aggregate generation fence.

mod cas;
mod cleanup;
mod commit;
mod model;
mod preparation;
mod recovery_acquisition;
mod recovery_cas;
mod recovery_cleanup;
mod validation;

#[cfg(test)]
mod tests;

pub use model::{
    CommittedOptiScalerJournalAggregate, OptiScalerJournalAggregateBegin,
    OptiScalerJournalAggregateCommit, PendingFileMutationRecoveryCandidate,
    PreparedOptiScalerJournalAggregate, PreparingOptiScalerJournalAggregate,
    RecoveringOptiScalerJournalAggregate,
};
