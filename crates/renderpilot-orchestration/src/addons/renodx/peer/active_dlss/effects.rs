use std::path::PathBuf;

use renderpilot_domain::{PathRef, PeerEndpointRole, RenoDxDlssBeforeImage};

use crate::addons::shared_vulkan_mutation::FileIntent;
use crate::peer_mutation_executor::{
    EndpointExpectation, EndpointPostcondition, ExactEndpoint, PeerPathSnapshot, VerifiedPeerFile,
};

use super::error::ActiveDlssError;
use super::model::ActiveDlssEffect;

/// One lowered endpoint plus the aligned native payload and game intent.
#[derive(Debug)]
pub(super) struct LoweredEndpoint {
    pub(super) endpoint: ExactEndpoint,
    pub(super) payload: Option<Vec<u8>>,
    pub(super) intent: FileIntent,
}

/// Lowers one sealed endpoint. `None` means the effect is a byte-level no-op.
pub(super) fn lower_endpoint(
    path: &PathRef,
    snapshot: &PeerPathSnapshot,
    effect: &ActiveDlssEffect,
    role: PeerEndpointRole,
) -> Result<Option<LoweredEndpoint>, ActiveDlssError> {
    let before = snapshot_image(path, snapshot)?;
    match effect {
        ActiveDlssEffect::Unchanged => Ok(None),
        ActiveDlssEffect::Write(bytes) => {
            let after = image_for_bytes(path, bytes)?;
            if snapshot.bytes() == Some(bytes.as_slice()) {
                return Ok(None);
            }
            let (expectation, before_bytes) = match before {
                None => (EndpointExpectation::Absent, None),
                Some(image) => (
                    EndpointExpectation::File(image),
                    Some(snapshot_bytes(path, snapshot)?.to_owned()),
                ),
            };
            Ok(Some(LoweredEndpoint {
                endpoint: ExactEndpoint::new(
                    path.clone(),
                    role,
                    expectation,
                    EndpointPostcondition::File(after.digest().clone()),
                ),
                payload: Some(bytes.clone()),
                intent: FileIntent {
                    live_path: PathBuf::from(path.as_str()),
                    before: before_bytes,
                    after: Some(bytes.clone()),
                },
            }))
        }
        ActiveDlssEffect::Remove => {
            let Some(before) = before else {
                return Ok(None);
            };
            let before_bytes = snapshot_bytes(path, snapshot)?.to_owned();
            Ok(Some(LoweredEndpoint {
                endpoint: ExactEndpoint::new(
                    path.clone(),
                    role,
                    EndpointExpectation::File(before),
                    EndpointPostcondition::Absent,
                ),
                payload: None,
                intent: FileIntent {
                    live_path: PathBuf::from(path.as_str()),
                    before: Some(before_bytes),
                    after: None,
                },
            }))
        }
    }
}

/// Verifies that a retained snapshot's digest and length describe its bytes.
pub(super) fn snapshot_image(
    path: &PathRef,
    snapshot: &PeerPathSnapshot,
) -> Result<Option<VerifiedPeerFile>, ActiveDlssError> {
    let Some(file) = snapshot.file() else {
        if snapshot.bytes().is_some() {
            return Err(ActiveDlssError::Invalid(
                "absent DLSS-Fix snapshot carries retained bytes",
            ));
        }
        return Ok(None);
    };
    let bytes = snapshot_bytes(path, snapshot)?;
    let digest = renderpilot_detection::sha256_bytes(bytes)
        .map_err(|_| ActiveDlssError::Invalid("cannot hash retained DLSS-Fix endpoint bytes"))?;
    if digest != *file.digest() {
        return Err(ActiveDlssError::Invalid(
            "retained DLSS-Fix endpoint digest differs from its bytes",
        ));
    }
    if file.length() != bytes.len() as u64 {
        return Err(ActiveDlssError::Invalid(
            "retained DLSS-Fix endpoint length differs from its bytes",
        ));
    }
    Ok(Some(file.clone()))
}

/// Converts the validated retained endpoint into the domain before-image.
pub(super) fn before_image(
    path: &PathRef,
    snapshot: &PeerPathSnapshot,
) -> Result<RenoDxDlssBeforeImage, ActiveDlssError> {
    let Some(file) = snapshot_image(path, snapshot)? else {
        return Ok(RenoDxDlssBeforeImage::absent());
    };
    let bytes = snapshot_bytes(path, snapshot)?;
    Ok(RenoDxDlssBeforeImage::present(
        file.identity().to_owned(),
        file.digest().clone(),
        file.length(),
        bytes.to_vec(),
    )?)
}

fn snapshot_bytes<'a>(
    path: &PathRef,
    snapshot: &'a PeerPathSnapshot,
) -> Result<&'a [u8], ActiveDlssError> {
    snapshot
        .bytes()
        .ok_or_else(|| ActiveDlssError::Path(path.clone()))
}

fn image_for_bytes(path: &PathRef, bytes: &[u8]) -> Result<VerifiedPeerFile, ActiveDlssError> {
    let digest = renderpilot_detection::sha256_bytes(bytes)
        .map_err(|_| ActiveDlssError::Path(path.clone()))?;
    VerifiedPeerFile::new_with_length(
        format!("active-dlss:sha256:{}", digest.as_str()),
        digest,
        bytes.len() as u64,
    )
    .map_err(ActiveDlssError::Program)
}
