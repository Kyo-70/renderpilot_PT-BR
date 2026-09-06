//! Derivation of managed-file claims and their sidecar transitions.

use crate::{
    ManagedFileBaseline, ManagedFileMode, NormalizedPathRelation, PathRef, normalized_path_relation,
};

use super::super::EndpointGuard;
use super::super::claims::{ManagedClaim, PeerSnapshot};
use super::super::model::{
    PeerEndpointIntent, PeerEndpointOperation, PeerEndpointRole, PeerTransitionError,
};
use super::super::reconcile::DerivedEndpoint;
use super::helpers::add_endpoint;
use super::reused;

pub(super) fn derive(
    before: &PeerSnapshot,
    after: &PeerSnapshot,
    topology_path: Option<&PathRef>,
    endpoints: &mut Vec<DerivedEndpoint>,
) -> Result<(), PeerTransitionError> {
    let keys = before
        .managed
        .keys()
        .chain(after.managed.keys())
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    for key in keys {
        let before_claim = before.managed.get(&key);
        let after_claim = after.managed.get(&key);
        match (before_claim, after_claim) {
            (Some(before), Some(after))
                if before.mode == ManagedFileMode::Reused
                    && after.mode == ManagedFileMode::Reused =>
            {
                if before != after {
                    return Err(PeerTransitionError::ReusedClaimChanged(after.path.clone()));
                }
            }
            (Some(before), None) if before.mode == ManagedFileMode::Reused => {
                // Releasing a reused membership changes only the aggregate record.
                // The live bytes remain protected by the read-guard from the snapshot.
            }
            (None, Some(after)) if after.mode == ManagedFileMode::Reused => {
                // Acquiring a compatible file is a record-only membership addition.
            }
            (Some(before), Some(after))
                if before.mode == ManagedFileMode::Reused
                    && after.mode == ManagedFileMode::Owned =>
            {
                reused::validate_acquisition(before, after)?;
                reused::emit_acquisition(before, after, topology_path, endpoints)?;
            }
            (Some(before), Some(after))
                if before.mode != after.mode
                    || before.mode == ManagedFileMode::Reused
                    || after.mode == ManagedFileMode::Reused =>
            {
                return Err(PeerTransitionError::InvalidManagedModeTransition(
                    after.path.clone(),
                ));
            }
            (None, Some(after)) if after.mode == ManagedFileMode::Owned => {
                emit_owned_add(after, topology_path, endpoints)?;
            }
            (Some(before), None) if before.mode == ManagedFileMode::Owned => {
                emit_owned_remove(before, topology_path, endpoints)?;
            }
            (Some(before), Some(after)) => {
                emit_owned_change(before, after, topology_path, endpoints)?;
            }
            (None | Some(_), None) | (None, Some(_)) => {}
        }
    }
    Ok(())
}

fn emit_owned_add(
    claim: &ManagedClaim,
    topology_path: Option<&PathRef>,
    endpoints: &mut Vec<DerivedEndpoint>,
) -> Result<(), PeerTransitionError> {
    if topology_path.is_none_or(|path| {
        !matches!(
            normalized_path_relation(path.as_str(), claim.path.as_str()),
            NormalizedPathRelation::Equal
        )
    }) {
        let operation = if matches!(claim.baseline, ManagedFileBaseline::Absent) {
            PeerEndpointOperation::Create
        } else {
            PeerEndpointOperation::Replace
        };
        add_managed_live(
            endpoints,
            claim,
            operation,
            None,
            Some(claim.installed.clone()),
        )?;
    }
    if let ManagedFileBaseline::Present { sha256 } = &claim.baseline {
        add_managed_sidecar(
            endpoints,
            &claim.path,
            PeerEndpointOperation::Create,
            None,
            Some(sha256.clone()),
        )?;
    }
    Ok(())
}

fn emit_owned_remove(
    claim: &ManagedClaim,
    topology_path: Option<&PathRef>,
    endpoints: &mut Vec<DerivedEndpoint>,
) -> Result<(), PeerTransitionError> {
    if topology_path.is_none_or(|path| {
        !matches!(
            normalized_path_relation(path.as_str(), claim.path.as_str()),
            NormalizedPathRelation::Equal
        )
    }) {
        match &claim.baseline {
            ManagedFileBaseline::Absent => add_managed_live(
                endpoints,
                claim,
                PeerEndpointOperation::Remove,
                Some(claim.installed.clone()),
                None,
            )?,
            ManagedFileBaseline::Present { sha256 } => add_managed_live(
                endpoints,
                claim,
                PeerEndpointOperation::Replace,
                Some(claim.installed.clone()),
                Some(sha256.clone()),
            )?,
        }
    }
    if let ManagedFileBaseline::Present { sha256 } = &claim.baseline {
        add_managed_sidecar(
            endpoints,
            &claim.path,
            PeerEndpointOperation::Remove,
            Some(sha256.clone()),
            None,
        )?;
    }
    Ok(())
}

fn emit_owned_change(
    before: &ManagedClaim,
    after: &ManagedClaim,
    topology_path: Option<&PathRef>,
    endpoints: &mut Vec<DerivedEndpoint>,
) -> Result<(), PeerTransitionError> {
    if topology_path.is_none_or(|path| {
        !matches!(
            normalized_path_relation(path.as_str(), after.path.as_str()),
            NormalizedPathRelation::Equal
        )
    }) && before.installed != after.installed
    {
        add_managed_live(
            endpoints,
            after,
            PeerEndpointOperation::Replace,
            Some(before.installed.clone()),
            Some(after.installed.clone()),
        )?;
    }
    match (&before.baseline, &after.baseline) {
        (ManagedFileBaseline::Absent, ManagedFileBaseline::Present { sha256 }) => {
            add_managed_sidecar(
                endpoints,
                &after.path,
                PeerEndpointOperation::Create,
                None,
                Some(sha256.clone()),
            )?;
        }
        (ManagedFileBaseline::Present { sha256 }, ManagedFileBaseline::Absent) => {
            add_managed_sidecar(
                endpoints,
                &after.path,
                PeerEndpointOperation::Remove,
                Some(sha256.clone()),
                None,
            )?;
        }
        (
            ManagedFileBaseline::Present { sha256: before_sha },
            ManagedFileBaseline::Present { sha256: after_sha },
        ) if before_sha != after_sha => {
            add_managed_sidecar(
                endpoints,
                &after.path,
                PeerEndpointOperation::Replace,
                Some(before_sha.clone()),
                Some(after_sha.clone()),
            )?;
        }
        _ => {}
    }
    Ok(())
}

pub(super) fn add_managed_live(
    endpoints: &mut Vec<DerivedEndpoint>,
    claim: &ManagedClaim,
    operation: PeerEndpointOperation,
    before_sha256: Option<crate::Sha256Hash>,
    after_sha256: Option<crate::Sha256Hash>,
) -> Result<(), PeerTransitionError> {
    let intent = PeerEndpointIntent::new(
        claim.path.clone(),
        PeerEndpointRole::Disjoint,
        operation,
        after_sha256.clone(),
        None,
    )?;
    add_endpoint(
        endpoints,
        intent,
        EndpointGuard {
            before_sha256,
            after_sha256,
            ..EndpointGuard::default()
        },
    );
    Ok(())
}

pub(super) fn add_managed_sidecar(
    endpoints: &mut Vec<DerivedEndpoint>,
    live: &PathRef,
    operation: PeerEndpointOperation,
    before_sha256: Option<crate::Sha256Hash>,
    after_sha256: Option<crate::Sha256Hash>,
) -> Result<(), PeerTransitionError> {
    let path = super::super::claims::managed_sidecar_path(live)?;
    let intent = PeerEndpointIntent::new(
        path,
        PeerEndpointRole::Disjoint,
        operation,
        after_sha256.clone(),
        None,
    )?;
    add_endpoint(
        endpoints,
        intent,
        EndpointGuard {
            before_sha256,
            after_sha256,
            ..EndpointGuard::default()
        },
    );
    Ok(())
}
