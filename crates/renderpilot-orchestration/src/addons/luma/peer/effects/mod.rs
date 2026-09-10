//! Pure Luma peer effect accumulation.
//!
//! This module lowers already-observed images and already-prepared bytes into
//! one deterministic endpoint program. It owns no filesystem, storage, or
//! execution authority; those remain downstream of the finalized result.

mod error;
mod model;
mod validation;

use renderpilot_domain::{PathRef, PeerEndpointRole};

use crate::peer_mutation_executor::{
    EndpointExpectation, EndpointPostcondition, ExactEndpoint, ExactEndpointProgram,
    VerifiedPeerFile,
};

use self::model::{EndpointBundle, EndpointSpec};
use self::validation::{
    bundle_overlaps, ensure_distinct_pair_paths, group_rank, live_role, validate_group_roles,
};

pub(crate) use error::LumaPeerEffectError;
pub(crate) use model::{LumaPeerEffectGroup, LumaPeerOperationOrder};
pub(crate) use validation::ensure_bytes_match_image;

/// Mutable, Luma-specific collection of private endpoint bundles.
#[derive(Debug)]
pub(crate) struct LumaPeerEffectAccumulator {
    order: LumaPeerOperationOrder,
    bundles: Vec<EndpointBundle>,
}

impl LumaPeerEffectAccumulator {
    pub(crate) fn new(order: LumaPeerOperationOrder) -> Self {
        Self {
            order,
            bundles: Vec::new(),
        }
    }

    pub(crate) fn create(
        &mut self,
        group: LumaPeerEffectGroup,
        path: PathRef,
        bytes: Vec<u8>,
    ) -> Result<(), LumaPeerEffectError> {
        let role = live_role(group);
        self.push(EndpointBundle::Single {
            group,
            endpoint: EndpointSpec {
                path,
                role,
                before: EndpointExpectation::Absent,
                after: EndpointPostcondition::for_file_bytes(&bytes)
                    .map_err(|_| LumaPeerEffectError::InvalidPayloadDigest)?,
                payload: Some(bytes),
            },
        })
    }

    pub(crate) fn replace(
        &mut self,
        group: LumaPeerEffectGroup,
        path: PathRef,
        before: &VerifiedPeerFile,
        bytes: Vec<u8>,
    ) -> Result<(), LumaPeerEffectError> {
        let role = live_role(group);
        self.push(EndpointBundle::Single {
            group,
            endpoint: EndpointSpec {
                path,
                role,
                before: EndpointExpectation::File(before.clone()),
                after: EndpointPostcondition::for_file_bytes(&bytes)
                    .map_err(|_| LumaPeerEffectError::InvalidPayloadDigest)?,
                payload: Some(bytes),
            },
        })
    }

    pub(crate) fn remove(
        &mut self,
        group: LumaPeerEffectGroup,
        path: PathRef,
        before: &VerifiedPeerFile,
    ) -> Result<(), LumaPeerEffectError> {
        let role = live_role(group);
        self.push(EndpointBundle::Single {
            group,
            endpoint: EndpointSpec {
                path,
                role,
                before: EndpointExpectation::File(before.clone()),
                after: EndpointPostcondition::Absent,
                payload: None,
            },
        })
    }

    pub(crate) fn acquire_foreign(
        &mut self,
        group: LumaPeerEffectGroup,
        live_path: PathRef,
        sidecar_path: PathRef,
        live_before: &VerifiedPeerFile,
        original_bytes: Vec<u8>,
        prepared_bytes: Vec<u8>,
    ) -> Result<(), LumaPeerEffectError> {
        ensure_distinct_pair_paths(&live_path, &sidecar_path)?;
        ensure_bytes_match_image(&live_path, &original_bytes, live_before, false)?;
        self.push(EndpointBundle::Acquisition {
            group,
            sidecar: EndpointSpec {
                path: sidecar_path,
                role: PeerEndpointRole::Disjoint,
                before: EndpointExpectation::Absent,
                after: EndpointPostcondition::for_file_bytes(&original_bytes)
                    .map_err(|_| LumaPeerEffectError::InvalidPayloadDigest)?,
                payload: Some(original_bytes),
            },
            live: EndpointSpec {
                path: live_path,
                role: live_role(group),
                before: EndpointExpectation::File(live_before.clone()),
                after: EndpointPostcondition::for_file_bytes(&prepared_bytes)
                    .map_err(|_| LumaPeerEffectError::InvalidPayloadDigest)?,
                payload: Some(prepared_bytes),
            },
        })
    }

    pub(crate) fn release_present(
        &mut self,
        group: LumaPeerEffectGroup,
        live_path: PathRef,
        sidecar_path: PathRef,
        live_before: &VerifiedPeerFile,
        sidecar_before: &VerifiedPeerFile,
        baseline_bytes: Vec<u8>,
    ) -> Result<(), LumaPeerEffectError> {
        ensure_distinct_pair_paths(&live_path, &sidecar_path)?;
        ensure_bytes_match_image(&sidecar_path, &baseline_bytes, sidecar_before, true)?;
        self.push(EndpointBundle::Release {
            group,
            live: EndpointSpec {
                path: live_path,
                role: live_role(group),
                before: EndpointExpectation::File(live_before.clone()),
                after: EndpointPostcondition::for_file_bytes(&baseline_bytes)
                    .map_err(|_| LumaPeerEffectError::InvalidPayloadDigest)?,
                payload: Some(baseline_bytes),
            },
            sidecar: EndpointSpec {
                path: sidecar_path,
                role: PeerEndpointRole::Disjoint,
                before: EndpointExpectation::File(sidecar_before.clone()),
                after: EndpointPostcondition::Absent,
                payload: None,
            },
        })
    }

    /// Restore a missing DLSS-cascade live endpoint from the exact managed
    /// sidecar bytes, then remove that sidecar.  The explicit bundle preserves
    /// the required create-before-remove order and cannot be used by other
    /// effect groups.
    pub(crate) fn restore_absent(
        &mut self,
        group: LumaPeerEffectGroup,
        live_path: PathRef,
        sidecar_path: PathRef,
        sidecar_before: &VerifiedPeerFile,
        baseline_bytes: Vec<u8>,
    ) -> Result<(), LumaPeerEffectError> {
        if group != LumaPeerEffectGroup::DlssCascade {
            return Err(LumaPeerEffectError::DlssCascadeRestoreAbsentOnly);
        }
        ensure_distinct_pair_paths(&live_path, &sidecar_path)?;
        ensure_bytes_match_image(&sidecar_path, &baseline_bytes, sidecar_before, true)?;
        self.push(EndpointBundle::RestoreAbsent {
            group,
            live: EndpointSpec {
                path: live_path,
                role: live_role(group),
                before: EndpointExpectation::Absent,
                after: EndpointPostcondition::for_file_bytes(&baseline_bytes)
                    .map_err(|_| LumaPeerEffectError::InvalidPayloadDigest)?,
                payload: Some(baseline_bytes),
            },
            sidecar: EndpointSpec {
                path: sidecar_path,
                role: PeerEndpointRole::Disjoint,
                before: EndpointExpectation::File(sidecar_before.clone()),
                after: EndpointPostcondition::Absent,
                payload: None,
            },
        })
    }

    pub(crate) fn finalize(mut self) -> Result<Option<LumaPeerEffects>, LumaPeerEffectError> {
        if self.bundles.is_empty() {
            return Ok(None);
        }
        validate_group_roles(&self.bundles)?;
        self.bundles.sort_by(|left, right| {
            group_rank(self.order, left.group())
                .cmp(&group_rank(self.order, right.group()))
                .then_with(|| {
                    renderpilot_domain::normalized_path_key(left.logical_path().as_str()).cmp(
                        &renderpilot_domain::normalized_path_key(right.logical_path().as_str()),
                    )
                })
        });

        let endpoint_count = self.bundles.iter().map(EndpointBundle::len).sum();
        let mut endpoints = Vec::with_capacity(endpoint_count);
        let mut payloads = Vec::with_capacity(endpoint_count);
        for bundle in self.bundles {
            bundle.visit_owned(|spec| {
                let EndpointSpec {
                    path,
                    role,
                    before,
                    after,
                    payload,
                } = spec;
                endpoints.push(ExactEndpoint::new(path, role, before, after));
                payloads.push(payload);
            });
        }

        let program = ExactEndpointProgram::new(endpoints).map_err(LumaPeerEffectError::Program)?;
        Ok(Some(LumaPeerEffects { program, payloads }))
    }

    fn push(&mut self, bundle: EndpointBundle) -> Result<(), LumaPeerEffectError> {
        if self.bundles.iter().any(|existing| existing == &bundle) {
            return Ok(());
        }
        if self.bundles.iter().any(|existing| {
            renderpilot_domain::normalized_path_key(existing.logical_path().as_str())
                == renderpilot_domain::normalized_path_key(bundle.logical_path().as_str())
        }) {
            return Err(LumaPeerEffectError::ConflictingBundle(
                bundle.logical_path().clone(),
            ));
        }

        for existing in &self.bundles {
            if bundle_overlaps(existing, &bundle) {
                return Err(LumaPeerEffectError::OverlappingPaths(
                    existing.logical_path().clone(),
                    bundle.logical_path().clone(),
                ));
            }
        }
        self.bundles.push(bundle);
        Ok(())
    }
}

/// Finalized physical effects. The program and payload vector are immutable
/// through borrowing accessors and remain aligned by endpoint ordinal.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct LumaPeerEffects {
    program: ExactEndpointProgram,
    payloads: Vec<Option<Vec<u8>>>,
}

impl LumaPeerEffects {
    #[cfg(test)]
    pub(crate) fn program(&self) -> &ExactEndpointProgram {
        &self.program
    }

    #[cfg(test)]
    pub(crate) fn payloads(&self) -> &[Option<Vec<u8>>] {
        &self.payloads
    }

    pub(crate) fn into_parts(self) -> (ExactEndpointProgram, Vec<Option<Vec<u8>>>) {
        (self.program, self.payloads)
    }
}
