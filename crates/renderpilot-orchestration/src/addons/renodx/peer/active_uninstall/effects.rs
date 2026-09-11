use renderpilot_domain::{NormalizedPathRelation, PeerEndpointRole, normalized_path_relation};

use crate::addons::shared_vulkan_mutation::FileIntent;
use crate::peer_mutation_executor::{
    EndpointExpectation, EndpointPostcondition, ExactEndpoint, PeerPathSnapshot,
};

use super::error::RenoDxActiveUninstallError;
use super::model::ActiveUninstallEndpointOwned;

/// Aligned endpoint, payload, and journal-intent outputs for active uninstall.
pub(super) struct ActiveUninstallEffects {
    endpoints: Vec<ExactEndpoint>,
    payloads: Vec<Option<Vec<u8>>>,
    intents: Vec<FileIntent>,
}

impl ActiveUninstallEffects {
    pub(super) const fn new() -> Self {
        Self {
            endpoints: Vec::new(),
            payloads: Vec::new(),
            intents: Vec::new(),
        }
    }

    pub(super) fn endpoints(&self) -> &[ExactEndpoint] {
        &self.endpoints
    }

    pub(super) fn into_parts(self) -> (Vec<ExactEndpoint>, Vec<Option<Vec<u8>>>, Vec<FileIntent>) {
        (self.endpoints, self.payloads, self.intents)
    }
}

pub(super) fn find_endpoint<'a>(
    endpoints: &'a [ActiveUninstallEndpointOwned],
    path: &renderpilot_domain::PathRef,
) -> Result<&'a ActiveUninstallEndpointOwned, RenoDxActiveUninstallError> {
    endpoints
        .iter()
        .find(|endpoint| {
            matches!(
                normalized_path_relation(endpoint.path().as_str(), path.as_str()),
                NormalizedPathRelation::Equal
            )
        })
        .ok_or_else(|| RenoDxActiveUninstallError::Path(std::path::PathBuf::from(path.as_str())))
}

pub(super) fn present_image(
    path: &renderpilot_domain::PathRef,
    snapshot: &PeerPathSnapshot,
) -> Result<crate::peer_mutation_executor::VerifiedPeerFile, RenoDxActiveUninstallError> {
    snapshot
        .file()
        .cloned()
        .ok_or_else(|| RenoDxActiveUninstallError::Path(std::path::PathBuf::from(path.as_str())))
}

pub(super) fn present_bytes(
    path: &renderpilot_domain::PathRef,
    snapshot: &PeerPathSnapshot,
) -> Result<Vec<u8>, RenoDxActiveUninstallError> {
    snapshot
        .bytes()
        .map(ToOwned::to_owned)
        .ok_or_else(|| RenoDxActiveUninstallError::Path(std::path::PathBuf::from(path.as_str())))
}

pub(super) fn image_for_bytes(
    bytes: &[u8],
) -> Result<crate::peer_mutation_executor::VerifiedPeerFile, RenoDxActiveUninstallError> {
    let digest = renderpilot_detection::sha256_bytes(bytes)
        .map_err(|_| RenoDxActiveUninstallError::Invalid("cannot hash active uninstall payload"))?;
    let identity = format!("active-uninstall:sha256:{}", digest.as_str());
    crate::peer_mutation_executor::VerifiedPeerFile::new_with_length(
        identity,
        digest,
        bytes.len() as u64,
    )
    .map_err(|_| RenoDxActiveUninstallError::Invalid("cannot build active uninstall payload image"))
}

pub(super) fn emit_remove(
    path: &renderpilot_domain::PathRef,
    role: PeerEndpointRole,
    before: crate::peer_mutation_executor::VerifiedPeerFile,
    before_bytes: Vec<u8>,
    effects: &mut ActiveUninstallEffects,
) -> Result<(), RenoDxActiveUninstallError> {
    effects.endpoints.push(ExactEndpoint::new(
        path.clone(),
        role,
        EndpointExpectation::File(before),
        EndpointPostcondition::Absent,
    ));
    effects.payloads.push(None);
    effects.intents.push(FileIntent {
        live_path: std::path::PathBuf::from(path.as_str()),
        before: Some(before_bytes),
        after: None,
    });
    Ok(())
}

pub(super) fn emit_replace(
    path: &renderpilot_domain::PathRef,
    role: PeerEndpointRole,
    before: crate::peer_mutation_executor::VerifiedPeerFile,
    before_bytes: Vec<u8>,
    after: &crate::peer_mutation_executor::VerifiedPeerFile,
    restored: Vec<u8>,
    effects: &mut ActiveUninstallEffects,
) -> Result<(), RenoDxActiveUninstallError> {
    effects.endpoints.push(ExactEndpoint::new(
        path.clone(),
        role,
        EndpointExpectation::File(before),
        EndpointPostcondition::File(after.digest().clone()),
    ));
    effects.payloads.push(Some(restored.clone()));
    effects.intents.push(FileIntent {
        live_path: std::path::PathBuf::from(path.as_str()),
        before: Some(before_bytes),
        after: Some(restored),
    });
    Ok(())
}
