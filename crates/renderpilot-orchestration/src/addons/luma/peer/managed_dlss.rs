//! Read-only lowering for one persisted Luma-managed DLSS uninstall claim.
//!
//! The persisted claim is the authority for the physical endpoint.  This
//! adapter only accepts the release shapes produced for that claim, observes
//! the live/managed-sidecar pair, and delegates endpoint construction to the
//! existing generic lowerer.

use std::{
    error::Error,
    fmt,
    path::{Component, Path},
};

use renderpilot_domain::{
    ManagedAddonFile, ManagedFileBaseline, ManagedFileMode, PathRef, PeerTransitionError,
    Sha256Hash, managed_sidecar_path, normalized_path_key,
};

use crate::peer_mutation_executor::{PeerPathSnapshot, observe_peer_path_snapshot};
use crate::{ServiceError, coordinated_files::CoordinatedFilePlan};

use super::effects::{LumaPeerEffectAccumulator, LumaPeerEffectError, ensure_bytes_match_image};
use super::generic::{LumaGenericDecision, LumaGenericLoweringError, lower_generic_decision};
use super::snapshot_input::{LumaSnapshotInputError, require_absent, require_file, snapshot_bytes};
use crate::addons::luma::dlss::PlannedDlss;

/// Failure to lower a persisted managed DLSS release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ManagedDlssUninstallError {
    InvalidPlan(&'static str),
    PathMismatch {
        persisted: PathRef,
        planned: PathRef,
    },
    InvalidExpectedLive(PathRef),
    InstalledBaselineEqual(PathRef),
    ExpectedDigestMismatch {
        path: PathRef,
        expected: Sha256Hash,
        observed: Sha256Hash,
    },
    Observation {
        path: PathRef,
        error: ServiceError,
    },
    Domain(PeerTransitionError),
    Effects(LumaPeerEffectError),
    Lowering(LumaGenericLoweringError),
}

impl fmt::Display for ManagedDlssUninstallError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPlan(reason) => {
                write!(formatter, "invalid managed DLSS uninstall plan: {reason}")
            }
            Self::PathMismatch { persisted, planned } => write!(
                formatter,
                "managed DLSS uninstall path {planned} does not match persisted claim {persisted}"
            ),
            Self::InvalidExpectedLive(path) => {
                write!(
                    formatter,
                    "managed DLSS uninstall has an invalid expected live set: {path}"
                )
            }
            Self::InstalledBaselineEqual(path) => write!(
                formatter,
                "managed DLSS uninstall cannot replace byte-identical live and baseline files: {path}"
            ),
            Self::ExpectedDigestMismatch {
                path,
                expected,
                observed,
            } => write!(
                formatter,
                "managed DLSS uninstall digest mismatch at {path}: expected {expected}, observed {observed}"
            ),
            Self::Observation { path, error } => {
                write!(
                    formatter,
                    "failed to observe managed DLSS path {path}: {error}"
                )
            }
            Self::Domain(error) => error.fmt(formatter),
            Self::Effects(error) => error.fmt(formatter),
            Self::Lowering(error) => error.fmt(formatter),
        }
    }
}

impl Error for ManagedDlssUninstallError {}

impl From<LumaSnapshotInputError> for ManagedDlssUninstallError {
    fn from(error: LumaSnapshotInputError) -> Self {
        Self::Lowering(error.into())
    }
}

impl From<PeerTransitionError> for ManagedDlssUninstallError {
    fn from(error: PeerTransitionError) -> Self {
        Self::Domain(error)
    }
}

impl From<LumaPeerEffectError> for ManagedDlssUninstallError {
    fn from(error: LumaPeerEffectError) -> Self {
        Self::Effects(error)
    }
}

impl From<LumaGenericLoweringError> for ManagedDlssUninstallError {
    fn from(error: LumaGenericLoweringError) -> Self {
        Self::Lowering(error)
    }
}

enum ReleaseShape {
    Remove,
    Restore { baseline_sha256: Sha256Hash },
}

/// Lowers a prepared release for the exact persisted managed DLSS claim.
///
/// This function has no storage or mutation authority.  It observes each
/// retained filesystem snapshot under `authorized_root`; all writes remain in the
/// finalized peer effects consumed by the lifecycle owner.
pub(super) fn lower_managed_dlss_uninstall(
    persisted: &ManagedAddonFile,
    plan: &PlannedDlss,
    authorized_root: &PathRef,
    accumulator: &mut LumaPeerEffectAccumulator,
) -> Result<(), ManagedDlssUninstallError> {
    if persisted.mode() == ManagedFileMode::Reused {
        return lower_reused(plan);
    }

    if plan.binding.is_some() {
        return Err(ManagedDlssUninstallError::InvalidPlan(
            "release plan must not retain a managed binding",
        ));
    }

    let shape = release_shape(persisted, plan)?;
    let live_path = persisted.path();
    require_canonical_path(live_path)?;
    let live_path_for_plan = match action_path(plan) {
        Some(path) => path_ref_from_path(path)?,
        None => {
            return Err(ManagedDlssUninstallError::InvalidPlan(
                "owned release must use a physical release action",
            ));
        }
    };
    require_canonical_path(&live_path_for_plan)?;
    if normalized_path_key(live_path.as_str()) != normalized_path_key(live_path_for_plan.as_str()) {
        return Err(ManagedDlssUninstallError::PathMismatch {
            persisted: live_path.clone(),
            planned: live_path_for_plan,
        });
    }

    let live_snapshot = observe(live_path, authorized_root)?;
    require_digest(
        live_path,
        &live_snapshot,
        persisted.installed_sha256(),
        false,
    )?;

    match shape {
        ReleaseShape::Remove => {
            let sidecar_path = managed_sidecar_path(live_path)?;
            let sidecar_snapshot = observe(&sidecar_path, authorized_root)?;
            require_absent(&sidecar_path, &sidecar_snapshot)?;
            lower_generic_decision(
                LumaGenericDecision::RemoveCreated {
                    live_path,
                    live_snapshot: &live_snapshot,
                },
                accumulator,
            )?;
        }
        ReleaseShape::Restore { baseline_sha256 } => {
            let sidecar_path = managed_sidecar_path(live_path)?;
            let sidecar_snapshot = observe(&sidecar_path, authorized_root)?;
            require_digest(&sidecar_path, &sidecar_snapshot, &baseline_sha256, true)?;
            lower_generic_decision(
                LumaGenericDecision::ReleaseBacked {
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

fn lower_reused(plan: &PlannedDlss) -> Result<(), ManagedDlssUninstallError> {
    if plan.binding.is_some() || !matches!(plan.action, CoordinatedFilePlan::Keep) {
        return Err(ManagedDlssUninstallError::InvalidPlan(
            "reused claim may only be released by an unbound Keep plan",
        ));
    }
    Ok(())
}

fn release_shape(
    persisted: &ManagedAddonFile,
    plan: &PlannedDlss,
) -> Result<ReleaseShape, ManagedDlssUninstallError> {
    match (persisted.baseline(), &plan.action) {
        (
            ManagedFileBaseline::Absent,
            CoordinatedFilePlan::RemoveAndRelease { expected_live, .. },
        ) => {
            require_expected_live(
                persisted.path(),
                expected_live,
                persisted.installed_sha256(),
            )?;
            Ok(ReleaseShape::Remove)
        }
        (
            ManagedFileBaseline::Present { sha256 },
            CoordinatedFilePlan::RestoreAndRelease {
                baseline_sha256,
                expected_live,
                ..
            },
        ) => {
            if baseline_sha256 != sha256 {
                return Err(ManagedDlssUninstallError::InvalidPlan(
                    "restore baseline does not match persisted claim",
                ));
            }
            require_expected_live(
                persisted.path(),
                expected_live,
                persisted.installed_sha256(),
            )?;
            if persisted.installed_sha256() == sha256 {
                return Err(ManagedDlssUninstallError::InstalledBaselineEqual(
                    persisted.path().clone(),
                ));
            }
            Ok(ReleaseShape::Restore {
                baseline_sha256: sha256.clone(),
            })
        }
        (ManagedFileBaseline::Absent, _) => Err(ManagedDlssUninstallError::InvalidPlan(
            "absent baseline requires RemoveAndRelease",
        )),
        (ManagedFileBaseline::Present { .. }, _) => Err(ManagedDlssUninstallError::InvalidPlan(
            "present baseline requires RestoreAndRelease",
        )),
    }
}

fn require_expected_live(
    path: &PathRef,
    expected_live: &[Sha256Hash],
    installed: &Sha256Hash,
) -> Result<(), ManagedDlssUninstallError> {
    if expected_live.is_empty() || !expected_live.contains(installed) {
        return Err(ManagedDlssUninstallError::InvalidExpectedLive(path.clone()));
    }
    Ok(())
}

fn require_digest<'a>(
    path: &PathRef,
    snapshot: &'a PeerPathSnapshot,
    expected: &Sha256Hash,
    baseline: bool,
) -> Result<&'a crate::peer_mutation_executor::VerifiedPeerFile, ManagedDlssUninstallError> {
    let image = require_file(path, snapshot)?;
    let bytes = snapshot_bytes(path, snapshot)?;
    ensure_bytes_match_image(path, &bytes, image, baseline)?;
    if image.digest() != expected {
        return Err(ManagedDlssUninstallError::ExpectedDigestMismatch {
            path: path.clone(),
            expected: expected.clone(),
            observed: image.digest().clone(),
        });
    }
    Ok(image)
}

fn observe(
    path: &PathRef,
    authorized_root: &PathRef,
) -> Result<PeerPathSnapshot, ManagedDlssUninstallError> {
    observe_peer_path_snapshot(path, authorized_root).map_err(|error| {
        ManagedDlssUninstallError::Observation {
            path: path.clone(),
            error,
        }
    })
}

fn path_ref_from_path(path: &Path) -> Result<PathRef, ManagedDlssUninstallError> {
    let value = path.to_str().ok_or(ManagedDlssUninstallError::InvalidPlan(
        "release action path is not valid UTF-8",
    ))?;
    PathRef::new(value.to_owned()).map_err(|_| {
        ManagedDlssUninstallError::InvalidPlan("release action path is not a valid path")
    })
}

fn require_canonical_path(path: &PathRef) -> Result<(), ManagedDlssUninstallError> {
    if Path::new(path.as_str())
        .components()
        .any(|component| matches!(component, Component::CurDir | Component::ParentDir))
    {
        return Err(ManagedDlssUninstallError::InvalidPlan(
            "managed DLSS endpoint path is not canonical",
        ));
    }
    Ok(())
}

fn action_path(plan: &PlannedDlss) -> Option<&Path> {
    match &plan.action {
        CoordinatedFilePlan::RestoreAndRelease { path, .. }
        | CoordinatedFilePlan::RemoveAndRelease { path, .. } => Some(path),
        _ => None,
    }
}
