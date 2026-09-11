use std::{error::Error, fmt};

use renderpilot_domain::PathRef;

use crate::peer_mutation_executor::PeerRouteError;

/// Failure while lowering a pure RenoDX endpoint program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RenoDxPeerEffectError {
    Program(PeerRouteError),
    InvalidPayloadDigest,
    BeforeImageMismatch(PathRef),
    SamePairPath(PathRef),
    DuplicatePath(PathRef),
    OverlappingPaths(PathRef, PathRef),
    ConflictingPath(PathRef),
    OverlappingBundle(PathRef, PathRef),
    InvalidHostCardinality(usize),
}

impl fmt::Display for RenoDxPeerEffectError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Program(error) => error.fmt(formatter),
            Self::InvalidPayloadDigest => formatter.write_str("invalid RenoDX payload digest"),
            Self::BeforeImageMismatch(path) => {
                write!(formatter, "RenoDX before image mismatch at {path}")
            }
            Self::SamePairPath(path) => {
                write!(formatter, "RenoDX pair uses the same path twice: {path}")
            }
            Self::DuplicatePath(path) => {
                write!(formatter, "duplicate RenoDX effect path: {path}")
            }
            Self::OverlappingPaths(left, right) => {
                write!(
                    formatter,
                    "overlapping RenoDX effect paths: {left} and {right}"
                )
            }
            Self::ConflictingPath(path) => write!(formatter, "conflicting RenoDX effect at {path}"),
            Self::OverlappingBundle(left, right) => {
                write!(
                    formatter,
                    "overlapping RenoDX effect bundles: {left} and {right}"
                )
            }
            Self::InvalidHostCardinality(count) => {
                write!(
                    formatter,
                    "RenoDX effect program has {count} host endpoints"
                )
            }
        }
    }
}

impl Error for RenoDxPeerEffectError {}
