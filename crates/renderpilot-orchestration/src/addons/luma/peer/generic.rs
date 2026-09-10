//! Pure lowering for generic Luma payload endpoints.

use std::{error::Error, fmt};

use renderpilot_domain::PathRef;

use crate::peer_mutation_executor::PeerPathSnapshot;

use super::effects::{LumaPeerEffectAccumulator, LumaPeerEffectError, LumaPeerEffectGroup};
use super::snapshot_input::{
    LumaSnapshotInputError, require_absent, require_file, require_managed_sidecar, snapshot_bytes,
};

#[derive(Debug)]
pub(super) enum LumaGenericDecision<'a> {
    Unchanged,
    Create {
        live_path: &'a PathRef,
        live_snapshot: &'a PeerPathSnapshot,
        prepared_bytes: Vec<u8>,
    },
    AcquireForeign {
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
    RemoveCreated {
        live_path: &'a PathRef,
        live_snapshot: &'a PeerPathSnapshot,
    },
    ReleaseBacked {
        live_path: &'a PathRef,
        sidecar_path: &'a PathRef,
        live_snapshot: &'a PeerPathSnapshot,
        sidecar_snapshot: &'a PeerPathSnapshot,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum LumaGenericLoweringError {
    Snapshot(LumaSnapshotInputError),
    Effects(LumaPeerEffectError),
}

impl fmt::Display for LumaGenericLoweringError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Snapshot(error) => error.fmt(formatter),
            Self::Effects(error) => error.fmt(formatter),
        }
    }
}

impl Error for LumaGenericLoweringError {}

impl From<LumaSnapshotInputError> for LumaGenericLoweringError {
    fn from(error: LumaSnapshotInputError) -> Self {
        Self::Snapshot(error)
    }
}

impl From<LumaPeerEffectError> for LumaGenericLoweringError {
    fn from(error: LumaPeerEffectError) -> Self {
        Self::Effects(error)
    }
}

type LoweringResult<T> = Result<T, LumaGenericLoweringError>;

pub(super) fn lower_generic_decision(
    decision: LumaGenericDecision<'_>,
    accumulator: &mut LumaPeerEffectAccumulator,
) -> LoweringResult<()> {
    match decision {
        LumaGenericDecision::Unchanged => Ok(()),
        LumaGenericDecision::Create {
            live_path,
            live_snapshot,
            prepared_bytes,
        } => {
            require_absent(live_path, live_snapshot)?;
            accumulator.create(
                LumaPeerEffectGroup::Generic,
                live_path.clone(),
                prepared_bytes,
            )?;
            Ok(())
        }
        LumaGenericDecision::AcquireForeign {
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
                LumaPeerEffectGroup::Generic,
                live_path.clone(),
                managed_sidecar,
                live_before,
                original_bytes,
                prepared_bytes,
            )?;
            Ok(())
        }
        LumaGenericDecision::ReplaceOwned {
            live_path,
            live_snapshot,
            prepared_bytes,
        } => {
            let live_before = require_file(live_path, live_snapshot)?;
            accumulator.replace(
                LumaPeerEffectGroup::Generic,
                live_path.clone(),
                live_before,
                prepared_bytes,
            )?;
            Ok(())
        }
        LumaGenericDecision::RemoveCreated {
            live_path,
            live_snapshot,
        } => {
            let live_before = require_file(live_path, live_snapshot)?;
            accumulator.remove(LumaPeerEffectGroup::Generic, live_path.clone(), live_before)?;
            Ok(())
        }
        LumaGenericDecision::ReleaseBacked {
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
                LumaPeerEffectGroup::Generic,
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
