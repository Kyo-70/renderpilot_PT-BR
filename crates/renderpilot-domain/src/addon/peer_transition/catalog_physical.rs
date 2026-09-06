//! Catalog rollback to physical endpoint projection.

use std::collections::{BTreeMap, BTreeSet};

use crate::{InstalledAddon, ManagedFileMode, PathRef, Sha256Hash, normalized_path_key};

use super::EndpointGuard;
use super::catalog::PeerCatalogRollbackClaim;
use super::catalog_physical_helpers::*;
use super::guards::{PeerReadGuardExpectation, PeerReadGuardSource};
use super::model::{
    PeerEndpointIntent, PeerEndpointOperation, PeerEndpointRole, PeerFileImage, PeerTransitionError,
};
use super::reconcile::DerivedEndpoint;

/// One catalog-derived read guard, kept private to the peer transition layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CatalogGuard {
    pub(super) path: PathRef,
    pub(super) source: PeerReadGuardSource,
    pub(super) expectation: PeerReadGuardExpectation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CatalogBaselineSatisfaction {
    pub(super) live_path: PathRef,
    pub(super) baseline_sha256: Sha256Hash,
    pub(super) sidecar_path: PathRef,
}

/// Opaque physical projection of one exact catalog rollback claim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerCatalogPhysicalContract {
    before_peer: Option<InstalledAddon>,
    after_peer: Option<InstalledAddon>,
    physical_program: Vec<PeerEndpointIntent>,
    endpoints: Vec<DerivedEndpoint>,
    guards: Vec<CatalogGuard>,
    satisfactions: Vec<CatalogBaselineSatisfaction>,
    peer_required_live_paths: BTreeSet<String>,
}

impl PeerCatalogPhysicalContract {
    /// Derives catalog endpoints and catalog-owned read guards from one exact
    /// O1 physical program and its ordered preimages.
    pub fn derive(
        claim: &PeerCatalogRollbackClaim,
        before_peer: Option<&InstalledAddon>,
        after_peer: Option<&InstalledAddon>,
        physical_program: &[PeerEndpointIntent],
        preimages: &[Option<PeerFileImage>],
    ) -> Result<Self, PeerTransitionError> {
        if physical_program.len() != preimages.len() {
            return Err(PeerTransitionError::EvidenceCardinality {
                expected: physical_program.len(),
                actual: preimages.len(),
            });
        }
        validate_peer_games(claim, before_peer, after_peer)?;
        super::claims::validate_snapshots(before_peer, after_peer, None, None)?;
        let index = index_program(physical_program)?;
        let owned = owned_authority(before_peer);
        let reused = reused_paths(before_peer);
        let mut current = BTreeMap::new();
        let mut baseline = BTreeMap::new();
        let affected = claim
            .deleted_baselines()
            .iter()
            .map(super::catalog::PeerCatalogDeletedBaseline::component_id)
            .collect::<BTreeSet<_>>();
        for component in claim.before_components() {
            if !affected.contains(&component.id()) {
                continue;
            }
            for file in component.files() {
                let digest = active_digest(file, &owned, file.path())?;
                current.insert(normalized_path_key(file.path().as_str()), (file, digest));
            }
        }
        for entry in claim.deleted_baselines() {
            for file in entry.baseline().files() {
                let digest = file
                    .sha256()
                    .ok_or(PeerTransitionError::CatalogClaimInvalid(
                        "baseline file has no SHA-256",
                    ))?;
                baseline.insert(
                    normalized_path_key(file.path().as_str()),
                    (file, digest.clone()),
                );
            }
        }

        let mut endpoints = Vec::new();
        let mut guards = Vec::new();
        let mut satisfactions = Vec::new();
        let mut peer_required_live_paths = BTreeSet::new();
        for (key, (file, active)) in &current {
            if baseline.contains_key(key) {
                continue;
            }
            if reused.contains(key) {
                return Err(PeerTransitionError::CatalogPhysicalMismatch(
                    file.path().clone(),
                ));
            }
            let (index, intent) = required_endpoint(&index, physical_program, file.path())?;
            if intent.operation() != PeerEndpointOperation::Remove
                || intent.role() != PeerEndpointRole::Disjoint
            {
                return Err(PeerTransitionError::CatalogPhysicalMismatch(
                    file.path().clone(),
                ));
            }
            if owned.contains_key(key) {
                peer_required_live_paths.insert(key.clone());
            }
            require_image_digest(preimages[index].as_ref(), active, file.path())?;
            endpoints.push(DerivedEndpoint {
                intent: intent.clone(),
                guard: EndpointGuard {
                    before_sha256: Some(active.clone()),
                    before_length: preimages[index].as_ref().map(PeerFileImage::length),
                    ..EndpointGuard::default()
                },
            });
        }

        for (key, (baseline_file, baseline_digest)) in &baseline {
            let live_path = baseline_file.path();
            let sidecar = super::managed_sidecar_path(live_path)?;
            let sidecar_key = normalized_path_key(sidecar.as_str());
            let current_active = current
                .get(key)
                .map(|(_, digest)| digest)
                .or_else(|| owned.get(key).copied());
            let sidecar_index = index.get(&sidecar_key).copied();
            let live_index = index.get(key).copied();
            let sidecar_before = match sidecar_index {
                Some(index) => {
                    let intent = &physical_program[index];
                    if intent.operation() != PeerEndpointOperation::Remove
                        || intent.role() != PeerEndpointRole::Disjoint
                    {
                        return Err(PeerTransitionError::CatalogPhysicalMismatch(
                            sidecar.clone(),
                        ));
                    }
                    let observed = preimages[index].as_ref().ok_or_else(|| {
                        PeerTransitionError::CatalogPhysicalMismatch(sidecar.clone())
                    })?;
                    if observed.sha256() != baseline_digest {
                        return Err(PeerTransitionError::DigestMismatch(sidecar));
                    }
                    Some(observed)
                }
                None => None,
            };

            let Some(sidecar_before) = sidecar_before else {
                if current_active.is_some_and(|digest| digest == baseline_digest)
                    && current.contains_key(key)
                    && live_index.is_none()
                {
                    add_catalog_guard(
                        &mut guards,
                        live_path.clone(),
                        PeerReadGuardSource::CatalogBaselineLive,
                        PeerReadGuardExpectation::Digest {
                            sha256: baseline_digest.clone(),
                        },
                    );
                    add_catalog_guard(
                        &mut guards,
                        sidecar,
                        PeerReadGuardSource::CatalogBaselineSidecar,
                        PeerReadGuardExpectation::Absent,
                    );
                    continue;
                }
                return Err(PeerTransitionError::CatalogPhysicalMismatch(
                    live_path.clone(),
                ));
            };

            let Some(sidecar_index) = sidecar_index else {
                return Err(PeerTransitionError::CatalogPhysicalMismatch(sidecar));
            };
            endpoints.push(DerivedEndpoint {
                intent: physical_program[sidecar_index].clone(),
                guard: EndpointGuard {
                    before_sha256: Some(baseline_digest.clone()),
                    before_length: Some(sidecar_before.length()),
                    ..EndpointGuard::default()
                },
            });

            match current_active {
                Some(active) if active != baseline_digest => {
                    if reused.contains(key) {
                        return Err(PeerTransitionError::CatalogPhysicalMismatch(
                            live_path.clone(),
                        ));
                    }
                    if !current.contains_key(key) && !owned.contains_key(key) {
                        return Err(PeerTransitionError::CatalogPhysicalMismatch(
                            live_path.clone(),
                        ));
                    }
                    let index = live_index.ok_or_else(|| {
                        PeerTransitionError::CatalogPhysicalMismatch(live_path.clone())
                    })?;
                    let intent = &physical_program[index];
                    let Some(before) = preimages[index].as_ref() else {
                        return Err(PeerTransitionError::CatalogPhysicalMismatch(
                            live_path.clone(),
                        ));
                    };
                    if intent.operation() != PeerEndpointOperation::Replace
                        || intent.role() != PeerEndpointRole::Disjoint
                        || before.sha256() != active
                    {
                        return Err(PeerTransitionError::CatalogPhysicalMismatch(
                            live_path.clone(),
                        ));
                    }
                    if owned.contains_key(key) {
                        peer_required_live_paths.insert(key.clone());
                    }
                    require_postimage(intent, baseline_digest, sidecar_before.length(), live_path)?;
                    endpoints.push(DerivedEndpoint {
                        intent: intent.clone(),
                        guard: EndpointGuard {
                            before_sha256: Some(active.clone()),
                            before_length: Some(before.length()),
                            after_sha256: Some(baseline_digest.clone()),
                            after_length: Some(sidecar_before.length()),
                        },
                    });
                }
                Some(_) => {
                    if live_index.is_some() {
                        return Err(PeerTransitionError::CatalogPhysicalMismatch(
                            live_path.clone(),
                        ));
                    }
                    if owned.contains_key(key) {
                        peer_required_live_paths.insert(key.clone());
                    }
                    satisfactions.push(CatalogBaselineSatisfaction {
                        live_path: live_path.clone(),
                        baseline_sha256: baseline_digest.clone(),
                        sidecar_path: sidecar,
                    });
                    add_catalog_guard(
                        &mut guards,
                        live_path.clone(),
                        PeerReadGuardSource::CatalogBaselineLive,
                        PeerReadGuardExpectation::Digest {
                            sha256: baseline_digest.clone(),
                        },
                    );
                }
                None => {
                    if reused.contains(key) {
                        return Err(PeerTransitionError::CatalogPhysicalMismatch(
                            live_path.clone(),
                        ));
                    }
                    let index = live_index.ok_or_else(|| {
                        PeerTransitionError::CatalogPhysicalMismatch(live_path.clone())
                    })?;
                    let intent = &physical_program[index];
                    if intent.operation() != PeerEndpointOperation::Create
                        || intent.role() != PeerEndpointRole::Disjoint
                        || preimages[index].is_some()
                    {
                        return Err(PeerTransitionError::CatalogPhysicalMismatch(
                            live_path.clone(),
                        ));
                    }
                    require_postimage(intent, baseline_digest, sidecar_before.length(), live_path)?;
                    endpoints.push(DerivedEndpoint {
                        intent: intent.clone(),
                        guard: EndpointGuard {
                            after_sha256: Some(baseline_digest.clone()),
                            after_length: Some(sidecar_before.length()),
                            ..EndpointGuard::default()
                        },
                    });
                }
            }
        }

        Ok(Self {
            before_peer: before_peer.cloned(),
            after_peer: after_peer.cloned(),
            physical_program: physical_program.to_vec(),
            endpoints,
            guards,
            satisfactions,
            peer_required_live_paths,
        })
    }

    /// Reconstructs the physical contract from its exact domain inputs.
    pub fn from_parts(
        claim: &PeerCatalogRollbackClaim,
        before_peer: Option<&InstalledAddon>,
        after_peer: Option<&InstalledAddon>,
        physical_program: &[PeerEndpointIntent],
        preimages: &[Option<PeerFileImage>],
    ) -> Result<Self, PeerTransitionError> {
        Self::derive(claim, before_peer, after_peer, physical_program, preimages)
    }

    pub(super) fn matches_inputs(
        &self,
        before_peer: Option<&InstalledAddon>,
        after_peer: Option<&InstalledAddon>,
        physical_program: &[PeerEndpointIntent],
    ) -> bool {
        self.before_peer.as_ref() == before_peer
            && self.after_peer.as_ref() == after_peer
            && self.physical_program == physical_program
    }

    pub(super) fn endpoints(&self) -> &[DerivedEndpoint] {
        &self.endpoints
    }

    pub(super) fn guards(&self) -> &[CatalogGuard] {
        &self.guards
    }

    pub(super) fn satisfactions(&self) -> &[CatalogBaselineSatisfaction] {
        &self.satisfactions
    }

    pub(super) fn requires_peer_endpoint(&self, path: &PathRef) -> bool {
        self.peer_required_live_paths
            .contains(&normalized_path_key(path.as_str()))
    }

    pub(super) fn can_discharge_peer_release(
        &self,
        path: &PathRef,
        before_peer: Option<&InstalledAddon>,
        after_peer: Option<&InstalledAddon>,
        peer_endpoint: &DerivedEndpoint,
    ) -> bool {
        let Some(satisfaction) = self.satisfactions.iter().find(|satisfaction| {
            normalized_path_key(satisfaction.live_path.as_str())
                == normalized_path_key(path.as_str())
        }) else {
            return false;
        };
        let Some(before) = before_peer else {
            return false;
        };
        if after_peer.is_some_and(|after| {
            after.managed_files().iter().any(|file| {
                normalized_path_key(file.path().as_str()) == normalized_path_key(path.as_str())
            })
        }) {
            return false;
        }
        let Some(file) = before.managed_files().iter().find(|file| {
            file.mode() == ManagedFileMode::Owned
                && normalized_path_key(file.path().as_str()) == normalized_path_key(path.as_str())
                && matches!(file.baseline(), crate::ManagedFileBaseline::Present { .. })
        }) else {
            return false;
        };
        let crate::ManagedFileBaseline::Present { sha256 } = file.baseline() else {
            return false;
        };
        let Some(sidecar) = self.endpoints.iter().find(|endpoint| {
            normalized_path_key(endpoint.intent.path().as_str())
                == normalized_path_key(satisfaction.sidecar_path.as_str())
        }) else {
            return false;
        };
        sha256 == &satisfaction.baseline_sha256
            && sidecar.intent.operation() == PeerEndpointOperation::Remove
            && sidecar.intent.role() == PeerEndpointRole::Disjoint
            && peer_endpoint.intent.operation() == PeerEndpointOperation::Replace
            && peer_endpoint.intent.role() == PeerEndpointRole::Disjoint
            && peer_endpoint.intent.planned_sha256() == Some(&satisfaction.baseline_sha256)
    }
}
