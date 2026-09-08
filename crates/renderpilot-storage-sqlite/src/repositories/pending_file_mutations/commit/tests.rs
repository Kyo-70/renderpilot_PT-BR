use super::prelude::*;
use super::*;

pub(in crate::repositories) use renderpilot_domain::{
    ControlNamespaceBinding, CreateDirectoryEffect, CreateDirectoryState, DeleteEffect,
    DeleteState, DurableObservation, Endpoint, ExpectedAfter, FileReceipt, MaterializationState,
    OperationEndpoint, OperationRecord, PrivateArtifactSlots, RelocateEffect, RelocateState,
    Sha256Hash, VerifyEffect, VerifyState, WriteEffect, WriteState,
};

mod auxiliary;
mod cas;
mod helpers;
mod namespace;
mod preimage;
mod relocation;

use helpers::*;

#[test]
fn workspace_materialization_transition_is_ordered() {
    assert!(
        validate_materialization_transition(
            &MaterializationState::Planned,
            &MaterializationState::ControlCreateIntent,
            1,
        )
        .is_ok()
    );
    assert!(
        validate_materialization_transition(
            &MaterializationState::Workspaces {
                next_workspace_id: 0
            },
            &MaterializationState::WorkspaceCreateIntent { workspace_id: 0 },
            1,
        )
        .is_ok()
    );
    assert!(
        validate_materialization_transition(
            &MaterializationState::Workspaces {
                next_workspace_id: 0
            },
            &MaterializationState::Ready,
            1,
        )
        .is_err()
    );
}
