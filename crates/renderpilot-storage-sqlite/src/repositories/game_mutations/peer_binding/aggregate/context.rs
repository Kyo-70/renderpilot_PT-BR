use super::super::super::*;
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct BindingContext<'a> {
    pub(super) before_state: Option<&'a OptiScalerInstallState>,
    pub(super) after_state: Option<&'a OptiScalerInstallState>,
    pub(super) before_topology: Option<&'a GameProxyTopology>,
    pub(super) after_topology: Option<&'a GameProxyTopology>,
    pub(super) peer: PeerAggregateBinding<'a>,
    pub(super) before_receipts: BTreeMap<String, (String, FileReceipt)>,
    pub(super) after_receipts: BTreeMap<String, (String, FileReceipt)>,
    pub(super) before_directories: BTreeMap<String, (String, String)>,
    pub(super) after_directories: BTreeMap<String, (String, String)>,
    pub(super) known_paths: BTreeSet<String>,
    pub(super) paths: BTreeMap<String, pending_file_mutations::OptiScalerBoundPath>,
    pub(super) after_claims: BTreeMap<String, pending_file_mutations::OptiScalerBoundPath>,
    pub(super) retained_fsr_custody: BTreeMap<String, FileReceipt>,
    pub(super) owned_preservations: Vec<pending_file_mutations::OptiScalerOwnedPreservation>,
    pub(super) auxiliary_destinations: BTreeSet<String>,
    pub(super) auxiliary_sources: BTreeSet<String>,
    pub(super) relocation_keys: BTreeSet<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ReusedAggregateRole {
    Configuration,
    ReleaseArtifact,
    RuntimeBinding,
    TopologyOuter,
}

impl<'a> BindingContext<'a> {
    pub(super) fn new(
        before_state: Option<&'a OptiScalerInstallState>,
        after_state: Option<&'a OptiScalerInstallState>,
        before_topology: Option<&'a GameProxyTopology>,
        after_topology: Option<&'a GameProxyTopology>,
        peer: PeerAggregateBinding<'a>,
    ) -> AppResult<Self> {
        let before_receipts = collect_optiscaler_receipts(before_state, before_topology)?;
        let after_receipts = collect_optiscaler_receipts(after_state, after_topology)?;
        let before_directories = collect_optiscaler_directories(before_state);
        let after_directories = collect_optiscaler_directories(after_state);
        let mut known_paths = BTreeSet::new();
        for state in [before_state, after_state].into_iter().flatten() {
            known_paths.extend(release_path_map(Some(state)).into_keys());
            known_paths.extend(runtime_path_map(Some(state)).into_keys());
            known_paths.extend(directory_path_map(Some(state)).into_keys());
            for file in &state.release_files {
                if let Some((_, custody, _)) = file.baseline.retained_original() {
                    known_paths.insert(normalized_path_key(custody.as_str()));
                }
            }
        }
        for topology in [before_topology, after_topology].into_iter().flatten() {
            known_paths.extend(topology_path_map(Some(topology)).into_keys());
        }
        for addon in [
            match peer.mutation {
                OptiScalerPeerMutation::Replace { before, .. } => Some(before),
                OptiScalerPeerMutation::Keep => None,
            },
            match peer.mutation {
                OptiScalerPeerMutation::Replace { after, .. } => Some(after),
                OptiScalerPeerMutation::Keep => None,
            },
        ]
        .into_iter()
        .flatten()
        {
            known_paths.extend(peer_path_map(Some(addon)).into_keys());
            known_paths.extend(peer_owned_sidecar_path_map(Some(addon)).into_keys());
        }

        Ok(Self {
            before_state,
            after_state,
            before_topology,
            after_topology,
            peer,
            before_receipts,
            after_receipts,
            before_directories,
            after_directories,
            known_paths,
            paths: BTreeMap::new(),
            after_claims: BTreeMap::new(),
            retained_fsr_custody: BTreeMap::new(),
            owned_preservations: Vec::new(),
            auxiliary_destinations: BTreeSet::new(),
            auxiliary_sources: BTreeSet::new(),
            relocation_keys: BTreeSet::new(),
        })
    }

    pub(super) fn reused_role(&self, key: &str) -> Option<ReusedAggregateRole> {
        if let Some(state) = self.before_state {
            if let Some(file) = state
                .release_files
                .iter()
                .find(|file| normalized_path_key(file.path.as_str()) == key)
            {
                return Some(match file.role {
                    OptiScalerFileRole::Configuration => ReusedAggregateRole::Configuration,
                    OptiScalerFileRole::Runtime => ReusedAggregateRole::ReleaseArtifact,
                });
            }
            if state
                .runtime_bindings
                .iter()
                .any(|binding| normalized_path_key(binding.path.as_str()) == key)
            {
                return Some(ReusedAggregateRole::RuntimeBinding);
            }
        }
        self.before_topology
            .filter(|topology| {
                normalized_path_key(topology.outer.path.as_str()) == key
                    && topology.outer.implementation == ProxyImplementation::OptiScaler
            })
            .map(|_| ReusedAggregateRole::TopologyOuter)
    }
}
