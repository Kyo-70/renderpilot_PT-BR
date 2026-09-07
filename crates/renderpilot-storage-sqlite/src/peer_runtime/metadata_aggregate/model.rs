use std::sync::Arc;

use renderpilot_domain::{PathRef, PeerReadGuardEvidence, PeerReusedClaimMembershipContract};

use super::super::aggregate::{AggregateAfter, CommittedAggregate, GameAggregateMutation};
use super::super::permit::RuntimeInstance;
use crate::repositories::peer_aggregate_reservations::PeerAggregateReservation;

/// Route selected by the caller for a metadata-only aggregate transition.
///
/// The storage boundary always rederives and validates the route from the
/// complete aggregate mutation.  The borrowed fields are observation input,
/// never durable authority supplied by the caller.
#[derive(Debug, Clone, Copy)]
pub enum MetadataAggregateTransition<'a> {
    /// Refresh peer provenance without changing physical claims.
    PeerMetadataRefresh,
    /// Add or remove reused claims while keeping the OptiScaler topology
    /// unchanged and writing no endpoint.
    ReusedClaimMembership {
        /// Process-local canonical game root.
        canonical_game_root: &'a str,
        /// Ordered roots sealed by the caller's authority boundary.
        sealed_roots: &'a [String],
        /// Initial observations for storage-derived membership guards.
        initial_read_guards: &'a [PeerReadGuardEvidence],
    },
}

/// Borrowed input for one metadata-only aggregate preparation.
#[derive(Debug)]
pub struct MetadataAggregatePreparation<'a> {
    pub(super) mutation: GameAggregateMutation,
    pub(super) transition: MetadataAggregateTransition<'a>,
}

impl<'a> MetadataAggregatePreparation<'a> {
    /// Creates a preparation request.  The mutation is owned so the eventual
    /// permit cannot depend on caller-owned aggregate images.
    #[must_use]
    pub const fn new(
        mutation: GameAggregateMutation,
        transition: MetadataAggregateTransition<'a>,
    ) -> Self {
        Self {
            mutation,
            transition,
        }
    }

    #[must_use]
    /// Returns the exact aggregate mutation owned by this preparation.
    pub fn mutation(&self) -> &GameAggregateMutation {
        &self.mutation
    }

    /// Returns the caller-selected route and its borrowed observations.
    #[must_use]
    pub const fn transition(&self) -> MetadataAggregateTransition<'a> {
        self.transition
    }

    pub(super) fn into_parts(self) -> (GameAggregateMutation, MetadataAggregateTransition<'a>) {
        (self.mutation, self.transition)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum PreparedMetadataRoute {
    PeerMetadataRefresh,
    ReusedClaimMembership {
        canonical_game_root: PathRef,
        sealed_roots: Vec<PathRef>,
        contract: PeerReusedClaimMembershipContract,
        initial_read_guards: Vec<PeerReadGuardEvidence>,
    },
}

/// Move-only proof for one exact Prepared metadata reservation.
#[derive(Debug)]
pub struct PreparedMetadataAggregateCommitPermit {
    pub(super) runtime_identity: Arc<RuntimeInstance>,
    pub(super) reservation: PeerAggregateReservation,
    pub(super) mutation: GameAggregateMutation,
    pub(super) after: AggregateAfter,
    pub(super) route: PreparedMetadataRoute,
}

/// Opaque result of a committed metadata aggregate transaction.
#[derive(Debug)]
pub struct CommittedMetadataAggregate {
    pub(super) runtime_identity: Arc<RuntimeInstance>,
    pub(super) aggregate: CommittedAggregate,
    pub(super) reservation: PeerAggregateReservation,
}

impl CommittedMetadataAggregate {
    /// Returns the committed aggregate image.  The reservation identity stays
    /// private so cleanup cannot be retargeted by a caller.
    #[must_use]
    pub fn aggregate(&self) -> &CommittedAggregate {
        &self.aggregate
    }

    #[must_use]
    pub(crate) fn reservation(&self) -> &PeerAggregateReservation {
        &self.reservation
    }
}
