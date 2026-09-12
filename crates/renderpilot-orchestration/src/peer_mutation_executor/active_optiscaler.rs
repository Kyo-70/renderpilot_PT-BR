use renderpilot_application::AppResult;

use super::{
    CommittedOptiScalerJournalAggregate, OptiScalerJournalAggregateBegin,
    OptiScalerJournalAggregateCommit, PeerMutationExecutor, PreparedOptiScalerJournalAggregate,
    PreparingOptiScalerJournalAggregate,
};

impl PeerMutationExecutor {
    pub(crate) fn begin_optiscaler_journal_aggregate(
        &self,
        begin: OptiScalerJournalAggregateBegin,
    ) -> AppResult<PreparingOptiScalerJournalAggregate> {
        self.runtime.begin_optiscaler_journal_aggregate(begin)
    }

    pub(crate) fn cas_preparing_optiscaler_journal_aggregate(
        &self,
        preparing: PreparingOptiScalerJournalAggregate,
        next_journal_json: impl Into<String>,
    ) -> AppResult<PreparingOptiScalerJournalAggregate> {
        self.runtime
            .cas_preparing_optiscaler_journal_aggregate(preparing, next_journal_json)
    }

    pub(crate) fn finish_optiscaler_journal_aggregate(
        &self,
        preparing: PreparingOptiScalerJournalAggregate,
        final_journal_json: impl Into<String>,
    ) -> AppResult<PreparedOptiScalerJournalAggregate> {
        self.runtime
            .finish_optiscaler_journal_aggregate(preparing, final_journal_json)
    }

    pub(crate) fn cas_prepared_optiscaler_journal_aggregate(
        &self,
        prepared: PreparedOptiScalerJournalAggregate,
        next_journal_json: impl Into<String>,
    ) -> AppResult<PreparedOptiScalerJournalAggregate> {
        self.runtime
            .cas_prepared_optiscaler_journal_aggregate(prepared, next_journal_json)
    }

    pub(crate) fn commit_optiscaler_journal_aggregate(
        &self,
        prepared: PreparedOptiScalerJournalAggregate,
        commit: OptiScalerJournalAggregateCommit<'_>,
    ) -> AppResult<CommittedOptiScalerJournalAggregate> {
        self.runtime
            .commit_optiscaler_journal_aggregate(prepared, commit)
    }

    pub(crate) fn cas_committed_optiscaler_journal_aggregate(
        &self,
        committed: CommittedOptiScalerJournalAggregate,
        next_journal_json: impl Into<String>,
    ) -> AppResult<CommittedOptiScalerJournalAggregate> {
        self.runtime
            .cas_committed_optiscaler_journal_aggregate(committed, next_journal_json)
    }

    pub(crate) fn delete_preparing_optiscaler_journal_aggregate_after_rollback(
        &self,
        preparing: PreparingOptiScalerJournalAggregate,
    ) -> AppResult<()> {
        self.runtime
            .delete_preparing_optiscaler_journal_aggregate_after_rollback(preparing)
    }

    pub(crate) fn delete_prepared_optiscaler_journal_aggregate_after_rollback(
        &self,
        prepared: PreparedOptiScalerJournalAggregate,
    ) -> AppResult<()> {
        self.runtime
            .delete_prepared_optiscaler_journal_aggregate_after_rollback(prepared)
    }

    pub(crate) fn delete_committed_optiscaler_journal_aggregate(
        &self,
        committed: CommittedOptiScalerJournalAggregate,
    ) -> AppResult<()> {
        self.runtime
            .delete_committed_optiscaler_journal_aggregate(committed)
    }
}
