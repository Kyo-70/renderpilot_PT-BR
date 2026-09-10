//! Pure lowering for one dgVoodoo runtime or final-config path.

use std::{error::Error, fmt};

use renderpilot_domain::PathRef;

use crate::peer_mutation_executor::PeerPathSnapshot;

use super::effects::{LumaPeerEffectAccumulator, LumaPeerEffectError, LumaPeerEffectGroup};
use super::snapshot_input::{
    LumaSnapshotInputError, require_absent, require_file, require_managed_sidecar, snapshot_bytes,
};

/// Closed per-path dgVoodoo transition. Group and role are fixed internally.
#[derive(Debug)]
pub(super) enum DgVoodooDecision<'a> {
    UnchangedOrReused,
    CreateManaged {
        live_path: &'a PathRef,
        live_snapshot: &'a PeerPathSnapshot,
        prepared_bytes: Vec<u8>,
    },
    AcquireManaged {
        live_path: &'a PathRef,
        sidecar_path: &'a PathRef,
        live_snapshot: &'a PeerPathSnapshot,
        sidecar_snapshot: &'a PeerPathSnapshot,
        prepared_bytes: Vec<u8>,
    },
    ReplaceOwned {
        live_path: &'a PathRef,
        live_snapshot: &'a PeerPathSnapshot,
        prepared_bytes: Vec<u8>,
    },
    ReleaseOwnedAbsent {
        live_path: &'a PathRef,
        live_snapshot: &'a PeerPathSnapshot,
    },
    ReleaseOwnedPresent {
        live_path: &'a PathRef,
        sidecar_path: &'a PathRef,
        live_snapshot: &'a PeerPathSnapshot,
        sidecar_snapshot: &'a PeerPathSnapshot,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DgVoodooLoweringError {
    Snapshot(LumaSnapshotInputError),
    Effects(LumaPeerEffectError),
}

impl fmt::Display for DgVoodooLoweringError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Snapshot(error) => error.fmt(formatter),
            Self::Effects(error) => error.fmt(formatter),
        }
    }
}

impl Error for DgVoodooLoweringError {}

impl From<LumaSnapshotInputError> for DgVoodooLoweringError {
    fn from(error: LumaSnapshotInputError) -> Self {
        Self::Snapshot(error)
    }
}

impl From<LumaPeerEffectError> for DgVoodooLoweringError {
    fn from(error: LumaPeerEffectError) -> Self {
        Self::Effects(error)
    }
}

type LoweringResult<T> = Result<T, DgVoodooLoweringError>;

pub(super) fn lower_dgvoodoo_decision(
    decision: DgVoodooDecision<'_>,
    accumulator: &mut LumaPeerEffectAccumulator,
) -> LoweringResult<()> {
    match decision {
        DgVoodooDecision::UnchangedOrReused => Ok(()),
        DgVoodooDecision::CreateManaged {
            live_path,
            live_snapshot,
            prepared_bytes,
        } => {
            require_absent(live_path, live_snapshot)?;
            accumulator.create(
                LumaPeerEffectGroup::DgVoodoo,
                live_path.clone(),
                prepared_bytes,
            )?;
            Ok(())
        }
        DgVoodooDecision::AcquireManaged {
            live_path,
            sidecar_path,
            live_snapshot,
            sidecar_snapshot,
            prepared_bytes,
        } => {
            let managed_sidecar = require_managed_sidecar(live_path, sidecar_path)?;
            let live_before = require_file(live_path, live_snapshot)?;
            require_absent(sidecar_path, sidecar_snapshot)?;
            let original_bytes = snapshot_bytes(live_path, live_snapshot)?;
            accumulator.acquire_foreign(
                LumaPeerEffectGroup::DgVoodoo,
                live_path.clone(),
                managed_sidecar,
                live_before,
                original_bytes,
                prepared_bytes,
            )?;
            Ok(())
        }
        DgVoodooDecision::ReplaceOwned {
            live_path,
            live_snapshot,
            prepared_bytes,
        } => {
            let live_before = require_file(live_path, live_snapshot)?;
            accumulator.replace(
                LumaPeerEffectGroup::DgVoodoo,
                live_path.clone(),
                live_before,
                prepared_bytes,
            )?;
            Ok(())
        }
        DgVoodooDecision::ReleaseOwnedAbsent {
            live_path,
            live_snapshot,
        } => {
            let live_before = require_file(live_path, live_snapshot)?;
            accumulator.remove(
                LumaPeerEffectGroup::DgVoodoo,
                live_path.clone(),
                live_before,
            )?;
            Ok(())
        }
        DgVoodooDecision::ReleaseOwnedPresent {
            live_path,
            sidecar_path,
            live_snapshot,
            sidecar_snapshot,
        } => {
            let managed_sidecar = require_managed_sidecar(live_path, sidecar_path)?;
            let live_before = require_file(live_path, live_snapshot)?;
            let sidecar_before = require_file(sidecar_path, sidecar_snapshot)?;
            let baseline_bytes = snapshot_bytes(sidecar_path, sidecar_snapshot)?;
            accumulator.release_present(
                LumaPeerEffectGroup::DgVoodoo,
                live_path.clone(),
                managed_sidecar,
                live_before,
                sidecar_before,
                baseline_bytes,
            )?;
            Ok(())
        }
    }
}
