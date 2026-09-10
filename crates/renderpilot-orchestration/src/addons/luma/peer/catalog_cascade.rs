//! Read-only lowering of selected catalog rollback plans into Luma DLSS peer
//! effects.
//!
//! Catalog selection and filesystem mutation stay outside this adapter.  It
//! only projects the selected current/baseline file images, observes retained
//! snapshots under an immutable sealed Luma authority, and delegates the closed physical
//! decision to [`super::cascade`].

use std::collections::BTreeMap;
use std::{error::Error, fmt};

use renderpilot_domain::{
    ComponentFile, PathRef, PeerTransitionError, Sha256Hash, managed_sidecar_path,
};

use crate::ServiceError;
use crate::catalog::cascade::ValidatedRollbackPlan;
use crate::peer_mutation_executor::{PeerPathSnapshot, observe_peer_path_snapshot};

use super::cascade::{
    LumaDlssCascadeDecision, LumaDlssCascadeLoweringError, lower_dlss_cascade_decision,
};
use super::effects::LumaPeerEffectAccumulator;
use super::root_authority::LumaPeerRootAuthority;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LumaCatalogCascadeError {
    DuplicatePath(PathRef),
    MissingDigest(PathRef),
    Authority { path: PathRef, error: ServiceError },
    Observation { path: PathRef, error: ServiceError },
    Domain(PeerTransitionError),
    Lowering(LumaDlssCascadeLoweringError),
}

impl fmt::Display for LumaCatalogCascadeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicatePath(path) => {
                write!(formatter, "duplicate catalog cascade path: {path}")
            }
            Self::MissingDigest(path) => {
                write!(
                    formatter,
                    "catalog cascade path has no SHA-256 digest: {path}"
                )
            }
            Self::Authority { path, error } => {
                write!(
                    formatter,
                    "catalog cascade path is outside the sealed Luma roots {path}: {error}"
                )
            }
            Self::Observation { path, error } => {
                write!(
                    formatter,
                    "failed to observe catalog cascade path {path}: {error}"
                )
            }
            Self::Domain(error) => error.fmt(formatter),
            Self::Lowering(error) => error.fmt(formatter),
        }
    }
}

impl Error for LumaCatalogCascadeError {}

impl From<PeerTransitionError> for LumaCatalogCascadeError {
    fn from(error: PeerTransitionError) -> Self {
        Self::Domain(error)
    }
}

impl From<LumaDlssCascadeLoweringError> for LumaCatalogCascadeError {
    fn from(error: LumaDlssCascadeLoweringError) -> Self {
        Self::Lowering(error)
    }
}

#[derive(Debug, Default)]
struct LogicalFile<'a> {
    current: Option<&'a ComponentFile>,
    baseline: Option<&'a ComponentFile>,
}

/// Lowers all selected catalog rollback plans into an existing accumulator.
///
/// The plan files and their sealed-root authority are validated before any
/// snapshot is observed. The map also gives multi-plan input deterministic
/// behavior while preserving the accumulator's global effect ordering.
pub(crate) fn lower_catalog_cascade(
    plans: &[ValidatedRollbackPlan],
    authority: &LumaPeerRootAuthority,
    accumulator: &mut LumaPeerEffectAccumulator,
) -> Result<(), LumaCatalogCascadeError> {
    let entries = collect_logical_files(plans)?;
    for entry in entries.values() {
        require_digests(entry)?;
    }
    for entry in entries.values() {
        validate_authority(entry, authority)?;
    }
    for entry in entries.values() {
        lower_logical_file(entry, authority, accumulator)?;
    }
    Ok(())
}

fn collect_logical_files(
    plans: &[ValidatedRollbackPlan],
) -> Result<BTreeMap<String, LogicalFile<'_>>, LumaCatalogCascadeError> {
    let mut all = BTreeMap::new();
    for plan in plans {
        let mut per_plan = BTreeMap::new();
        add_files(&mut per_plan, plan.current_files(), true)?;
        add_files(&mut per_plan, plan.baseline_files(), false)?;
        for (key, entry) in per_plan {
            if let Some(existing) = all.insert(key, entry) {
                let path = existing
                    .current
                    .or(existing.baseline)
                    .expect("logical file entry is non-empty")
                    .path()
                    .clone();
                return Err(LumaCatalogCascadeError::DuplicatePath(path));
            }
        }
    }
    Ok(all)
}

fn add_files<'a>(
    entries: &mut BTreeMap<String, LogicalFile<'a>>,
    files: &'a [ComponentFile],
    current: bool,
) -> Result<(), LumaCatalogCascadeError> {
    for file in files {
        let key = crate::paths::normalized_key(std::path::Path::new(file.path().as_str()));
        let entry = entries.entry(key).or_default();
        let slot = if current {
            &mut entry.current
        } else {
            &mut entry.baseline
        };
        if slot.replace(file).is_some() {
            return Err(LumaCatalogCascadeError::DuplicatePath(file.path().clone()));
        }
    }
    Ok(())
}

fn require_digests(entry: &LogicalFile<'_>) -> Result<(), LumaCatalogCascadeError> {
    for file in [entry.current, entry.baseline].into_iter().flatten() {
        if file.sha256().is_none() {
            return Err(LumaCatalogCascadeError::MissingDigest(file.path().clone()));
        }
    }
    Ok(())
}

fn lower_logical_file(
    entry: &LogicalFile<'_>,
    authority: &LumaPeerRootAuthority,
    accumulator: &mut LumaPeerEffectAccumulator,
) -> Result<(), LumaCatalogCascadeError> {
    match (entry.current, entry.baseline) {
        (Some(current), None) => {
            let live_path = current.path();
            let live_snapshot = observe(live_path, authority)?;
            let decision = LumaDlssCascadeDecision::CurrentOnly {
                live_path,
                live_snapshot: &live_snapshot,
                expected_active_digest: digest(current)?,
            };
            lower_dlss_cascade_decision(decision, accumulator)?;
        }
        (Some(current), Some(baseline)) => {
            let live_path = current.path();
            let sidecar_path = managed_sidecar_path(live_path)?;
            let live_snapshot = observe(live_path, authority)?;
            let sidecar_snapshot = observe(&sidecar_path, authority)?;
            let active_digest = digest(current)?;
            let baseline_digest = digest(baseline)?;
            let decision = if active_digest == baseline_digest {
                match &sidecar_snapshot {
                    PeerPathSnapshot::Absent => {
                        LumaDlssCascadeDecision::BaselineAlreadyLiveWithoutSidecar {
                            live_path,
                            sidecar_path: &sidecar_path,
                            live_snapshot: &live_snapshot,
                            sidecar_snapshot: &sidecar_snapshot,
                            expected_active_digest: active_digest,
                            expected_baseline_digest: baseline_digest,
                        }
                    }
                    PeerPathSnapshot::File(_) => LumaDlssCascadeDecision::BaselineAlreadyLive {
                        live_path,
                        sidecar_path: &sidecar_path,
                        live_snapshot: &live_snapshot,
                        sidecar_snapshot: &sidecar_snapshot,
                        expected_active_digest: active_digest,
                        expected_baseline_digest: baseline_digest,
                    },
                }
            } else {
                LumaDlssCascadeDecision::RestorePresent {
                    live_path,
                    sidecar_path: &sidecar_path,
                    live_snapshot: &live_snapshot,
                    sidecar_snapshot: &sidecar_snapshot,
                    expected_active_digest: active_digest,
                    expected_baseline_digest: baseline_digest,
                }
            };
            lower_dlss_cascade_decision(decision, accumulator)?;
        }
        (None, Some(baseline)) => {
            let live_path = baseline.path();
            let sidecar_path = managed_sidecar_path(live_path)?;
            let live_snapshot = observe(live_path, authority)?;
            let sidecar_snapshot = observe(&sidecar_path, authority)?;
            let decision = LumaDlssCascadeDecision::RestoreMissingLive {
                live_path,
                sidecar_path: &sidecar_path,
                live_snapshot: &live_snapshot,
                sidecar_snapshot: &sidecar_snapshot,
                expected_baseline_digest: digest(baseline)?,
            };
            lower_dlss_cascade_decision(decision, accumulator)?;
        }
        (None, None) => unreachable!("logical file entry is non-empty"),
    }
    Ok(())
}

fn observe(
    path: &PathRef,
    authority: &LumaPeerRootAuthority,
) -> Result<PeerPathSnapshot, LumaCatalogCascadeError> {
    let authorized_root =
        authority
            .authorized_root(path)
            .map_err(|error| LumaCatalogCascadeError::Authority {
                path: path.clone(),
                error,
            })?;
    observe_peer_path_snapshot(path, authorized_root).map_err(|error| {
        LumaCatalogCascadeError::Observation {
            path: path.clone(),
            error,
        }
    })
}

fn validate_authority(
    entry: &LogicalFile<'_>,
    authority: &LumaPeerRootAuthority,
) -> Result<(), LumaCatalogCascadeError> {
    for file in [entry.current, entry.baseline].into_iter().flatten() {
        authority.authorized_root(file.path()).map_err(|error| {
            LumaCatalogCascadeError::Authority {
                path: file.path().clone(),
                error,
            }
        })?;
        let sidecar = managed_sidecar_path(file.path())?;
        authority.authorized_root(&sidecar).map_err(|error| {
            LumaCatalogCascadeError::Authority {
                path: sidecar,
                error,
            }
        })?;
    }
    Ok(())
}

fn digest(file: &ComponentFile) -> Result<&Sha256Hash, LumaCatalogCascadeError> {
    file.sha256()
        .ok_or_else(|| LumaCatalogCascadeError::MissingDigest(file.path().clone()))
}
