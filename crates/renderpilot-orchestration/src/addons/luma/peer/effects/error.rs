use std::{error::Error, fmt};

use renderpilot_domain::PathRef;

use crate::peer_mutation_executor::PeerRouteError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LumaPeerEffectError {
    ConflictingBundle(PathRef),
    OverlappingPaths(PathRef, PathRef),
    SamePairPath(PathRef),
    InvalidHostBundle(usize),
    DlssCascadeRestoreAbsentOnly,
    BeforeImageMismatch(PathRef),
    BaselineImageMismatch(PathRef),
    InvalidPayloadDigest,
    Program(PeerRouteError),
}

impl fmt::Display for LumaPeerEffectError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ConflictingBundle(path) => {
                write!(
                    formatter,
                    "conflicting Luma peer bundle for {}",
                    path.as_str()
                )
            }
            Self::OverlappingPaths(left, right) => write!(
                formatter,
                "overlapping Luma peer paths {} and {}",
                left.as_str(),
                right.as_str()
            ),
            Self::SamePairPath(path) => write!(
                formatter,
                "Luma peer live and sidecar paths must differ: {}",
                path.as_str()
            ),
            Self::InvalidHostBundle(count) => write!(
                formatter,
                "Luma host group must contain exactly one topology downstream live endpoint, found {count}"
            ),
            Self::DlssCascadeRestoreAbsentOnly => {
                formatter.write_str("restore-from-absent is reserved for the Luma DLSS cascade")
            }
            Self::BeforeImageMismatch(path) => write!(
                formatter,
                "supplied Luma peer bytes do not match the observed image at {}",
                path.as_str()
            ),
            Self::BaselineImageMismatch(path) => write!(
                formatter,
                "supplied Luma baseline bytes do not match the observed sidecar at {}",
                path.as_str()
            ),
            Self::InvalidPayloadDigest => {
                formatter.write_str("prepared Luma peer payload produced an invalid SHA-256")
            }
            Self::Program(error) => error.fmt(formatter),
        }
    }
}

impl Error for LumaPeerEffectError {}
