use std::{error::Error, fmt};

use renderpilot_domain::PathRef;

use super::super::effects::RenoDxPeerEffectError;

/// Fail-closed errors produced while composing an active RenoDX update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RenoDxActiveUpdateError {
    /// A required input invariant is missing.
    InvalidInput(&'static str),
    /// A path is outside the supplied sealed roots or otherwise mismatched.
    InvalidPath(PathRef),
    /// The retained endpoint image does not match its verified metadata.
    BeforeImageMismatch(PathRef),
    /// The peer record contains an invalid physical claim.
    InvalidRecord(&'static str),
    /// Endpoint lowering rejected the exact program.
    Effects(RenoDxPeerEffectError),
}

impl fmt::Display for RenoDxActiveUpdateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInput(reason) => {
                write!(formatter, "invalid RenoDX active update: {reason}")
            }
            Self::InvalidPath(path) => {
                write!(formatter, "invalid RenoDX active-update path: {path}")
            }
            Self::BeforeImageMismatch(path) => {
                write!(
                    formatter,
                    "RenoDX active-update before image mismatch at {path}"
                )
            }
            Self::InvalidRecord(reason) => {
                write!(formatter, "invalid RenoDX active-update record: {reason}")
            }
            Self::Effects(error) => error.fmt(formatter),
        }
    }
}
impl Error for RenoDxActiveUpdateError {}

impl From<RenoDxPeerEffectError> for RenoDxActiveUpdateError {
    fn from(error: RenoDxPeerEffectError) -> Self {
        Self::Effects(error)
    }
}
