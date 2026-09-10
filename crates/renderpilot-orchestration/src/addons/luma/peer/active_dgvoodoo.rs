//! Active-topology dgVoodoo planning and lowering.
//!
//! The facade exposes one phase-1 planner and one phase-3 lowerer. The
//! implementation is split by responsibility so filesystem authority, state
//! modelling, and effect construction cannot grow into another lifecycle
//! orchestrator.

use std::{error::Error, fmt};

use renderpilot_domain::{PathRef, PeerTransitionError};

use super::{
    dgvoodoo::DgVoodooLoweringError, effects::LumaPeerEffectError,
    snapshot_input::LumaSnapshotInputError,
};

mod lowering;
mod model;
mod validation;

pub(crate) use lowering::lower_active_dgvoodoo;
pub(crate) use model::{
    ActiveDgVoodooPlan, ActiveDgVoodooRecordProjection, ActiveDgVoodooSnapshot,
    ActiveDgVoodooTargetView, ActiveDgVoodooTargetViewPayload,
};
pub(crate) use validation::plan_active_dgvoodoo;

/// The kind of target rejected by structural validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActiveDgVoodooTargetKind {
    Runtime,
    Config,
    Sidecar,
}

impl fmt::Display for ActiveDgVoodooTargetKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Runtime => "runtime",
            Self::Config => "config",
            Self::Sidecar => "sidecar",
        })
    }
}

/// Fail-closed errors raised before any effect is added or while lowering a
/// previously validated plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ActiveDgVoodooError {
    AdoptedOwnershipUnsupported,
    InvalidTargetName {
        kind: ActiveDgVoodooTargetKind,
        name: String,
    },
    UnauthorizedTarget {
        kind: ActiveDgVoodooTargetKind,
        path: PathRef,
    },
    DuplicateTarget {
        first: PathRef,
        duplicate: PathRef,
    },
    SidecarPath {
        live: PathRef,
        error: PeerTransitionError,
    },
    DuplicateSnapshot(PathRef),
    UnexpectedSnapshot(PathRef),
    MissingSnapshot(PathRef),
    SnapshotPathAlias {
        expected: PathRef,
        supplied: PathRef,
    },
    Snapshot(LumaSnapshotInputError),
    SnapshotImageMismatch(PathRef),
    InvalidConfigEncoding(PathRef),
    Lowering(DgVoodooLoweringError),
    Effects(LumaPeerEffectError),
}

impl fmt::Display for ActiveDgVoodooError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AdoptedOwnershipUnsupported => formatter.write_str(
                "active Luma install requires adoptable dgVoodoo to be normalized as reused",
            ),
            Self::InvalidTargetName { kind, name } => {
                write!(formatter, "invalid dgVoodoo {kind} target name `{name}`")
            }
            Self::UnauthorizedTarget { kind, path } => {
                write!(
                    formatter,
                    "dgVoodoo {kind} target is outside the sealed game root: {path}"
                )
            }
            Self::DuplicateTarget { first, duplicate } => write!(
                formatter,
                "dgVoodoo targets alias each other: {first} and {duplicate}"
            ),
            Self::SidecarPath { live, error } => {
                write!(
                    formatter,
                    "failed to derive dgVoodoo sidecar for {live}: {error}"
                )
            }
            Self::DuplicateSnapshot(path) => {
                write!(formatter, "dgVoodoo phase-3 snapshot is duplicated: {path}")
            }
            Self::UnexpectedSnapshot(path) => {
                write!(formatter, "unexpected dgVoodoo phase-3 snapshot: {path}")
            }
            Self::MissingSnapshot(path) => {
                write!(formatter, "missing dgVoodoo phase-3 snapshot: {path}")
            }
            Self::SnapshotPathAlias { expected, supplied } => write!(
                formatter,
                "dgVoodoo snapshot path alias `{supplied}` does not equal expected `{expected}`"
            ),
            Self::Snapshot(error) => error.fmt(formatter),
            Self::SnapshotImageMismatch(path) => {
                write!(
                    formatter,
                    "dgVoodoo snapshot bytes do not match its retained image: {path}"
                )
            }
            Self::InvalidConfigEncoding(path) => {
                write!(formatter, "dgVoodoo config is not valid UTF-8: {path}")
            }
            Self::Lowering(error) => error.fmt(formatter),
            Self::Effects(error) => error.fmt(formatter),
        }
    }
}

impl Error for ActiveDgVoodooError {}

impl From<LumaSnapshotInputError> for ActiveDgVoodooError {
    fn from(error: LumaSnapshotInputError) -> Self {
        Self::Snapshot(error)
    }
}

impl From<DgVoodooLoweringError> for ActiveDgVoodooError {
    fn from(error: DgVoodooLoweringError) -> Self {
        Self::Lowering(error)
    }
}

impl From<LumaPeerEffectError> for ActiveDgVoodooError {
    fn from(error: LumaPeerEffectError) -> Self {
        Self::Effects(error)
    }
}
