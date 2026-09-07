use renderpilot_application::{AppError, AppResult};

use super::super::permit::{PeerStorageRuntime, ensure_runtime};
use super::model::CommittedMetadataAggregate;
use super::validation::read_generation;
use crate::repositories::peer_aggregate_reservations::{
    PeerAggregateKind, PeerAggregateReservationState, delete_within_transaction,
    read_within_transaction,
};

impl PeerStorageRuntime {
    /// Removes exactly the committed metadata reservation carried by the
    /// opaque result.  Until this succeeds, ordinary game-scoped writes stay
    /// fenced by the committed reservation.
    pub fn cleanup_metadata_aggregate(
        &self,
        committed: CommittedMetadataAggregate,
    ) -> AppResult<()> {
        let runtime = self.runtime_instance();
        let result = ensure_runtime(&runtime, &committed.runtime_identity)
            .and_then(|()| self.cleanup_metadata_aggregate_inner(&committed));
        drop(committed);
        result
    }

    fn cleanup_metadata_aggregate_inner(
        &self,
        committed: &CommittedMetadataAggregate,
    ) -> AppResult<()> {
        let reservation = committed.reservation();
        if reservation.kind() != PeerAggregateKind::Metadata
            || reservation.binding() != PeerAggregateKind::Metadata.binding()
            || reservation.state() != PeerAggregateReservationState::Committed
            || reservation.game_id() != committed.aggregate.after().game_id()
            || reservation.operation_id() != committed.aggregate.operation_id()
        {
            return Err(AppError::storage_failed(
                "metadata aggregate committed result does not match its reservation",
            ));
        }
        let expected_generation = reservation.expected_revision().checked_successor()?;
        if committed.aggregate.generation() != expected_generation {
            return Err(AppError::storage_failed(
                "metadata aggregate committed generation does not match its reservation",
            ));
        }

        self.repositories()
            .with_immediate_transaction(|transaction| {
                let current = read_within_transaction(
                    transaction,
                    reservation.game_id(),
                    reservation.operation_id(),
                )?
                .ok_or_else(|| {
                    AppError::storage_failed("metadata aggregate committed reservation disappeared")
                })?;
                if &current != reservation
                    || current.kind() != PeerAggregateKind::Metadata
                    || current.binding() != PeerAggregateKind::Metadata.binding()
                    || current.state() != PeerAggregateReservationState::Committed
                {
                    return Err(AppError::storage_failed(
                        "metadata aggregate committed reservation changed before cleanup",
                    ));
                }
                let current_generation = read_generation(transaction, reservation.game_id())?;
                if current_generation != committed.aggregate.generation() {
                    return Err(AppError::storage_failed(
                        "metadata aggregate generation changed before cleanup",
                    ));
                }
                delete_within_transaction(
                    transaction,
                    reservation.game_id(),
                    reservation.operation_id(),
                    PeerAggregateKind::Metadata,
                    reservation.expected_revision(),
                    PeerAggregateReservationState::Committed,
                )
            })
    }
}
