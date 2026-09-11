use std::{error::Error, fmt};

use renderpilot_domain::PathRef;

use crate::{ServiceError, peer_mutation_executor::PeerRouteError};

use super::super::effects::RenoDxPeerEffectError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RenoDxHostError {
    Path(PathRef, PathRef),
    Evidence(PathRef, &'static str),
    Assessment(&'static str),
    Prepared(&'static str),
    Sidecar(ServiceError),
    Effects(RenoDxPeerEffectError),
    Program(PeerRouteError),
}

impl fmt::Display for RenoDxHostError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Path(expected, observed) => write!(
                formatter,
                "RenoDX active host path {observed} differs from expected {expected}"
            ),
            Self::Evidence(path, reason) => {
                write!(formatter, "RenoDX host evidence at {path}: {reason}")
            }
            Self::Assessment(reason) => write!(formatter, "RenoDX host assessment: {reason}"),
            Self::Prepared(reason) => write!(formatter, "RenoDX prepared host: {reason}"),
            Self::Sidecar(error) => error.fmt(formatter),
            Self::Effects(error) => error.fmt(formatter),
            Self::Program(error) => error.fmt(formatter),
        }
    }
}

impl Error for RenoDxHostError {}

impl From<RenoDxPeerEffectError> for RenoDxHostError {
    fn from(error: RenoDxPeerEffectError) -> Self {
        Self::Effects(error)
    }
}

impl From<PeerRouteError> for RenoDxHostError {
    fn from(error: PeerRouteError) -> Self {
        Self::Program(error)
    }
}
