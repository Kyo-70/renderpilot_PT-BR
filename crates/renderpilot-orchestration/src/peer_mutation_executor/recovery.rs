use renderpilot_application::AppResult;
use renderpilot_domain::GameId;
use renderpilot_storage_sqlite::{PendingFileMutationRecoveryCandidate, PendingFileMutationRow};

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
}
