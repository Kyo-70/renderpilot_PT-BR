//! Read-only lowering for an active-topology managed Luma host release.
//!
//! The active topology and persisted managed claim jointly authorize the
//! endpoint.  This adapter observes the retained live/sidecar images and
//! delegates the physical effect construction to the closed host lowerer.

use std::{
    error::Error,
    fmt,
    path::{Component, Path},
};

use renderpilot_domain::{
    FileOwnership, GameProxyTopology, ManagedAddonFile, ManagedFileBaseline, ManagedFileMode,
    PathRef, PeerTransitionError, ProxyImplementation, ProxyTopologyError, Sha256Hash,
    managed_sidecar_path, normalized_path_key,
};

use crate::ServiceError;
use crate::peer_mutation_executor::{PeerPathSnapshot, observe_peer_path_snapshot};

use super::effects::{LumaPeerEffectAccumulator, LumaPeerEffectError, ensure_bytes_match_image};
use super::host::{LumaHostDecision, LumaHostLoweringError, lower_host_decision};
use super::snapshot_input::{LumaSnapshotInputError, require_absent, require_file, snapshot_bytes};

/// Failure to lower an active-topology managed host release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ManagedHostReleaseError {
    InvalidTopology(&'static str),
    Topology(ProxyTopologyError),
    ClaimMismatch(&'static str),
    PathMismatch {
        persisted: PathRef,
        topology: PathRef,
    },
    DigestMismatch {
        path: PathRef,
        expected: Sha256Hash,
        observed: Sha256Hash,
    },
    Observation {
        path: PathRef,
        error: ServiceError,
    },
    Snapshot(LumaSnapshotInputError),
    Domain(PeerTransitionError),
    Effects(LumaPeerEffectError),
    Lowering(LumaHostLoweringError),
}

impl fmt::Display for ManagedHostReleaseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidTopology(reason) => {
                write!(formatter, "invalid active Luma host topology: {reason}")
            }
            Self::Topology(error) => error.fmt(formatter),
            Self::ClaimMismatch(reason) => {
                write!(formatter, "active Luma host claim mismatch: {reason}")
            }
            Self::PathMismatch {
                persisted,
                topology,
            } => write!(
                formatter,
                "active Luma host path {persisted} differs from topology downstream {topology}"
            ),
            Self::DigestMismatch {
                path,
                expected,
                observed,
            } => write!(
                formatter,
                "active Luma host digest mismatch at {path}: expected {expected}, observed {observed}"
            ),
            Self::Observation { path, error } => {
                write!(
                    formatter,
                    "failed to observe active Luma host path {path}: {error}"
                )
            }
            Self::Snapshot(error) => error.fmt(formatter),
            Self::Domain(error) => error.fmt(formatter),
            Self::Effects(error) => error.fmt(formatter),
            Self::Lowering(error) => error.fmt(formatter),
        }
    }
}

impl Error for ManagedHostReleaseError {}

impl From<ProxyTopologyError> for ManagedHostReleaseError {
    fn from(error: ProxyTopologyError) -> Self {
        Self::Topology(error)
    }
}

impl From<LumaSnapshotInputError> for ManagedHostReleaseError {
    fn from(error: LumaSnapshotInputError) -> Self {
        Self::Snapshot(error)
    }
}

impl From<PeerTransitionError> for ManagedHostReleaseError {
    fn from(error: PeerTransitionError) -> Self {
        Self::Domain(error)
    }
}

impl From<LumaPeerEffectError> for ManagedHostReleaseError {
    fn from(error: LumaPeerEffectError) -> Self {
        Self::Effects(error)
    }
}

impl From<LumaHostLoweringError> for ManagedHostReleaseError {
    fn from(error: LumaHostLoweringError) -> Self {
        Self::Lowering(error)
    }
}

/// Lowers the release of one persisted active-topology Luma host claim.
///
/// No storage, topology mutation, or filesystem mutation is performed here.
/// Filesystem access is limited to retained no-follow observations under the
/// caller-authorized game root.
pub(super) fn lower_managed_host_release(
    topology: &GameProxyTopology,
    persisted: &ManagedAddonFile,
    game_root: &PathRef,
    accumulator: &mut LumaPeerEffectAccumulator,
) -> Result<(), ManagedHostReleaseError> {
    validate_topology_and_claim(topology, persisted)?;

    if persisted.mode() == ManagedFileMode::Reused {
        return Ok(());
    }

    let live_path = persisted.path();
    require_canonical_path(live_path)?;
    let live_snapshot = observe(live_path, game_root)?;
    require_digest(
        live_path,
        &live_snapshot,
        persisted.installed_sha256(),
        false,
    )?;

    let sidecar_path = managed_sidecar_path(live_path)?;
    match persisted.baseline() {
        ManagedFileBaseline::Absent => {
            let sidecar_snapshot = observe(&sidecar_path, game_root)?;
            require_absent(&sidecar_path, &sidecar_snapshot)?;
            lower_host_decision(
                LumaHostDecision::ReleaseAbsent {
                    live_path,
                    live_snapshot: &live_snapshot,
                },
                accumulator,
            )?;
        }
        ManagedFileBaseline::Present { sha256 } => {
            if persisted.installed_sha256() == sha256 {
                return Err(ManagedHostReleaseError::ClaimMismatch(
                    "owned host installed digest equals its baseline",
                ));
            }
            let sidecar_snapshot = observe(&sidecar_path, game_root)?;
            require_digest(&sidecar_path, &sidecar_snapshot, sha256, true)?;
            lower_host_decision(
                LumaHostDecision::ReleasePresent {
                    live_path,
                    sidecar_path: &sidecar_path,
                    live_snapshot: &live_snapshot,
                    sidecar_snapshot: &sidecar_snapshot,
                },
                accumulator,
            )?;
        }
    }
    Ok(())
}

fn validate_topology_and_claim(
    topology: &GameProxyTopology,
    persisted: &ManagedAddonFile,
) -> Result<(), ManagedHostReleaseError> {
    topology.validate()?;
    if topology.outer.implementation != ProxyImplementation::OptiScaler {
        return Err(ManagedHostReleaseError::InvalidTopology(
            "outer implementation is not OptiScaler",
        ));
    }
    let Some(downstream) = topology.downstream.as_ref() else {
        return Err(ManagedHostReleaseError::InvalidTopology(
            "topology has no active ReShade downstream",
        ));
    };
    if downstream.implementation != ProxyImplementation::ReShade {
        return Err(ManagedHostReleaseError::InvalidTopology(
            "downstream implementation is not ReShade",
        ));
    }
    if normalized_path_key(persisted.path().as_str())
        != normalized_path_key(downstream.path.as_str())
    {
        return Err(ManagedHostReleaseError::PathMismatch {
            persisted: persisted.path().clone(),
            topology: downstream.path.clone(),
        });
    }
    if downstream.receipt.digest() != persisted.installed_sha256() {
        return Err(ManagedHostReleaseError::DigestMismatch {
            path: persisted.path().clone(),
            expected: persisted.installed_sha256().clone(),
            observed: downstream.receipt.digest().clone(),
        });
    }
    let expected_ownership = match persisted.mode() {
        ManagedFileMode::Owned => FileOwnership::Owned,
        ManagedFileMode::Reused => FileOwnership::Reused,
    };
    if downstream.receipt.ownership() != expected_ownership {
        return Err(ManagedHostReleaseError::ClaimMismatch(
            "topology receipt ownership differs from persisted host mode",
        ));
    }
    match persisted.mode() {
        ManagedFileMode::Reused => match persisted.baseline() {
            ManagedFileBaseline::Present { sha256 } if sha256 == persisted.installed_sha256() => {
                Ok(())
            }
            _ => Err(ManagedHostReleaseError::ClaimMismatch(
                "reused host requires a present baseline equal to installed bytes",
            )),
        },
        ManagedFileMode::Owned => Ok(()),
    }
}

fn require_digest<'a>(
    path: &PathRef,
    snapshot: &'a PeerPathSnapshot,
    expected: &Sha256Hash,
    baseline: bool,
) -> Result<&'a crate::peer_mutation_executor::VerifiedPeerFile, ManagedHostReleaseError> {
    let image = require_file(path, snapshot)?;
    let bytes = snapshot_bytes(path, snapshot)?;
    ensure_bytes_match_image(path, &bytes, image, baseline)?;
    if image.digest() != expected {
        return Err(ManagedHostReleaseError::DigestMismatch {
            path: path.clone(),
            expected: expected.clone(),
            observed: image.digest().clone(),
        });
    }
    Ok(image)
}

fn observe(
    path: &PathRef,
    game_root: &PathRef,
) -> Result<PeerPathSnapshot, ManagedHostReleaseError> {
    observe_peer_path_snapshot(path, game_root).map_err(|error| {
        ManagedHostReleaseError::Observation {
            path: path.clone(),
            error,
        }
    })
}

fn require_canonical_path(path: &PathRef) -> Result<(), ManagedHostReleaseError> {
    if Path::new(path.as_str())
        .components()
        .any(|component| matches!(component, Component::CurDir | Component::ParentDir))
    {
        return Err(ManagedHostReleaseError::ClaimMismatch(
            "persisted host path is not canonical",
        ));
    }
    Ok(())
}
