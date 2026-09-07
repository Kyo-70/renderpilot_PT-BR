use renderpilot_application::{AppError, AppResult};

use super::super::aggregate::CommittedAggregate;
use super::super::permit::{PeerStorageRuntime, ensure_runtime};
use super::model::{CommittedMetadataAggregate, PreparedMetadataAggregateCommitPermit};
use super::validation::{
    load_before, read_generation, validate_final_guards, validate_owned_route,
};
use crate::repositories::installed_addons;
use crate::repositories::peer_aggregate_reservations::{
    PeerAggregateKind, PeerAggregateReservationState, read_within_transaction,
    transition_to_committed_within_transaction,
};

/// Borrowed view of one consumed metadata permit while its transaction runs.
///
/// The public entry point destructures the move-only permit before constructing
/// this view, so a failed commit cannot leave the proof available for reuse.
struct MetadataAggregateCommitInput<'a> {
    runtime_identity: &'a std::sync::Arc<super::super::permit::RuntimeInstance>,
    reservation: &'a crate::repositories::peer_aggregate_reservations::PeerAggregateReservation,
    mutation: &'a super::super::aggregate::GameAggregateMutation,
    after: &'a super::super::aggregate::AggregateAfter,
    route: &'a super::model::PreparedMetadataRoute,
}

impl PeerStorageRuntime {
    /// Commits a prepared metadata aggregate and advances its scalar
    /// generation in the same IMMEDIATE transaction as the peer-row update.
    pub fn commit_metadata_aggregate(
        &self,
        permit: PreparedMetadataAggregateCommitPermit,
        final_read_guards: Vec<renderpilot_domain::PeerReadGuardEvidence>,
    ) -> AppResult<CommittedMetadataAggregate> {
        let PreparedMetadataAggregateCommitPermit {
            runtime_identity,
            reservation,
            mutation,
            after,
            route,
        } = permit;
        let result = self.commit_metadata_aggregate_inner(
            &MetadataAggregateCommitInput {
                runtime_identity: &runtime_identity,
                reservation: &reservation,
                mutation: &mutation,
                after: &after,
                route: &route,
            },
            &final_read_guards,
        );
        drop(final_read_guards);
        result
    }

    fn commit_metadata_aggregate_inner(
        &self,
        permit: &MetadataAggregateCommitInput<'_>,
        final_read_guards: &[renderpilot_domain::PeerReadGuardEvidence],
    ) -> AppResult<CommittedMetadataAggregate> {
        let runtime = self.runtime_instance();
        ensure_runtime(&runtime, permit.runtime_identity)?;
        let after = validate_owned_route(permit.mutation, permit.route)?;
        if after != *permit.after {
            return Err(AppError::storage_failed(
                "metadata aggregate permit after-image changed",
            ));
        }
        validate_final_guards(permit.route, final_read_guards)?;

        self.repositories()
            .with_immediate_transaction(|transaction| {
                let current_reservation = read_within_transaction(
                    transaction,
                    permit.reservation.game_id(),
                    permit.reservation.operation_id(),
                )?
                .ok_or_else(|| {
                    AppError::storage_failed("metadata aggregate Prepared reservation disappeared")
                })?;
                if current_reservation != *permit.reservation
                    || current_reservation.kind() != PeerAggregateKind::Metadata
                    || current_reservation.binding() != PeerAggregateKind::Metadata.binding()
                    || current_reservation.state() != PeerAggregateReservationState::Prepared
                {
                    return Err(AppError::storage_failed(
                        "metadata aggregate Prepared reservation changed before commit",
                    ));
                }

                let current_before = load_before(transaction, permit.mutation.before().game_id())?;
                if &current_before != permit.mutation.before() {
                    return Err(AppError::storage_failed(
                        "metadata aggregate before-image changed before commit",
                    ));
                }
                let current_after = validate_owned_route(permit.mutation, permit.route)?;
                if current_after != *permit.after {
                    return Err(AppError::storage_failed(
                        "metadata aggregate after-image changed before commit",
                    ));
                }
                validate_final_guards(permit.route, final_read_guards)?;

                let current_generation =
                    read_generation(transaction, permit.mutation.before().game_id())?;
                if current_generation != permit.reservation.expected_revision() {
                    return Err(AppError::storage_failed(
                        "metadata aggregate generation changed before commit",
                    ));
                }
                let next_generation = current_generation.checked_successor()?;

                installed_addons::upsert_within_transaction(
                    transaction,
                    permit.after.peer().ok_or_else(|| {
                        AppError::storage_failed("metadata aggregate after peer disappeared")
                    })?,
                )?;
                let changed = transaction
                    .execute(
                        "UPDATE games
                     SET peer_aggregate_revision = ?1
                     WHERE id = ?2 AND peer_aggregate_revision = ?3",
                        rusqlite::params![
                            next_generation.as_i64(),
                            permit.mutation.before().game_id().as_str(),
                            current_generation.as_i64(),
                        ],
                    )
                    .map_err(crate::error::storage_error)?;
                if changed != 1 {
                    return Err(AppError::storage_failed(
                        "metadata aggregate generation CAS failed",
                    ));
                }
                let committed_reservation = transition_to_committed_within_transaction(
                    transaction,
                    permit.reservation.game_id(),
                    permit.reservation.operation_id(),
                    PeerAggregateKind::Metadata,
                    permit.reservation.expected_revision(),
                )?;
                let aggregate = CommittedAggregate::new(
                    permit.mutation.operation_id().to_owned(),
                    next_generation,
                    permit.after.clone(),
                )?;
                Ok(CommittedMetadataAggregate {
                    runtime_identity: self.runtime_instance(),
                    aggregate,
                    reservation: committed_reservation,
                })
            })
    }
}
