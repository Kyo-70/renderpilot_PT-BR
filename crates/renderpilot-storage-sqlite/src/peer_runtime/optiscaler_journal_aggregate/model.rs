use std::sync::Arc;

use renderpilot_application::AppResult;
use renderpilot_domain::{GameId, OptiScalerJournal};

use super::super::aggregate::{AggregateBefore, AggregateGeneration, CommittedAggregate};
use super::super::permit::RuntimeInstance;
use crate::repositories::OptiScalerAggregateMutation;
use crate::repositories::peer_aggregate_reservations::PeerAggregateKind;
use crate::repositories::peer_aggregate_reservations::PeerAggregateReservation;
use crate::repositories::pending_file_mutations::{
    PendingFileMutationRow, PendingFileMutationState,
};

/// Input for reserving one OptiScaler journal row before native work starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OptiScalerJournalAggregateBegin {
    pub(super) operation_id: String,
    pub(super) game_id: GameId,
    pub(super) feature: String,
    pub(super) subject_id: Option<String>,
    pub(super) initial_journal_json: String,
}

impl OptiScalerJournalAggregateBegin {
    /// Creates a validated OptiScaler journal preparation request.
    pub fn new(
        operation_id: impl Into<String>,
        game_id: GameId,
        feature: impl Into<String>,
        subject_id: Option<String>,
        initial_journal_json: impl Into<String>,
    ) -> AppResult<Self> {
        let begin = Self {
            operation_id: operation_id.into(),
            game_id,
            feature: feature.into(),
            subject_id,
            initial_journal_json: initial_journal_json.into(),
        };
        super::validation::validate_begin(&begin)?;
        Ok(begin)
    }

    /// Returns the operation identity used for the pending row and fence.
    #[must_use]
    pub fn operation_id(&self) -> &str {
        &self.operation_id
    }

    /// Returns the game that owns the journal row.
    #[must_use]
    pub fn game_id(&self) -> &GameId {
        &self.game_id
    }

    /// Returns the exact OptiScaler feature label.
    #[must_use]
    pub fn feature(&self) -> &str {
        &self.feature
    }

    /// Returns the feature subject bound to the row.
    #[must_use]
    pub fn subject_id(&self) -> Option<&str> {
        self.subject_id.as_deref()
    }

    /// Returns the initial unmaterialized journal JSON.
    #[must_use]
    pub fn initial_journal_json(&self) -> &str {
        &self.initial_journal_json
    }
}

/// Opaque proof that the exact journal reservation is still Preparing.
#[derive(Debug)]
pub struct PreparingOptiScalerJournalAggregate {
    pub(super) runtime_identity: Arc<RuntimeInstance>,
    pub(super) reservation: PeerAggregateReservation,
    pub(super) begin: OptiScalerJournalAggregateBegin,
    pub(super) current_journal_json: String,
    pub(super) pending_identity: PendingJournalIdentity,
}

impl PreparingOptiScalerJournalAggregate {
    /// Returns the immutable reservation input captured before native work.
    #[must_use]
    pub fn begin(&self) -> &OptiScalerJournalAggregateBegin {
        &self.begin
    }
}

/// Opaque proof that the exact journal row reached Prepared.
#[derive(Debug)]
pub struct PreparedOptiScalerJournalAggregate {
    pub(super) runtime_identity: Arc<RuntimeInstance>,
    pub(super) reservation: PeerAggregateReservation,
    pub(super) begin: OptiScalerJournalAggregateBegin,
    pub(super) catalog_binding:
        crate::repositories::pending_file_mutations::PreparedOptiScalerCatalogBinding,
    pub(super) current_journal_json: String,
    pub(super) pending_identity: PendingJournalIdentity,
}

impl PreparedOptiScalerJournalAggregate {
    /// Returns the immutable reservation input captured before native work.
    #[must_use]
    pub fn begin(&self) -> &OptiScalerJournalAggregateBegin {
        &self.begin
    }

    /// Returns the exact complete journal persisted in the Prepared row.
    #[must_use]
    pub fn final_journal_json(&self) -> &str {
        &self.current_journal_json
    }
}

/// Borrowed participant mutation accepted by the journal aggregate commit.
///
/// Metadata adoption is deliberately not represented here: it has no
/// prepared journal row and therefore cannot use this commit boundary.
#[derive(Debug, Clone)]
pub struct OptiScalerJournalAggregateCommit<'a> {
    pub(super) before: AggregateBefore,
    pub(super) mutation: OptiScalerAggregateMutation<'a>,
}

impl<'a> OptiScalerJournalAggregateCommit<'a> {
    /// Binds one filesystem-backed OptiScaler participant transition to the
    /// prepared journal operation.
    #[must_use]
    pub const fn new(before: AggregateBefore, mutation: OptiScalerAggregateMutation<'a>) -> Self {
        Self { before, mutation }
    }

    /// Returns the complete aggregate before-image that must still be
    /// present when the prepared journal is committed.
    #[must_use]
    pub fn before(&self) -> &AggregateBefore {
        &self.before
    }

    /// Returns the exact participant mutation consumed by the commit.
    #[must_use]
    pub const fn mutation(&self) -> OptiScalerAggregateMutation<'a> {
        self.mutation
    }
}

/// Opaque proof returned after the journal, participants, and generation are
/// published atomically.
#[derive(Debug)]
pub struct CommittedOptiScalerJournalAggregate {
    pub(super) runtime_identity: Arc<RuntimeInstance>,
    pub(super) aggregate: CommittedAggregate,
    pub(super) reservation: PeerAggregateReservation,
    pub(super) begin: OptiScalerJournalAggregateBegin,
    pub(super) current_journal_json: String,
    pub(super) pending_identity: PendingJournalIdentity,
}

impl CommittedOptiScalerJournalAggregate {
    /// Returns the exact aggregate image published by the commit.
    #[must_use]
    pub fn aggregate(&self) -> &CommittedAggregate {
        &self.aggregate
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct PendingJournalIdentity {
    pub(super) rowid: i64,
    pub(super) created_at: i64,
    pub(super) updated_at: i64,
}

/// Read-only catalog authority captured while acquiring a Prepared proof.
///
/// This is intentionally separate from the mutation-time binding type: the
/// acquisition path only observes the canonical catalog classifier and never
/// invalidates or advances catalog authority.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum PreparedRecoveryCatalogBinding {
    CatalogAbsent,
    CatalogInvalidated {
        authority_epoch: u64,
        mutation_token: String,
    },
}

/// A row selected for restart recovery.
///
/// The ordinary variants intentionally expose only the public pending-row
/// projection. An OptiScaler row carries a move-only proof whose durable
/// fingerprint remains private to storage.
#[derive(Debug)]
pub enum PendingFileMutationRecoveryCandidate {
    /// A row with no aggregate binding or reservation.
    NotAggregate(PendingFileMutationRow),
    /// A complete OptiScaler journal aggregate recovery proof.
    OptiScaler(Box<RecoveringOptiScalerJournalAggregate>),
}

/// Opaque, restart-safe evidence for one selected OptiScaler journal row.
///
/// This proof deliberately contains no runtime handle or transaction. The
/// acquisition transaction is committed before the value is returned, and
/// future recovery operations must fence every private field again.
#[derive(Debug)]
pub struct RecoveringOptiScalerJournalAggregate {
    pub(super) operation_id: String,
    pub(super) game_id: GameId,
    pub(super) feature: String,
    pub(super) subject_id: Option<String>,
    pub(super) state: PendingFileMutationState,
    pub(super) journal: OptiScalerJournal,
    pub(super) current_journal_json: String,
    pub(super) pending_identity: PendingJournalIdentity,
    pub(super) aggregate_kind: PeerAggregateKind,
    pub(super) aggregate_revision: AggregateGeneration,
    pub(super) reservation: PeerAggregateReservation,
    pub(super) expected_generation: AggregateGeneration,
    pub(super) catalog_binding: Option<PreparedRecoveryCatalogBinding>,
}

impl RecoveringOptiScalerJournalAggregate {
    /// Returns the operation identity selected for recovery.
    #[must_use]
    pub fn operation_id(&self) -> &str {
        &self.operation_id
    }

    /// Returns the game owning the selected journal.
    #[must_use]
    pub fn game_id(&self) -> &GameId {
        &self.game_id
    }

    /// Returns the persisted feature label.
    #[must_use]
    pub fn feature(&self) -> &str {
        &self.feature
    }

    /// Returns the optional feature subject.
    #[must_use]
    pub fn subject_id(&self) -> Option<&str> {
        self.subject_id.as_deref()
    }

    /// Returns the canonical journal snapshot selected from durable storage.
    #[must_use]
    pub fn journal(&self) -> &OptiScalerJournal {
        &self.journal
    }
}
