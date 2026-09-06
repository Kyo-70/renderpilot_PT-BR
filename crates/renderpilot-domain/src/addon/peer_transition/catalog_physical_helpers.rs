//! Small closed helpers for the catalog physical projection.

use std::collections::{BTreeMap, BTreeSet};

use crate::{
    ComponentFile, InstalledAddon, PathRef, Sha256Hash, normalized_path_key,
    normalized_path_relation,
};

use super::catalog::PeerCatalogRollbackClaim;
use super::catalog_physical::CatalogGuard;
use super::guards::{PeerReadGuardExpectation, PeerReadGuardSource};
use super::model::{PeerEndpointIntent, PeerFileImage, PeerTransitionError};

pub(super) fn validate_peer_games(
    claim: &PeerCatalogRollbackClaim,
    before_peer: Option<&InstalledAddon>,
    after_peer: Option<&InstalledAddon>,
) -> Result<(), PeerTransitionError> {
    for peer in [before_peer, after_peer].into_iter().flatten() {
        if peer.game_id() != claim.game_id() {
            return Err(PeerTransitionError::InvalidPeerSnapshot(
                "peer and catalog rollback belong to different games",
            ));
        }
    }
    Ok(())
}

pub(super) fn index_program(
    intents: &[PeerEndpointIntent],
) -> Result<BTreeMap<String, usize>, PeerTransitionError> {
    let mut index = BTreeMap::new();
    for (position, intent) in intents.iter().enumerate() {
        if index
            .insert(normalized_path_key(intent.path().as_str()), position)
            .is_some()
        {
            return Err(PeerTransitionError::DuplicateEndpoint(
                intent.path().clone(),
            ));
        }
    }
    for (position, left) in intents.iter().enumerate() {
        for right in intents.iter().skip(position + 1) {
            if normalized_path_relation(left.path().as_str(), right.path().as_str()).overlaps() {
                return Err(PeerTransitionError::OverlappingEndpoints(
                    left.path().clone(),
                    right.path().clone(),
                ));
            }
        }
    }
    Ok(index)
}

pub(super) fn reused_paths(peer: Option<&InstalledAddon>) -> BTreeSet<String> {
    peer.into_iter()
        .flat_map(InstalledAddon::managed_files)
        .filter(|file| file.mode() == crate::ManagedFileMode::Reused)
        .map(|file| normalized_path_key(file.path().as_str()))
        .collect()
}

pub(super) fn owned_authority(peer: Option<&InstalledAddon>) -> BTreeMap<String, &Sha256Hash> {
    peer.into_iter()
        .flat_map(InstalledAddon::managed_files)
        .filter(|file| file.mode() == crate::ManagedFileMode::Owned)
        .map(|file| {
            (
                normalized_path_key(file.path().as_str()),
                file.installed_sha256(),
            )
        })
        .collect()
}

pub(super) fn active_digest<'a>(
    file: &'a ComponentFile,
    owned: &BTreeMap<String, &'a Sha256Hash>,
    path: &PathRef,
) -> Result<Sha256Hash, PeerTransitionError> {
    owned
        .get(&normalized_path_key(path.as_str()))
        .map(|digest| (*digest).clone())
        .or_else(|| file.sha256().cloned())
        .ok_or(PeerTransitionError::CatalogClaimInvalid(
            "current catalog file has no SHA-256",
        ))
}

pub(super) fn required_endpoint<'a>(
    index: &'a BTreeMap<String, usize>,
    intents: &'a [PeerEndpointIntent],
    path: &PathRef,
) -> Result<(usize, &'a PeerEndpointIntent), PeerTransitionError> {
    let position = index
        .get(&normalized_path_key(path.as_str()))
        .copied()
        .ok_or_else(|| PeerTransitionError::UnclaimedPhysicalEndpoint(path.clone()))?;
    Ok((position, &intents[position]))
}

pub(super) fn require_image_digest(
    observed: Option<&PeerFileImage>,
    expected: &Sha256Hash,
    path: &PathRef,
) -> Result<(), PeerTransitionError> {
    if observed.is_none_or(|image| image.sha256() != expected) {
        return Err(PeerTransitionError::DigestMismatch(path.clone()));
    }
    Ok(())
}

pub(super) fn require_postimage(
    intent: &PeerEndpointIntent,
    digest: &Sha256Hash,
    length: u64,
    path: &PathRef,
) -> Result<(), PeerTransitionError> {
    if intent.planned_sha256() != Some(digest) || intent.planned_length() != Some(length) {
        return Err(PeerTransitionError::CatalogPhysicalMismatch(path.clone()));
    }
    Ok(())
}

pub(super) fn add_catalog_guard(
    guards: &mut Vec<CatalogGuard>,
    path: PathRef,
    source: PeerReadGuardSource,
    expectation: PeerReadGuardExpectation,
) {
    guards.push(CatalogGuard {
        path,
        source,
        expectation,
    });
}
