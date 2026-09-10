use renderpilot_domain::{ManagedFileBaseline, PathRef};

use crate::addons::luma::peer::effects::{
    LumaPeerEffectAccumulator, LumaPeerEffectGroup, ensure_bytes_match_image,
};
use crate::peer_mutation_executor::PeerPathSnapshot;

use super::model::{ActiveDlssLoweringError, ActiveDlssOwnedPlan, ActiveDlssOwnedTransition};

/// Lowers an owned active-install plan into the existing DLSS effect group.
///
/// The caller can obtain a sidecar request only from an `Owned` plan. The
/// lowerer verifies the retained live image and requires the exact managed
/// sidecar to be absent before adding any endpoint.
pub(crate) fn lower_active_dlss_owned(
    plan: ActiveDlssOwnedPlan,
    live_snapshot: &PeerPathSnapshot,
    sidecar_snapshot: &PeerPathSnapshot,
    accumulator: &mut LumaPeerEffectAccumulator,
) -> Result<(), ActiveDlssLoweringError> {
    let sidecar_path = plan.sidecar_path()?;
    require_absent_owned(&sidecar_path, sidecar_snapshot)?;

    match plan.transition {
        ActiveDlssOwnedTransition::Create => {
            require_absent_owned(&plan.target, live_snapshot)?;
            if !matches!(plan.binding.baseline(), ManagedFileBaseline::Absent) {
                return Err(ActiveDlssLoweringError::BindingMismatch(
                    plan.target.clone(),
                ));
            }
            accumulator.create(
                LumaPeerEffectGroup::DlssCascade,
                plan.target,
                plan.bundled_bytes,
            )?;
        }
        ActiveDlssOwnedTransition::Replace { live_digest } => {
            let live_file = require_file_owned(&plan.target, live_snapshot)?;
            let live_bytes = live_snapshot.bytes().ok_or_else(|| {
                ActiveDlssLoweringError::SnapshotMissingBytes(plan.target.clone())
            })?;
            ensure_bytes_match_image(&plan.target, live_bytes, live_file, false)
                .map_err(|_| ActiveDlssLoweringError::SnapshotImageMismatch(plan.target.clone()))?;
            if live_file.digest() != &live_digest
                || plan.binding.baseline()
                    != &(ManagedFileBaseline::Present {
                        sha256: live_digest,
                    })
            {
                return Err(ActiveDlssLoweringError::BindingMismatch(
                    plan.target.clone(),
                ));
            }
            accumulator.acquire_foreign(
                LumaPeerEffectGroup::DlssCascade,
                plan.target,
                sidecar_path,
                live_file,
                live_bytes.to_vec(),
                plan.bundled_bytes,
            )?;
        }
    }
    Ok(())
}

fn require_absent_owned(
    path: &PathRef,
    snapshot: &PeerPathSnapshot,
) -> Result<(), ActiveDlssLoweringError> {
    if matches!(snapshot, PeerPathSnapshot::Absent) {
        Ok(())
    } else {
        Err(ActiveDlssLoweringError::SnapshotExpectedAbsent(
            path.clone(),
        ))
    }
}

fn require_file_owned<'a>(
    path: &PathRef,
    snapshot: &'a PeerPathSnapshot,
) -> Result<&'a crate::peer_mutation_executor::VerifiedPeerFile, ActiveDlssLoweringError> {
    snapshot
        .file()
        .ok_or_else(|| ActiveDlssLoweringError::SnapshotExpectedFile(path.clone()))
}
