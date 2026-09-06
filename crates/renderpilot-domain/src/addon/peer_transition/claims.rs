//! Canonical peer snapshot claim collection.

use std::collections::BTreeMap;

use crate::{
    AddonKind, GameProxyTopology, InstalledAddon, ManagedAddonFile, ManagedFileBaseline,
    ManagedFileMode, PathRef, normalized_path_key, normalized_path_relation,
};

use super::model::{
    PeerEndpointIntent, PeerEndpointOperation, PeerEndpointRole, PeerTransitionError,
};

/// Returns the canonical managed sidecar path for a live managed file.
///
/// Sidecars are part of the managed-file contract rather than an incidental
/// string convention at each call site. Keeping this derivation here makes
/// every peer snapshot and transition use the same path construction and
/// error boundary.
pub fn managed_sidecar_path(path: &PathRef) -> Result<PathRef, PeerTransitionError> {
    let mut sidecar = path.as_str().to_owned();
    sidecar.push_str(".bak");
    PathRef::new(sidecar)
        .map_err(|_| PeerTransitionError::InvalidPeerSnapshot("invalid managed sidecar path"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ManagedClaim {
    pub(super) path: PathRef,
    pub(super) mode: ManagedFileMode,
    pub(super) baseline: ManagedFileBaseline,
    pub(super) installed: crate::Sha256Hash,
}

#[derive(Debug, Default)]
pub(super) struct PeerSnapshot {
    pub(super) created: BTreeMap<String, PathRef>,
    pub(super) backed: BTreeMap<String, PathRef>,
    pub(super) managed: BTreeMap<String, ManagedClaim>,
}

/// A physical mutation of a generic claim whose persisted membership is
/// intentionally unchanged.
///
/// This is an internal derivation fact.  It is never serialized or accepted
/// from a caller: the only constructor verifies the normalized before/after
/// claim and the unchanged backed membership before selecting the operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RetainedGenericMutation {
    RewritePresent,
    RepairMissing,
}

impl RetainedGenericMutation {
    pub(super) fn from_stable_claim(
        before: &PeerSnapshot,
        after: &PeerSnapshot,
        intent: &PeerEndpointIntent,
    ) -> Option<Self> {
        let key = normalized_path_key(intent.path().as_str());
        let before_path = before.created.get(&key)?;
        let after_path = after.created.get(&key)?;
        if normalized_path_key(before_path.as_str()) != normalized_path_key(after_path.as_str())
            || before.backed.contains_key(&key) != after.backed.contains_key(&key)
        {
            return None;
        }

        match (intent.role(), intent.operation()) {
            (PeerEndpointRole::Disjoint, PeerEndpointOperation::Replace) => {
                Some(Self::RewritePresent)
            }
            (PeerEndpointRole::Disjoint, PeerEndpointOperation::Create)
                if intent.planned_sha256().is_some() && intent.planned_length().is_some() =>
            {
                Some(Self::RepairMissing)
            }
            _ => None,
        }
    }
}

impl PeerSnapshot {
    pub(super) fn from_peer(peer: Option<&InstalledAddon>) -> Result<Self, PeerTransitionError> {
        let Some(peer) = peer else {
            return Ok(Self::default());
        };
        let mut snapshot = Self::default();
        let addon_key = normalized_path_key(peer.addon_file().as_str());
        for path in peer.created_files() {
            let key = normalized_path_key(path.as_str());
            match snapshot.created.entry(key) {
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(path.clone());
                }
                std::collections::btree_map::Entry::Occupied(entry) => {
                    // InstalledAddon deliberately permits the canonical addon file
                    // to appear in created_files.  It is one claim, not two
                    // physical endpoints; normalize that duplicate here.
                    if entry.key() == &addon_key {
                        continue;
                    }
                    return Err(PeerTransitionError::DuplicateEndpoint(path.clone()));
                }
            }
        }
        snapshot
            .created
            .entry(addon_key)
            .or_insert_with(|| peer.addon_file().clone());
        for path in peer.backed_up_files() {
            let key = normalized_path_key(path.as_str());
            if snapshot.backed.insert(key, path.clone()).is_some() {
                return Err(PeerTransitionError::DuplicateEndpoint(path.clone()));
            }
        }
        for file in peer.managed_files() {
            let key = normalized_path_key(file.path().as_str());
            if snapshot
                .managed
                .insert(key, ManagedClaim::from_file(file))
                .is_some()
            {
                return Err(PeerTransitionError::DuplicateEndpoint(file.path().clone()));
            }
        }
        for path in snapshot.created.values() {
            if snapshot
                .managed
                .contains_key(&normalized_path_key(path.as_str()))
            {
                return Err(PeerTransitionError::InvalidPeerSnapshot(
                    "generic and managed claims overlap",
                ));
            }
        }
        for path in snapshot.backed.values() {
            let sidecar = managed_sidecar_path(path)?;
            let sidecar_key = normalized_path_key(sidecar.as_str());
            if snapshot.created.contains_key(&sidecar_key)
                || snapshot.backed.contains_key(&sidecar_key)
                || snapshot.managed.contains_key(&sidecar_key)
            {
                return Err(PeerTransitionError::DuplicateEndpoint(sidecar));
            }
            if snapshot
                .managed
                .contains_key(&normalized_path_key(path.as_str()))
            {
                return Err(PeerTransitionError::InvalidPeerSnapshot(
                    "generic and managed claims overlap",
                ));
            }
        }
        for claim in snapshot.managed.values() {
            if let ManagedFileBaseline::Present { .. } = claim.baseline {
                let sidecar = managed_sidecar_path(&claim.path)?;
                let sidecar_key = normalized_path_key(sidecar.as_str());
                if snapshot.created.contains_key(&sidecar_key)
                    || snapshot.backed.contains_key(&sidecar_key)
                    || snapshot.managed.contains_key(&sidecar_key)
                {
                    return Err(PeerTransitionError::DuplicateEndpoint(sidecar));
                }
            }
        }
        validate_peer_overlaps(&snapshot)?;
        Ok(snapshot)
    }
}

impl ManagedClaim {
    pub(super) fn from_file(file: &ManagedAddonFile) -> Self {
        Self {
            path: file.path().clone(),
            mode: file.mode(),
            baseline: file.baseline().clone(),
            installed: file.installed_sha256().clone(),
        }
    }
}

pub(super) fn validate_snapshots(
    before_peer: Option<&InstalledAddon>,
    after_peer: Option<&InstalledAddon>,
    before_topology: Option<&GameProxyTopology>,
    after_topology: Option<&GameProxyTopology>,
) -> Result<(), PeerTransitionError> {
    for peer in [before_peer, after_peer].into_iter().flatten() {
        if !matches!(peer.kind(), AddonKind::Luma | AddonKind::RenoDx) {
            return Err(PeerTransitionError::InvalidPeerSnapshot(
                "peer kind is not a supported proxy peer",
            ));
        }
        for topology in [before_topology, after_topology].into_iter().flatten() {
            if peer.game_id() != &topology.game_id {
                return Err(PeerTransitionError::InvalidPeerSnapshot(
                    "peer and topology belong to different games",
                ));
            }
        }
    }
    if let (Some(before), Some(after)) = (before_peer, after_peer)
        && before.kind() != after.kind()
    {
        return Err(PeerTransitionError::InvalidPeerSnapshot(
            "peer kind cannot change in one transition",
        ));
    }
    if let (Some(before), Some(after)) = (before_peer, after_peer)
        && before.game_id() != after.game_id()
    {
        return Err(PeerTransitionError::InvalidPeerSnapshot(
            "peer records belong to different games",
        ));
    }
    for topology in [before_topology, after_topology].into_iter().flatten() {
        topology
            .validate()
            .map_err(|error| PeerTransitionError::InvalidTopologySnapshot(error.to_string()))?;
    }
    if let (Some(before), Some(after)) = (before_topology, after_topology) {
        if before.game_id != after.game_id {
            return Err(PeerTransitionError::InvalidTopologySnapshot(
                "topology images belong to different games".to_owned(),
            ));
        }
        if before.id != after.id
            || before.root_slot != after.root_slot
            || before.outer != after.outer
        {
            return Err(PeerTransitionError::ImmutableTopology);
        }
    }
    Ok(())
}

fn validate_peer_overlaps(snapshot: &PeerSnapshot) -> Result<(), PeerTransitionError> {
    let mut paths = snapshot
        .created
        .values()
        .chain(snapshot.backed.values())
        .chain(snapshot.managed.values().map(|claim| &claim.path))
        .cloned()
        .collect::<Vec<_>>();
    let backed_sidecars = snapshot
        .backed
        .values()
        .map(managed_sidecar_path)
        .collect::<Result<Vec<_>, _>>()?;
    paths.extend(backed_sidecars);
    let managed_sidecars = snapshot
        .managed
        .values()
        .filter(|claim| matches!(claim.baseline, ManagedFileBaseline::Present { .. }))
        .map(|claim| managed_sidecar_path(&claim.path))
        .collect::<Result<Vec<_>, _>>()?;
    paths.extend(managed_sidecars);
    paths.sort_by_key(|path| normalized_path_key(path.as_str()));
    for (index, left) in paths.iter().enumerate() {
        for right in paths.iter().skip(index + 1) {
            if normalized_path_key(left.as_str()) == normalized_path_key(right.as_str()) {
                continue;
            }
            if normalized_path_relation(left.as_str(), right.as_str()).overlaps() {
                return Err(PeerTransitionError::OverlappingEndpoints(
                    (*left).clone(),
                    (*right).clone(),
                ));
            }
        }
    }
    Ok(())
}
