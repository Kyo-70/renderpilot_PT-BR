//! Planning boundary for an aggregate-only reused-claim membership change.
//!
//! Changing reused managed-file membership changes the peer receipt but does not
//! change any file.  It therefore has its own immutable package instead of
//! being represented as a synthetic no-op endpoint in the ordinary peer
//! mutation package.

use std::path::PathBuf;

use renderpilot_domain::{
    GameProxyTopology, InstalledAddon, PeerReadGuardRequirement, PeerReusedClaimMembershipContract,
};

use super::roots::PeerRoots;
use crate::ServiceError;
use crate::peer_mutation_executor::render_peer_roots;

/// Inputs for planning one active peer's aggregate-only reused-claim membership change.
///
/// The roots are supplied by the caller's already-sealed operation authority.
/// They are canonicalized exactly once by [`PeerRoots::new`]; topology paths
/// are always authorized by the first (game) root, while membership guards may be
/// authorized by either explicitly sealed root.
pub(crate) struct PeerAggregateMembershipRequest<'a> {
    pub(crate) before_peer: &'a InstalledAddon,
    pub(crate) after_peer: &'a InstalledAddon,
    pub(crate) unchanged_topology: &'a GameProxyTopology,
    pub(crate) game_root: PathBuf,
    pub(crate) payload_root: Option<PathBuf>,
}

/// Immutable authority package for a zero-write reused-claim membership change.
///
/// This package deliberately contains no physical endpoint program, payload,
/// ancestor plan, catalog projection, baseline mutation, pending-mutation
/// state, recovery state, or filesystem timestamp.  The executor can only
/// pass the exact aggregate images and the storage-derived guard projection to
/// the storage CAS boundary.
#[derive(Debug, Clone)]
pub(crate) struct PeerAggregateMembershipPackage {
    before_peer: InstalledAddon,
    after_peer: InstalledAddon,
    unchanged_topology: GameProxyTopology,
    contract: PeerReusedClaimMembershipContract,
    canonical_game_root: String,
    sealed_roots: Vec<String>,
}

/// Owned data that crosses from sealed membership planning into the one
/// aggregate commit ceremony. Keeping these values together preserves the
/// package's move-only boundary while allowing the executor to borrow each
/// projection for its corresponding preflight or storage operation.
pub(crate) struct PeerAggregateMembershipCommitInputs {
    before_peer: InstalledAddon,
    after_peer: InstalledAddon,
    unchanged_topology: GameProxyTopology,
    read_guards: Vec<PeerReadGuardRequirement>,
    canonical_game_root: String,
    sealed_roots: Vec<String>,
}

impl PeerAggregateMembershipCommitInputs {
    pub(crate) fn game_id(&self) -> &renderpilot_domain::GameId {
        self.before_peer.game_id()
    }

    pub(crate) fn before_peer(&self) -> &InstalledAddon {
        &self.before_peer
    }

    pub(crate) fn after_peer(&self) -> &InstalledAddon {
        &self.after_peer
    }

    pub(crate) fn unchanged_topology(&self) -> &GameProxyTopology {
        &self.unchanged_topology
    }

    pub(crate) fn read_guards(&self) -> &[PeerReadGuardRequirement] {
        &self.read_guards
    }

    pub(crate) fn canonical_game_root(&self) -> &str {
        &self.canonical_game_root
    }

    pub(crate) fn sealed_roots(&self) -> &[String] {
        &self.sealed_roots
    }
}

impl PeerAggregateMembershipPackage {
    /// Plans and seals the complete root/aggregate/guard projection.
    pub(crate) fn plan_active(
        request: PeerAggregateMembershipRequest<'_>,
    ) -> Result<Self, ServiceError> {
        let roots = PeerRoots::new(request.game_root, request.payload_root)?;

        // A payload root can authorize a reused live file, but it cannot
        // authorize any topology participant.  This keeps the outer proxy
        // aggregate tied to the canonical game root even when ReShade's
        // AddonPath is elsewhere.
        for path in request.unchanged_topology.participant_paths() {
            roots.require_game_path(path).map_err(|error| {
                crate::failed(format!(
                    "peer topology participant is outside the explicit game root: {error}"
                ))
            })?;
        }

        let contract = PeerReusedClaimMembershipContract::derive(
            request.before_peer,
            request.after_peer,
            request.unchanged_topology,
        )
        .map_err(|error| {
            crate::failed(format!(
                "peer aggregate membership change rejected: {error}"
            ))
        })?;
        let read_guards = contract.read_guards().to_vec();
        for requirement in &read_guards {
            roots
                .require_sealed_path(requirement.path())
                .map_err(|error| {
                    crate::failed(format!(
                        "peer aggregate membership guard is outside the explicit sealed roots: {error}"
                    ))
                })?;
        }

        let sealed_roots = render_peer_roots(roots.roots())
            .map_err(|error| crate::failed(format!("invalid peer membership root: {error}")))?;
        let canonical_game_root = sealed_roots.first().cloned().ok_or_else(|| {
            crate::failed("peer aggregate membership change has no canonical game root")
        })?;

        Ok(Self {
            before_peer: request.before_peer.clone(),
            after_peer: request.after_peer.clone(),
            unchanged_topology: request.unchanged_topology.clone(),
            contract,
            canonical_game_root,
            sealed_roots,
        })
    }

    /// Consumes the sealed package before the executor observes live guards
    /// and prepares the storage CAS. No caller can reuse a mutable planning
    /// input between those steps.
    pub(crate) fn into_commit_inputs(self) -> PeerAggregateMembershipCommitInputs {
        PeerAggregateMembershipCommitInputs {
            before_peer: self.before_peer,
            after_peer: self.after_peer,
            unchanged_topology: self.unchanged_topology,
            read_guards: self.contract.read_guards().to_vec(),
            canonical_game_root: self.canonical_game_root,
            sealed_roots: self.sealed_roots,
        }
    }
}

#[cfg(test)]
mod tests;
