//! Guards for the aggregate reservation fence.
//!
//! Ordinary repository routes can only observe the fence; lifecycle owners
//! alone may create, advance, or delete a reservation.

mod lifecycle;
mod model;

pub(crate) use lifecycle::{
    begin_within_transaction, delete_within_transaction,
    ensure_no_peer_aggregate_reservation_within_transaction, read_within_transaction,
    transition_to_committed_within_transaction, transition_to_prepared_within_transaction,
};
pub(crate) use model::{
    PeerAggregateKind, PeerAggregateReservation, PeerAggregateReservationState,
};
