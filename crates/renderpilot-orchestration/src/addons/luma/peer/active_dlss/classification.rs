use renderpilot_domain::{ManagedAddonFile, ManagedFileBaseline, PathRef};

use crate::addons::luma::peer::effects::ensure_bytes_match_image;
use crate::coordinated_files::CatalogPathClaim;
use crate::peer_mutation_executor::{PeerPathSnapshot, VerifiedPeerFile};

use super::model::{
    ActiveDlssClassification, ActiveDlssClassificationError, ActiveDlssOwnedPlan,
    ActiveDlssOwnedTransition,
};

/// Classifies a bundled DLSS payload against one retained live endpoint.
pub(crate) fn classify_active_dlss(
    target: &PathRef,
    bundled_bytes: Option<Vec<u8>>,
    claim: &CatalogPathClaim,
    live_snapshot: &PeerPathSnapshot,
) -> Result<ActiveDlssClassification, ActiveDlssClassificationError> {
    if !is_dlss_target(target) {
        return Err(ActiveDlssClassificationError::InvalidTarget(target.clone()));
    }

    let Some(bundled_bytes) = bundled_bytes else {
        return Ok(ActiveDlssClassification::NoPayload);
    };
    let bundled_info = renderpilot_detection::DlssBinaryInfo::from_bytes(&bundled_bytes)
        .map_err(|error| ActiveDlssClassificationError::InvalidBundled(error.to_string()))?;
    let bundled_hash = renderpilot_detection::sha256_bytes(&bundled_bytes)
        .map_err(|error| ActiveDlssClassificationError::InvalidBundled(error.to_string()))?;

    let Some(live) = inspect_live_snapshot(target, live_snapshot)? else {
        if !claim.active_hashes().is_empty() {
            return Err(ActiveDlssClassificationError::CatalogMissing(
                target.clone(),
            ));
        }
        return Ok(ActiveDlssClassification::Owned(ActiveDlssOwnedPlan {
            target: target.clone(),
            binding: ManagedAddonFile::owned(
                target.clone(),
                ManagedFileBaseline::Absent,
                bundled_hash,
            ),
            bundled_bytes,
            transition: ActiveDlssOwnedTransition::Create,
        }));
    };

    if !claim.active_hashes().is_empty()
        && !claim.active_hashes().contains(&live.file.digest().clone())
    {
        return Err(ActiveDlssClassificationError::CatalogDrift {
            path: target.clone(),
            expected: claim.active_hashes().to_vec(),
            observed: live.file.digest().clone(),
        });
    }

    if !renderpilot_domain::dlss::versions_are_compatible(
        live.info.version(),
        bundled_info.version(),
    ) {
        return if claim.active_hashes().is_empty() {
            Err(ActiveDlssClassificationError::IncompatibleLive {
                path: target.clone(),
                live_version: live.info.version().to_string(),
                bundled_version: bundled_info.version().to_string(),
            })
        } else {
            Err(ActiveDlssClassificationError::CatalogReplacementForbidden {
                path: target.clone(),
                reason: "live binary is from an incompatible DLSS generation",
            })
        };
    }

    if live.info.version() >= bundled_info.version() {
        return Ok(ActiveDlssClassification::Reused {
            binding: ManagedAddonFile::reused(target.clone(), live.file.digest().clone()),
        });
    }

    if !claim.active_hashes().is_empty() {
        return Err(ActiveDlssClassificationError::CatalogReplacementForbidden {
            path: target.clone(),
            reason: "live catalog binary is older than the bundled binary",
        });
    }

    Ok(ActiveDlssClassification::Owned(ActiveDlssOwnedPlan {
        target: target.clone(),
        binding: ManagedAddonFile::owned(
            target.clone(),
            ManagedFileBaseline::Present {
                sha256: live.file.digest().clone(),
            },
            bundled_hash,
        ),
        bundled_bytes,
        transition: ActiveDlssOwnedTransition::Replace {
            live_digest: live.file.digest().clone(),
        },
    }))
}

struct LiveDlss<'a> {
    file: &'a VerifiedPeerFile,
    info: renderpilot_detection::DlssBinaryInfo,
}

fn inspect_live_snapshot<'a>(
    target: &PathRef,
    snapshot: &'a PeerPathSnapshot,
) -> Result<Option<LiveDlss<'a>>, ActiveDlssClassificationError> {
    let Some(file) = snapshot.file() else {
        return Ok(None);
    };
    let bytes = snapshot
        .bytes()
        .ok_or_else(|| ActiveDlssClassificationError::SnapshotMissingBytes(target.clone()))?;
    ensure_bytes_match_image(target, bytes, file, false)
        .map_err(|_| ActiveDlssClassificationError::SnapshotImageMismatch(target.clone()))?;
    let info = renderpilot_detection::DlssBinaryInfo::from_bytes(bytes).map_err(|error| {
        ActiveDlssClassificationError::InvalidLive {
            path: target.clone(),
            detail: error.to_string(),
        }
    })?;
    Ok(Some(LiveDlss { file, info }))
}

fn is_dlss_target(target: &PathRef) -> bool {
    target
        .as_str()
        .rsplit(['/', '\\'])
        .next()
        .is_some_and(|name| name.eq_ignore_ascii_case(renderpilot_detection::NVNGX_DLSS_FILE_NAME))
}
