//! Pure lowering for the active Luma host endpoint.

use std::{error::Error, fmt};

use renderpilot_domain::PathRef;

use crate::peer_mutation_executor::PeerPathSnapshot;

use super::effects::{LumaPeerEffectAccumulator, LumaPeerEffectError, LumaPeerEffectGroup};
use super::snapshot_input::{
    LumaSnapshotInputError, require_absent, require_file, require_managed_sidecar, snapshot_bytes,
};

/// Closed host transition selected by the Luma planner.
#[derive(Debug)]
pub(super) enum LumaHostDecision<'a> {
    /// Materialize an absent host file.
    Create {
        live_path: &'a PathRef,
        live_snapshot: &'a PeerPathSnapshot,
        prepared_bytes: Vec<u8>,
    },
    /// Replace the exact observed host file with prepared bytes.
    Replace {
        live_path: &'a PathRef,
        live_snapshot: &'a PeerPathSnapshot,
        prepared_bytes: Vec<u8>,
    },
    /// Remove an owned host whose baseline was absent.
    ReleaseAbsent {
        live_path: &'a PathRef,
        live_snapshot: &'a PeerPathSnapshot,
    },
    /// Restore an owned host's present baseline and remove its managed sidecar.
    ReleasePresent {
        live_path: &'a PathRef,
        sidecar_path: &'a PathRef,
        live_snapshot: &'a PeerPathSnapshot,
        sidecar_snapshot: &'a PeerPathSnapshot,
    },
}

/// Errors raised before a host decision can add anything to the accumulator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum LumaHostLoweringError {
    Snapshot(LumaSnapshotInputError),
    Effects(LumaPeerEffectError),
}

impl fmt::Display for LumaHostLoweringError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Snapshot(error) => error.fmt(formatter),
            Self::Effects(error) => error.fmt(formatter),
        }
    }
}

impl Error for LumaHostLoweringError {}

impl From<LumaSnapshotInputError> for LumaHostLoweringError {
    fn from(error: LumaSnapshotInputError) -> Self {
        Self::Snapshot(error)
    }
}

impl From<LumaPeerEffectError> for LumaHostLoweringError {
    fn from(error: LumaPeerEffectError) -> Self {
        Self::Effects(error)
    }
}

/// Lowers one typed host decision without observing, hashing, or writing.
pub(super) fn lower_host_decision(
    decision: LumaHostDecision<'_>,
    accumulator: &mut LumaPeerEffectAccumulator,
) -> Result<(), LumaHostLoweringError> {
    match decision {
        LumaHostDecision::Create {
            live_path,
            live_snapshot,
            prepared_bytes,
        } => {
            require_absent(live_path, live_snapshot)?;
            accumulator.create(LumaPeerEffectGroup::Host, live_path.clone(), prepared_bytes)?;
            Ok(())
        }
        LumaHostDecision::Replace {
            live_path,
            live_snapshot,
            prepared_bytes,
        } => {
            let live_before = require_file(live_path, live_snapshot)?;
            accumulator.replace(
                LumaPeerEffectGroup::Host,
                live_path.clone(),
                live_before,
                prepared_bytes,
            )?;
            Ok(())
        }
        LumaHostDecision::ReleaseAbsent {
            live_path,
            live_snapshot,
        } => {
            let live_before = require_file(live_path, live_snapshot)?;
            accumulator.remove(LumaPeerEffectGroup::Host, live_path.clone(), live_before)?;
            Ok(())
        }
        LumaHostDecision::ReleasePresent {
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
                LumaPeerEffectGroup::Host,
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
