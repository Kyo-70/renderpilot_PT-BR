use renderpilot_application::{AppError, AppResult};
use renderpilot_domain::GameId;
use rusqlite::{OptionalExtension, Transaction, named_params};

use super::super::aggregate::{
    AggregateAfter, AggregateBefore, AggregateGeneration, CommittedAggregate,
};
use super::super::permit::{PeerStorageRuntime, ensure_runtime};
use super::model::{
    CommittedOptiScalerJournalAggregate, OptiScalerJournalAggregateCommit,
    PreparedOptiScalerJournalAggregate,
};
use super::validation::{
    read_pending, validate_committed_row, validate_game_revision, validate_prepared_row,
    validate_reservation,
};
use crate::error::storage_error;
use crate::repositories::peer_aggregate_reservations::{
    PeerAggregateKind, PeerAggregateReservationState, transition_to_committed_within_transaction,
};
use crate::repositories::{OptiScalerAggregateMutation, OptiScalerPeerMutation};
use crate::repositories::{
    apply_optiscaler_transition_within_transaction,
    validate_optiscaler_transition_within_transaction,
};
use crate::sqlite_clock;

impl PeerStorageRuntime {
    /// Commits one prepared OptiScaler journal and all of its catalog
    /// participants in a single IMMEDIATE transaction.
    ///
    /// The prepared proof owns the journal row and reservation identity.  The
    /// participant mutation is checked again by the existing OptiScaler
    /// transition validator before the existing persistence primitive applies
    /// it.  The scalar game generation and both lifecycle records are then
    /// advanced under exact compare-and-swap predicates.
    pub fn commit_optiscaler_journal_aggregate(
        &self,
        prepared: PreparedOptiScalerJournalAggregate,
        commit: OptiScalerJournalAggregateCommit<'_>,
    ) -> AppResult<CommittedOptiScalerJournalAggregate> {
        ensure_runtime(&self.runtime_instance(), &prepared.runtime_identity)?;
        let mutation = commit.mutation();
        let mutation_id = filesystem_mutation_id(mutation)?;
        if mutation_id != prepared.begin().operation_id() {
            return Err(AppError::invalid_input(
                "OptiScaler journal commit mutation id differs from Prepared operation",
            ));
        }
        let game_id = prepared.begin().game_id().clone();
        let committed_begin = prepared.begin.clone();
        if commit.before().game_id() != &game_id {
            return Err(AppError::invalid_input(
                "OptiScaler journal commit before-image belongs to another game",
            ));
        }
        validate_mutation_before(mutation, commit.before())?;
        let result = self.repositories().with_immediate_transaction(|transaction| {
            crate::repositories::pending_file_mutations::validate_optiscaler_catalog_binding(
                transaction,
                &game_id,
                mutation_id,
                &prepared.catalog_binding,
            )?;
            let reservation = validate_reservation(
                transaction,
                &prepared.reservation,
                PeerAggregateReservationState::Prepared,
            )?;
            let row = read_pending(transaction, mutation_id)?.ok_or_else(|| {
                AppError::storage_failed(
                    "OptiScaler journal aggregate Prepared row disappeared before commit",
                )
            })?;
            validate_prepared_row(
                &row,
                prepared.begin(),
                prepared.final_journal_json(),
                reservation.expected_revision(),
                prepared.pending_identity,
            )?;
            if row.identity != prepared.pending_identity {
                return Err(AppError::storage_failed(
                    "OptiScaler journal aggregate Prepared row timestamps changed before commit",
                ));
            }
            validate_game_revision(transaction, &game_id, reservation.expected_revision())?;

            let current_before = load_before(transaction, &game_id)?;
            if current_before != *commit.before() {
                return Err(AppError::storage_failed(
                    "OptiScaler journal aggregate before-image changed before commit",
                ));
            }

            // This is the canonical OptiScaler binding validator.  It checks
            // state/topology/peer custody and the journal's domain binding;
            // this aggregate boundary adds the stricter row/reservation fence
            // above and never weakens the ordinary commit validator.
            let validated_id = validate_optiscaler_transition_within_transaction(
                transaction,
                &game_id,
                mutation,
            )?;
            if validated_id != Some(mutation_id) {
                return Err(AppError::storage_failed(
                    "OptiScaler journal aggregate validator returned a different mutation",
                ));
            }

            let after = mutation_after(mutation, &current_before)?;
            let next_generation = reservation.expected_revision().checked_successor()?;
            apply_optiscaler_transition_within_transaction(transaction, &game_id, mutation)?;

            let advanced = transaction
                .execute(
                    "UPDATE games
                     SET peer_aggregate_revision = :next_revision
                     WHERE id = :game_id
                       AND peer_aggregate_revision = :expected_revision",
                    named_params! {
                        ":next_revision": next_generation.as_i64(),
                        ":game_id": game_id.as_str(),
                        ":expected_revision": reservation.expected_revision().as_i64(),
                    },
                )
                .map_err(storage_error)?;
            if advanced != 1 {
                return Err(AppError::storage_failed(
                    "OptiScaler journal aggregate generation CAS failed",
                ));
            }

            mark_committed(
                transaction,
                &prepared,
                &game_id,
                reservation.expected_revision(),
            )?;
            let committed_reservation = transition_to_committed_within_transaction(
                transaction,
                &game_id,
                mutation_id,
                PeerAggregateKind::OptiScalerJournal,
                reservation.expected_revision(),
            )?;

            let persisted_after = load_before(transaction, &game_id)?;
            let persisted_after = AggregateAfter::new(
                game_id.clone(),
                persisted_after.state().cloned(),
                persisted_after.topology().cloned(),
                persisted_after.peer().cloned(),
            )?;
            if !same_persisted_after(&persisted_after, &after) {
                return Err(AppError::storage_failed(
                    "OptiScaler journal aggregate participants differ after commit",
                ));
            }
            if read_generation(transaction, &game_id)? != next_generation {
                return Err(AppError::storage_failed(
                    "OptiScaler journal aggregate generation differs after commit",
                ));
            }

            let committed_row = read_pending(transaction, mutation_id)?.ok_or_else(|| {
                AppError::storage_failed(
                    "OptiScaler journal aggregate committed row disappeared after commit",
                )
            })?;
            validate_committed_row(
                &committed_row,
                prepared.begin(),
                prepared.final_journal_json(),
                reservation.expected_revision(),
                prepared.pending_identity,
            )?;
            let reread_reservation = crate::repositories::peer_aggregate_reservations::read_within_transaction(
                transaction,
                &game_id,
                mutation_id,
            )?
            .ok_or_else(|| {
                AppError::storage_failed(
                    "OptiScaler journal aggregate committed reservation disappeared after commit",
                )
            })?;
            if reread_reservation != committed_reservation
                || reread_reservation.kind() != PeerAggregateKind::OptiScalerJournal
                || reread_reservation.binding() != PeerAggregateKind::OptiScalerJournal.binding()
                || reread_reservation.state() != PeerAggregateReservationState::Committed
            {
                return Err(AppError::storage_failed(
                    "OptiScaler journal aggregate committed reservation changed after commit",
                ));
            }
            let aggregate = CommittedAggregate::new(
                mutation_id.to_owned(),
                next_generation,
                after,
            )?;
            Ok((
                aggregate,
                committed_reservation,
                committed_row.identity,
                committed_row.manifest_json,
            ))
        });
        drop(commit);
        drop(prepared);
        result.map(
            |(aggregate, reservation, pending_identity, current_journal_json)| {
                CommittedOptiScalerJournalAggregate {
                    runtime_identity: self.runtime_instance(),
                    aggregate,
                    reservation,
                    begin: committed_begin,
                    current_journal_json,
                    pending_identity,
                }
            },
        )
    }
}

fn filesystem_mutation_id(mutation: OptiScalerAggregateMutation<'_>) -> AppResult<&str> {
    match mutation {
        OptiScalerAggregateMutation::Filesystem { mutation_id, .. }
        | OptiScalerAggregateMutation::FilesystemWithAuxiliary { mutation_id, .. } => {
            if mutation_id.trim().is_empty() {
                Err(AppError::invalid_input(
                    "OptiScaler journal commit mutation id is blank",
                ))
            } else {
                Ok(mutation_id)
            }
        }
        OptiScalerAggregateMutation::AdoptExactMetadata { .. } => Err(AppError::invalid_input(
            "OptiScaler metadata adoption cannot use a journal aggregate commit",
        )),
    }
}

fn load_before(transaction: &Transaction<'_>, game_id: &GameId) -> AppResult<AggregateBefore> {
    let state = crate::repositories::get_optiscaler_state_within_transaction(transaction, game_id)?;
    let topology =
        crate::repositories::proxy_topologies::get_within_transaction(transaction, game_id)?;
    let peer = crate::repositories::installed_addons::get_within_transaction(transaction, game_id)?;
    AggregateBefore::new(game_id.clone(), state, topology, peer)
}

fn validate_mutation_before(
    mutation: OptiScalerAggregateMutation<'_>,
    before: &AggregateBefore,
) -> AppResult<()> {
    let (before_state, before_topology, peer_before) = match mutation {
        OptiScalerAggregateMutation::Filesystem {
            before_state,
            before_topology,
            peer,
            ..
        }
        | OptiScalerAggregateMutation::FilesystemWithAuxiliary {
            before_state,
            before_topology,
            peer,
            ..
        } => (
            before_state,
            before_topology,
            match peer {
                OptiScalerPeerMutation::Keep => None,
                OptiScalerPeerMutation::Replace { before, .. } => Some(before),
            },
        ),
        OptiScalerAggregateMutation::AdoptExactMetadata { .. } => {
            return Err(AppError::invalid_input(
                "OptiScaler metadata adoption cannot use a journal aggregate commit",
            ));
        }
    };
    if before_state != before.state() || before_topology != before.topology() {
        return Err(AppError::invalid_input(
            "OptiScaler journal commit participant before-image differs from mutation",
        ));
    }
    if let Some(peer_before) = peer_before
        && before.peer() != Some(peer_before)
    {
        return Err(AppError::invalid_input(
            "OptiScaler journal commit peer before-image differs from mutation",
        ));
    }
    Ok(())
}

fn mutation_after(
    mutation: OptiScalerAggregateMutation<'_>,
    before: &AggregateBefore,
) -> AppResult<AggregateAfter> {
    let (after_state, after_topology, peer) = match mutation {
        OptiScalerAggregateMutation::Filesystem {
            after_state,
            after_topology,
            peer,
            ..
        }
        | OptiScalerAggregateMutation::FilesystemWithAuxiliary {
            after_state,
            after_topology,
            peer,
            ..
        } => (
            after_state.cloned(),
            after_topology.cloned(),
            match peer {
                OptiScalerPeerMutation::Keep => before.peer().cloned(),
                OptiScalerPeerMutation::Replace { after, .. } => Some(after.clone()),
            },
        ),
        OptiScalerAggregateMutation::AdoptExactMetadata { .. } => {
            return Err(AppError::invalid_input(
                "OptiScaler metadata adoption cannot use a journal aggregate commit",
            ));
        }
    };
    AggregateAfter::new(before.game_id().clone(), after_state, after_topology, peer)
}

fn mark_committed(
    transaction: &Transaction<'_>,
    prepared: &PreparedOptiScalerJournalAggregate,
    game_id: &GameId,
    expected_revision: AggregateGeneration,
) -> AppResult<()> {
    let updated = transaction
        .execute(
            "UPDATE pending_file_mutations
             SET state = 'committed', updated_at = :now_ms
             WHERE rowid = :rowid AND id = :id AND game_id = :game_id
               AND feature = :feature AND subject_id IS :subject_id
               AND state = 'prepared' AND manifest_json = :manifest_json
               AND aggregate_kind = :aggregate_kind
               AND aggregate_revision = :aggregate_revision
               AND created_at = :created_at AND updated_at = :updated_at",
            named_params! {
                ":rowid": prepared.pending_identity.rowid,
                ":id": prepared.begin().operation_id(),
                ":game_id": game_id.as_str(),
                ":feature": prepared.begin().feature(),
                ":subject_id": prepared.begin().subject_id(),
                ":manifest_json": prepared.final_journal_json(),
                ":aggregate_kind": PeerAggregateKind::OptiScalerJournal.as_str(),
                ":aggregate_revision": expected_revision.as_i64(),
                ":created_at": prepared.pending_identity.created_at,
                ":updated_at": prepared.pending_identity.updated_at,
                ":now_ms": sqlite_clock::now_ms(transaction)?,
            },
        )
        .map_err(storage_error)?;
    if updated != 1 {
        return Err(AppError::storage_failed(
            "OptiScaler journal aggregate Prepared row changed before commit",
        ));
    }
    Ok(())
}

fn read_generation(
    transaction: &Transaction<'_>,
    game_id: &GameId,
) -> AppResult<AggregateGeneration> {
    let value: Option<i64> = transaction
        .query_row(
            "SELECT peer_aggregate_revision FROM games WHERE id = ?1",
            [game_id.as_str()],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage_error)?;
    value
        .ok_or_else(|| AppError::storage_failed(format!("game `{game_id}` is missing")))
        .and_then(AggregateGeneration::from_persisted)
}

fn same_persisted_after(left: &AggregateAfter, right: &AggregateAfter) -> bool {
    left.persistence_equivalent(right)
}
