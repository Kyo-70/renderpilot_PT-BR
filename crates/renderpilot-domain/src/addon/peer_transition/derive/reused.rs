//! Closed transition rules for reused managed-file memberships.

use crate::{
    ManagedFileBaseline, ManagedFileMode, NormalizedPathRelation, PathRef, normalized_path_relation,
};

use super::super::claims::{ManagedClaim, PeerSnapshot, managed_sidecar_path};
use super::super::model::{PeerEndpointIntent, PeerEndpointOperation, PeerTransitionError};
use super::super::reconcile::DerivedEndpoint;
use crate::ProxyLink;

/// Validates promotion of a reused file into an owned file.
pub(super) fn validate_acquisition(
    before: &ManagedClaim,
    after: &ManagedClaim,
) -> Result<(), PeerTransitionError> {
    let baseline_matches = matches!(
        &after.baseline,
        ManagedFileBaseline::Present { sha256 } if sha256 == &before.installed
    );
    if before.path != after.path || !baseline_matches {
        return Err(PeerTransitionError::InvalidManagedModeTransition(
            after.path.clone(),
        ));
    }
    Ok(())
}

/// Adds the exact sidecar-capture and live replacement for a reused promotion.
pub(super) fn emit_acquisition(
    before: &ManagedClaim,
    after: &ManagedClaim,
    topology_path: Option<&PathRef>,
    endpoints: &mut Vec<DerivedEndpoint>,
) -> Result<(), PeerTransitionError> {
    super::managed::add_managed_sidecar(
        endpoints,
        &after.path,
        PeerEndpointOperation::Create,
        None,
        Some(before.installed.clone()),
    )?;
    if topology_path.is_none_or(|path| {
        !matches!(
            normalized_path_relation(path.as_str(), after.path.as_str()),
            NormalizedPathRelation::Equal
        )
    }) {
        super::managed::add_managed_live(
            endpoints,
            after,
            PeerEndpointOperation::Replace,
            Some(before.installed.clone()),
            Some(after.installed.clone()),
        )?;
    }
    Ok(())
}

/// Validates the initial adoption edge where the topology already records a
/// reused downstream, but the peer aggregate has no prior record.  The
/// topology receipt is the only accepted before-image for this edge; a peer
/// record cannot be synthesized merely to satisfy the normal acquisition
/// path.
pub(super) fn validate_initial_topology_acquisition(
    before_link: &ProxyLink,
    after: &ManagedClaim,
    physical_program: &[PeerEndpointIntent],
) -> Result<(), PeerTransitionError> {
    if before_link.receipt.ownership() != crate::FileOwnership::Reused
        || !matches!(
            normalized_path_relation(before_link.path.as_str(), after.path.as_str()),
            NormalizedPathRelation::Equal
        )
        || after.mode != ManagedFileMode::Owned
        || !matches!(
            &after.baseline,
            ManagedFileBaseline::Present { sha256 } if sha256 == before_link.receipt.digest()
        )
    {
        return Err(PeerTransitionError::InvalidManagedDownstream(
            after.path.clone(),
        ));
    }

    let sidecar = managed_sidecar_path(&after.path)?;
    let mut sidecar_iter = physical_program
        .iter()
        .enumerate()
        .filter(|(_, intent)| {
            intent.role() == super::super::model::PeerEndpointRole::Disjoint
                && matches!(
                    normalized_path_relation(intent.path().as_str(), sidecar.as_str()),
                    NormalizedPathRelation::Equal
                )
        })
        .map(|(index, _)| index);
    let sidecar_index = sidecar_iter.next();
    let sidecar_unique = sidecar_iter.next().is_none();

    let mut topology_iter = physical_program
        .iter()
        .enumerate()
        .filter(|(_, intent)| {
            intent.role() == super::super::model::PeerEndpointRole::TopologyDownstream
                && matches!(
                    normalized_path_relation(intent.path().as_str(), after.path.as_str()),
                    NormalizedPathRelation::Equal
                )
        })
        .map(|(index, _)| index);
    let topology_index = topology_iter.next();
    let topology_unique = topology_iter.next().is_none();

    let (Some(sidecar_index), Some(topology_index)) = (sidecar_index, topology_index) else {
        return Err(PeerTransitionError::PhysicalProgramMismatch(
            after.path.clone(),
        ));
    };
    if !sidecar_unique || !topology_unique || topology_index != sidecar_index + 1 {
        return Err(PeerTransitionError::PhysicalProgramMismatch(
            after.path.clone(),
        ));
    }

    let sidecar_intent = &physical_program[sidecar_index];
    let topology_intent = &physical_program[topology_index];
    if sidecar_intent.operation() != PeerEndpointOperation::Create
        || sidecar_intent.planned_sha256() != Some(before_link.receipt.digest())
        || topology_intent.operation() != PeerEndpointOperation::Replace
    {
        return Err(PeerTransitionError::PhysicalProgramMismatch(
            after.path.clone(),
        ));
    }
    Ok(())
}

/// Requires the sidecar capture to be adjacent and before the live replacement.
pub(super) fn validate_acquisition_order(
    before: &PeerSnapshot,
    after: &PeerSnapshot,
    physical_program: &[PeerEndpointIntent],
) -> Result<(), PeerTransitionError> {
    for (key, before_claim) in &before.managed {
        let Some(after_claim) = after.managed.get(key) else {
            continue;
        };
        if before_claim.mode != ManagedFileMode::Reused
            || after_claim.mode != ManagedFileMode::Owned
        {
            continue;
        }
        validate_acquisition(before_claim, after_claim)?;
        let sidecar = managed_sidecar_path(&after_claim.path)?;
        let Some(sidecar_index) = physical_program.iter().position(|intent| {
            matches!(
                normalized_path_relation(intent.path().as_str(), sidecar.as_str()),
                NormalizedPathRelation::Equal
            )
        }) else {
            continue;
        };
        let Some(live_index) = physical_program.iter().position(|intent| {
            matches!(
                normalized_path_relation(intent.path().as_str(), after_claim.path.as_str()),
                NormalizedPathRelation::Equal
            )
        }) else {
            continue;
        };
        let sidecar_intent = &physical_program[sidecar_index];
        let live_intent = &physical_program[live_index];
        if sidecar_intent.operation() != PeerEndpointOperation::Create
            || live_intent.operation() != PeerEndpointOperation::Replace
            || live_index != sidecar_index + 1
        {
            return Err(PeerTransitionError::PhysicalProgramMismatch(
                after_claim.path.clone(),
            ));
        }
    }
    Ok(())
}
