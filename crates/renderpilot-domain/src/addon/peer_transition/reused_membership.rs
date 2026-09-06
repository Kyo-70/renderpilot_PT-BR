//! Aggregate-only membership changes for reused managed-file claims.
//!
//! A reused claim describes bytes owned by another peer. Adding or removing
//! that claim changes only the peer receipt; it never writes a filesystem
//! endpoint. The contract therefore carries only the exact live-byte guards
//! for the changed membership keys.

use std::collections::BTreeMap;

use crate::{
    GameProxyTopology, InstalledAddon, ManagedAddonFile, ManagedFileBaseline, ManagedFileMode,
    ProxyImplementation, normalized_path_key,
};

use super::claims::{PeerSnapshot, validate_snapshots};
use super::guards::{PeerReadGuardRequirement, reused_live_guard};
use super::metadata::validate_peer_metadata_refresh;
use super::model::PeerTransitionError;

/// A zero-write contract for adding and/or removing accepted reused claims.
///
/// The contract is intentionally opaque. Its guards are derived only from the
/// exact before/after aggregate images and cannot be supplied by an adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerReusedClaimMembershipContract {
    read_guards: Vec<PeerReadGuardRequirement>,
}

impl PeerReusedClaimMembershipContract {
    /// Derives a guarded aggregate-only membership change.
    ///
    /// The topology is supplied as one unchanged receipt. It must be a valid
    /// OptiScaler topology for the same game as both peer records. The peer
    /// records may refresh only the metadata fields already permitted by the
    /// metadata-only policy; their managed vectors may differ only by Reused
    /// claims whose complete records are preserved for retained keys.
    pub fn derive(
        before: &InstalledAddon,
        after: &InstalledAddon,
        unchanged_topology: &GameProxyTopology,
    ) -> Result<Self, PeerTransitionError> {
        validate_snapshots(
            Some(before),
            Some(after),
            Some(unchanged_topology),
            Some(unchanged_topology),
        )?;
        if unchanged_topology.outer.implementation != ProxyImplementation::OptiScaler {
            return Err(PeerTransitionError::InvalidTopologySnapshot(
                "reused-claim membership change requires an OptiScaler outer topology".to_owned(),
            ));
        }

        // Re-run the same snapshot validation used by physical transitions so
        // malformed deserialized records cannot enter this zero-write route.
        PeerSnapshot::from_peer(Some(before))?;
        PeerSnapshot::from_peer(Some(after))?;
        validate_peer_metadata_refresh(before, after)?;

        let before_claims = keyed_claims(before.managed_files())?;
        let after_claims = keyed_claims(after.managed_files())?;
        validate_reused_records(before_claims.values().copied())?;
        validate_reused_records(after_claims.values().copied())?;
        validate_retained_claims(&before_claims, &after_claims)?;
        let before_order = claim_order(before.managed_files());
        let after_order = claim_order(after.managed_files());
        validate_retained_order(&before_order, &after_order, &before_claims, &after_claims)?;

        let mut guards = BTreeMap::new();
        for (key, claim) in &after_claims {
            if !before_claims.contains_key(key) {
                validate_membership_delta(claim)?;
                insert_guard(&mut guards, claim)?;
            }
        }
        for (key, claim) in &before_claims {
            if !after_claims.contains_key(key) {
                validate_membership_delta(claim)?;
                insert_guard(&mut guards, claim)?;
            }
        }

        if guards.is_empty() {
            return Err(PeerTransitionError::InvalidPeerSnapshot(
                "reused-claim membership change must add or remove at least one claim",
            ));
        }

        Ok(Self {
            read_guards: guards.into_values().collect(),
        })
    }

    /// Returns canonical live guards for changed membership keys.
    #[must_use]
    pub fn read_guards(&self) -> &[PeerReadGuardRequirement] {
        &self.read_guards
    }
}

fn keyed_claims(
    claims: &[ManagedAddonFile],
) -> Result<BTreeMap<String, &ManagedAddonFile>, PeerTransitionError> {
    let mut keyed = BTreeMap::new();
    for claim in claims {
        if keyed
            .insert(normalized_path_key(claim.path().as_str()), claim)
            .is_some()
        {
            return Err(PeerTransitionError::DuplicateEndpoint(claim.path().clone()));
        }
    }
    Ok(keyed)
}

fn validate_reused_records<'a>(
    claims: impl IntoIterator<Item = &'a ManagedAddonFile>,
) -> Result<(), PeerTransitionError> {
    for claim in claims {
        if claim.mode() != ManagedFileMode::Reused {
            continue;
        }
        match claim.baseline() {
            ManagedFileBaseline::Present { sha256 } if sha256 == claim.installed_sha256() => {}
            ManagedFileBaseline::Present { .. } => {
                return Err(PeerTransitionError::ReusedClaimChanged(
                    claim.path().clone(),
                ));
            }
            ManagedFileBaseline::Absent => {
                return Err(PeerTransitionError::InvalidPeerSnapshot(
                    "reused claim must have a present baseline",
                ));
            }
        }
    }
    Ok(())
}

fn validate_retained_claims(
    before: &BTreeMap<String, &ManagedAddonFile>,
    after: &BTreeMap<String, &ManagedAddonFile>,
) -> Result<(), PeerTransitionError> {
    for (key, before_claim) in before {
        let Some(after_claim) = after.get(key) else {
            continue;
        };
        if *before_claim == *after_claim {
            continue;
        }
        if before_claim.mode() != after_claim.mode() {
            return Err(PeerTransitionError::InvalidManagedModeTransition(
                after_claim.path().clone(),
            ));
        }
        if before_claim.mode() == ManagedFileMode::Reused {
            return Err(PeerTransitionError::ReusedClaimChanged(
                after_claim.path().clone(),
            ));
        }
        return Err(PeerTransitionError::InvalidPeerSnapshot(
            "retained managed claim changed",
        ));
    }
    Ok(())
}

fn validate_retained_order(
    before_order: &[String],
    after_order: &[String],
    before: &BTreeMap<String, &ManagedAddonFile>,
    after: &BTreeMap<String, &ManagedAddonFile>,
) -> Result<(), PeerTransitionError> {
    let before_retained = before_order.iter().filter(|key| after.contains_key(*key));
    let after_retained = after_order.iter().filter(|key| before.contains_key(*key));
    if !before_retained.eq(after_retained) {
        return Err(PeerTransitionError::InvalidPeerSnapshot(
            "retained reused claims changed relative order",
        ));
    }
    Ok(())
}

fn claim_order(claims: &[ManagedAddonFile]) -> Vec<String> {
    claims
        .iter()
        .map(|claim| normalized_path_key(claim.path().as_str()))
        .collect()
}

fn validate_membership_delta(claim: &ManagedAddonFile) -> Result<(), PeerTransitionError> {
    if claim.mode() != ManagedFileMode::Reused {
        return Err(PeerTransitionError::InvalidManagedModeTransition(
            claim.path().clone(),
        ));
    }
    match claim.baseline() {
        ManagedFileBaseline::Present { sha256 } if sha256 == claim.installed_sha256() => Ok(()),
        ManagedFileBaseline::Present { .. } => Err(PeerTransitionError::ReusedClaimChanged(
            claim.path().clone(),
        )),
        ManagedFileBaseline::Absent => Err(PeerTransitionError::InvalidPeerSnapshot(
            "reused claim must have a present baseline",
        )),
    }
}

fn insert_guard(
    guards: &mut BTreeMap<String, PeerReadGuardRequirement>,
    claim: &ManagedAddonFile,
) -> Result<(), PeerTransitionError> {
    let key = normalized_path_key(claim.path().as_str());
    if guards
        .insert(
            key,
            reused_live_guard(claim.path().clone(), claim.installed_sha256().clone()),
        )
        .is_some()
    {
        return Err(PeerTransitionError::DuplicateEndpoint(claim.path().clone()));
    }
    Ok(())
}
