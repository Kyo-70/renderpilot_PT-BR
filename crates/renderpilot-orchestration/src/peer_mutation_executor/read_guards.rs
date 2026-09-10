//! Shared no-follow peer observations and storage-bound read-guard evidence.

use renderpilot_domain::{
    PathRef, PeerFileImage, PeerReadGuardEvidence, PeerReadGuardRequirement, validate_read_guards,
};

use super::{EndpointObservation, PeerMutationPackage, VerifiedPeerFile};
use crate::{ServiceError, addons::errors};

pub(crate) fn observe_peer_path_evidence(
    path: &PathRef,
) -> Result<EndpointObservation, ServiceError> {
    let (identity, digest, length) = match super::observation::observe_peer_path(path)? {
        super::observation::PeerPathObservation::Absent { .. } => {
            return Ok(EndpointObservation::Absent);
        }
        super::observation::PeerPathObservation::File {
            bytes, observation, ..
        } => {
            let digest = observation.digest.ok_or_else(|| {
                errors::invalid(format!("peer path has no digest: {}", path.as_str()))
            })?;
            let length = u64::try_from(bytes.len()).map_err(|error| {
                errors::invalid(format!("peer endpoint length overflow: {error}"))
            })?;
            (observation.identity, digest, length)
        }
    };
    let digest = renderpilot_domain::Sha256Hash::new(digest)
        .map_err(|error| errors::invalid(error.to_string()))?;
    Ok(EndpointObservation::File(
        VerifiedPeerFile::new_with_length(identity, digest, length)
            .map_err(|error| errors::invalid(error.to_string()))?,
    ))
}

pub(crate) fn observe_peer_path(path: &PathRef) -> Result<EndpointObservation, ServiceError> {
    observe_peer_path_evidence(path)
}

pub(crate) fn observe_peer_read_guards(
    package: &PeerMutationPackage<'_>,
) -> Result<Vec<PeerReadGuardEvidence>, ServiceError> {
    observe_read_guard_requirements(package.read_guards())
}

/// Observes one immutable, already-derived requirement slice using the same
/// no-follow endpoint reader as ordinary peer packages.
///
/// The helper is intentionally requirement-based: aggregate-only routes do not
/// have a physical package, while ordinary routes continue to delegate here
/// without changing their observation or validation behavior.
pub(super) fn observe_read_guard_requirements(
    requirements: &[PeerReadGuardRequirement],
) -> Result<Vec<PeerReadGuardEvidence>, ServiceError> {
    let evidence = requirements
        .iter()
        .map(|requirement| {
            let observed = match observe_peer_path_evidence(requirement.path())? {
                EndpointObservation::Absent => None,
                EndpointObservation::File(file) => Some(
                    PeerFileImage::new(
                        file.identity().to_owned(),
                        file.digest().clone(),
                        file.length(),
                    )
                    .map_err(|error| crate::failed(error.to_string()))?,
                ),
            };
            Ok(PeerReadGuardEvidence::new(
                requirement.path().clone(),
                observed,
            ))
        })
        .collect::<Result<Vec<_>, ServiceError>>()?;
    validate_read_guards(requirements, &evidence)
        .map_err(|error| crate::failed(format!("peer read-guard observation rejected: {error}")))?;
    Ok(evidence)
}
