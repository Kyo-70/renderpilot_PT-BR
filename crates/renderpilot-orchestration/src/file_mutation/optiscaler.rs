//! Native execution adapter for the shared OptiScaler journal.
//!
//! Durable state is never projected into a second orchestration model. The
//! only native-only value is `DiskObservation`, converted at filesystem
//! authority boundaries. Implementation responsibilities are split into
//! focused modules below; this file remains the stable crate-private facade.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use renderpilot_domain::{
    ArtifactSlot, CleanupState as JournalCleanup, ControlNamespaceBinding,
    CreateDirectoryEffect as DomainCreateDirectoryEffect,
    CreateDirectoryState as DomainCreateDirectoryState, DeleteEffect as DomainDeleteEffect,
    DeleteState as DomainDeleteState, DurableObservation, Endpoint as DomainEndpoint,
    ExpectedAfter as JournalAfter, FileOwnership, FileReceipt,
    MaterializationState as DomainMaterializationState, NamespaceCapability,
    OperationEffect as DomainOperationEffect, OperationEndpoint as DomainOperationEndpoint,
    OperationRecord as DomainOperationRecord, OptiScalerJournal, Preimage as DomainPreimage,
    PrivateArtifactSlots, PrivateWorkspaceBinding, RelocateEffect as DomainRelocateEffect,
    RelocateState as DomainRelocateState, RemoveDirectoryEffect as DomainRemoveDirectoryEffect,
    RemoveDirectoryState as DomainRemoveDirectoryState, Sha256Hash,
    VerifyEffect as DomainVerifyEffect, VerifyState as DomainVerifyState,
    WriteEffect as DomainWriteEffect, WriteState as DomainWriteState,
};
#[cfg(test)]
use renderpilot_storage_sqlite::SqliteStorage;
use renderpilot_storage_sqlite::{
    AggregateBefore, CommittedOptiScalerJournalAggregate, OptiScalerAggregateMutation,
    OptiScalerJournalAggregateBegin, OptiScalerJournalAggregateCommit,
    PendingFileMutationRecoveryCandidate, PendingFileMutationState,
    PreparedOptiScalerJournalAggregate, PreparingOptiScalerJournalAggregate,
    RecoveringOptiScalerJournalAggregate,
};
use sha2::{Digest, Sha256};

use super::MutationScope;
use crate::fs::ControlNamespace;
use crate::game_mutation_lock::GameMutationGuard;
use crate::peer_mutation_executor::PeerMutationExecutor;
use crate::{Context, ServiceError};

pub(crate) use renderpilot_domain::ThreatModel;

// These implementation files are included into one private lexical module on
// purpose. They share the transaction's private journal/custody boundary;
// splitting that boundary into public or `pub(super)` state would weaken the
// invariant surface merely to satisfy file layout.
include!("optiscaler/support.rs");
mod prepared;

use prepared::JournalAuthority;
pub(crate) use prepared::PreparedFileMutation;
include!("optiscaler/planning.rs");
include!("optiscaler/filesystem.rs");
include!("optiscaler/adoption.rs");
include!("optiscaler/rollback_classify.rs");
include!("optiscaler/rollback_write.rs");
include!("optiscaler/rollback_delete.rs");
include!("optiscaler/rollback_relocate.rs");
include!("optiscaler/rollback_directory.rs");
include!("optiscaler/rollback_verify.rs");
include!("optiscaler/rollback.rs");
include!("optiscaler/recovery.rs");

mod namespace;
mod post_commit;

#[cfg(test)]
mod tests;
