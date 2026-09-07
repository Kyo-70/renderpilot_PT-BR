use renderpilot_application::{AppError, AppResult};

use super::super::permit::PeerStorageRuntime;
use super::model::{MetadataAggregatePreparation, PreparedMetadataAggregateCommitPermit};
use super::validation::{load_before, validate_owned_route, validate_preparation};
use crate::repositories::peer_aggregate_reservations::{
    PeerAggregateKind, begin_within_transaction, transition_to_prepared_within_transaction,
};

impl PeerStorageRuntime {
    /// Reserves and authenticates one metadata-only game aggregate.
    ///
    /// The reservation is opened first inside one IMMEDIATE transaction.  The
    /// complete persisted aggregate is then loaded and compared to the exact
    /// caller before-image before the row may become Prepared.
    pub fn prepare_metadata_aggregate(
        &self,
        preparation: MetadataAggregatePreparation<'_>,
    ) -> AppResult<PreparedMetadataAggregateCommitPermit> {
        let (mutation, transition) = preparation.into_parts();
        let validated = validate_preparation(&mutation, transition)?;
        let expected_after = validated.after;
        let expected_route = validated.route;
        let operation_id = mutation.operation_id().to_owned();
        let game_id = mutation.before().game_id().clone();

        self.repositories()
            .with_immediate_transaction(|transaction| {
                let reservation = begin_within_transaction(
                    transaction,
                    &game_id,
                    &operation_id,
                    PeerAggregateKind::Metadata,
                )?;
                let current_before = load_before(transaction, &game_id)?;
                if &current_before != mutation.before() {
                    return Err(AppError::storage_failed(
                        "metadata aggregate before-image changed before preparation",
                    ));
                }

                // Re-run every route derivation after the reservation fence has
                // been acquired.  The caller's route input is still borrowed only
                // for this call; the resulting permit owns the proof.
                let current = validate_owned_route(&mutation, &expected_route)?;
                if current != expected_after {
                    return Err(AppError::storage_failed(
                        "metadata aggregate after-image changed during preparation",
                    ));
                }
                let prepared = transition_to_prepared_within_transaction(
                    transaction,
                    &game_id,
                    &operation_id,
                    PeerAggregateKind::Metadata,
                    reservation.expected_revision(),
                )?;
                Ok(PreparedMetadataAggregateCommitPermit {
                    runtime_identity: self.runtime_instance(),
                    reservation: prepared,
                    mutation,
                    after: expected_after,
                    route: expected_route,
                })
            })
    }
}
