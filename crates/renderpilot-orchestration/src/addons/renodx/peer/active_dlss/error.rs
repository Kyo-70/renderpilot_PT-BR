use std::{error::Error, fmt};

use renderpilot_domain::{PathRef, PeerTransitionError};

use crate::peer_mutation_executor::PeerRouteError;

/// Closed errors for pure active DLSS composition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ActiveDlssError {
    /// A sealed input violates the active-DLSS contract.
    Invalid(&'static str),
    /// A path is outside the exact sealed endpoint/root contract.
    Path(PathRef),
    /// The domain claim projection rejected the before/after records.
    Domain(PeerTransitionError),
    /// The lowered endpoint program is not closed.
    Program(PeerRouteError),
}

impl fmt::Display for ActiveDlssError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(reason) => write!(formatter, "invalid active RenoDX DLSS-Fix: {reason}"),
            Self::Path(path) => write!(formatter, "invalid active RenoDX DLSS-Fix path: {path}"),
            Self::Domain(error) => {
                write!(formatter, "active RenoDX DLSS-Fix domain error: {error}")
            }
            Self::Program(error) => {
                write!(
                    formatter,
                    "active RenoDX DLSS-Fix endpoint program error: {error}"
                )
            }
        }
    }
}

impl Error for ActiveDlssError {}

impl From<PeerTransitionError> for ActiveDlssError {
    fn from(error: PeerTransitionError) -> Self {
        Self::Domain(error)
    }
}

impl From<PeerRouteError> for ActiveDlssError {
    fn from(error: PeerRouteError) -> Self {
        Self::Program(error)
    }
}
