use renderpilot_application::AppResult;
use renderpilot_domain::GameId;
use renderpilot_storage_sqlite::{
    PendingFileMutationRecoveryCandidate, PendingFileMutationRow,
    RecoveringOptiScalerJournalAggregate,
};

use super::PeerMutationExecutor;

impl PeerMutationExecutor {
    pub(crate) fn recover_pending_file_mutation_candidates_for_game(
        &self,
        game_id: &GameId,
        select: impl Fn(&PendingFileMutationRow) -> bool,
    ) -> AppResult<Vec<PendingFileMutationRecoveryCandidate>> {
        self.runtime
            .recover_pending_file_mutation_candidates_for_game(game_id, select)
    }

    pub(crate) fn cas_recovering_optiscaler_journal_aggregate(
        &self,
        proof: RecoveringOptiScalerJournalAggregate,
        next_json: impl Into<String>,
    ) -> AppResult<RecoveringOptiScalerJournalAggregate> {
        self.runtime
            .cas_recovering_optiscaler_journal_aggregate(proof, next_json)
    }

    pub(crate) fn delete_preparing_recovering_optiscaler_journal_aggregate_after_rollback(
        &self,
        proof: RecoveringOptiScalerJournalAggregate,
    ) -> AppResult<()> {
        self.runtime
            .delete_preparing_recovering_optiscaler_journal_aggregate_after_rollback(proof)
    }

    pub(crate) fn delete_prepared_recovering_optiscaler_journal_aggregate_after_rollback(
        &self,
        proof: RecoveringOptiScalerJournalAggregate,
    ) -> AppResult<()> {
        self.runtime
            .delete_prepared_recovering_optiscaler_journal_aggregate_after_rollback(proof)
    }

    pub(crate) fn delete_committed_recovering_optiscaler_journal_aggregate(
        &self,
        proof: RecoveringOptiScalerJournalAggregate,
    ) -> AppResult<()> {
        self.runtime
            .delete_committed_recovering_optiscaler_journal_aggregate(proof)
    }
}
