//! Pure derivation and validation of peer read-guard requirements.

mod model;
#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, BTreeSet};

use crate::{
    GameProxyTopology, InstalledAddon, ManagedFileBaseline, ManagedFileMode,
    NormalizedPathRelation, PathRef, Sha256Hash, normalized_path_key, normalized_path_relation,
};

use super::claims::{ManagedClaim, PeerSnapshot, managed_sidecar_path, validate_snapshots};
use super::model::{
    CoordinatedPeerOperation, PeerEndpointIntent, PeerEndpointOperation, PeerEndpointRole,
    PeerFileImage, PeerTransitionError, PlannedGameProxyTopology, ProxyPeerRoute,
};
use super::planning::{PlannedTopologyShape, topology_shape, validate_shape};
use super::renodx_reshade_ini::RenoDxReshadeIniAuthority;
use super::{ExactOptiConfigProjection, PeerTransitionContext};

pub use model::{
    PeerReadGuardEvidence, PeerReadGuardExpectation, PeerReadGuardRequirement, PeerReadGuardSource,
};

/// Builds the private guard used by aggregate-only reused-claim release.
///
/// The requirement constructor intentionally remains unavailable to callers;
/// every public contract derives its guard set from its own typed claims.
pub(super) fn reused_live_guard(path: PathRef, sha256: Sha256Hash) -> PeerReadGuardRequirement {
    PeerReadGuardRequirement::new(
        path,
        vec![PeerReadGuardSource::ManagedReusedLive],
        PeerReadGuardExpectation::Digest { sha256 },
    )
}

#[derive(Debug)]
struct GuardCandidate {
    path: PathRef,
    sources: BTreeSet<PeerReadGuardSource>,
    expectation: PeerReadGuardExpectation,
}

/// Derives the complete read-guard set for one peer/topology transition.
///
/// The derivation is filesystem-free and repeats the domain route/snapshot
/// checks at the trust boundary. Only immutable topology receipts, reused
/// live claims, and managed baseline sidecars are admitted; generic payload
/// claims never become read authority.
pub fn required_read_guards(
    before_peer: Option<&InstalledAddon>,
    after_peer: Option<&InstalledAddon>,
    before_topology: Option<&GameProxyTopology>,
    planned_after_topology: Option<&PlannedGameProxyTopology>,
    route: ProxyPeerRoute,
    intents: &[PeerEndpointIntent],
) -> Result<Vec<PeerReadGuardRequirement>, PeerTransitionError> {
    required_read_guards_with_catalog(
        before_peer,
        after_peer,
        before_topology,
        planned_after_topology,
        route,
        intents,
        None,
    )
}

/// Derives read guards while including one exact catalog rollback projection.
pub fn required_read_guards_with_catalog(
    before_peer: Option<&InstalledAddon>,
    after_peer: Option<&InstalledAddon>,
    before_topology: Option<&GameProxyTopology>,
    planned_after_topology: Option<&PlannedGameProxyTopology>,
    route: ProxyPeerRoute,
    intents: &[PeerEndpointIntent],
    catalog: Option<&super::catalog_physical::PeerCatalogPhysicalContract>,
) -> Result<Vec<PeerReadGuardRequirement>, PeerTransitionError> {
    required_read_guards_scoped(
        PeerTransitionContext::new(
            before_peer,
            after_peer,
            before_topology,
            planned_after_topology,
            route,
        ),
        intents,
        catalog,
        None,
        None,
    )
}

/// Derives read guards for a route carrying the typed RenoDX ReShade.ini
/// endpoint. The typed endpoint itself is physical and therefore contributes
/// no independent read guard.
pub fn required_read_guards_with_renodx_reshade_ini(
    before_peer: Option<&InstalledAddon>,
    after_peer: Option<&InstalledAddon>,
    before_topology: Option<&GameProxyTopology>,
    planned_after_topology: Option<&PlannedGameProxyTopology>,
    route: ProxyPeerRoute,
    intents: &[PeerEndpointIntent],
    authority: &RenoDxReshadeIniAuthority,
) -> Result<Vec<PeerReadGuardRequirement>, PeerTransitionError> {
    required_read_guards_scoped(
        PeerTransitionContext::new(
            before_peer,
            after_peer,
            before_topology,
            planned_after_topology,
            route,
        ),
        intents,
        None,
        Some(authority),
        None,
    )
}

/// Derives read guards for the narrow RenoDX route that also changes the one
/// typed OptiScaler configuration endpoint. The endpoint itself remains
/// governed by its exact O1 preimage, so this only extends contract
/// derivation; it does not add a second read authority.
pub fn required_read_guards_with_renodx_reshade_ini_and_optiscaler_config(
    context: PeerTransitionContext<'_>,
    intents: &[PeerEndpointIntent],
    renodx_reshade_ini: Option<&RenoDxReshadeIniAuthority>,
    optiscaler_config: &ExactOptiConfigProjection,
) -> Result<Vec<PeerReadGuardRequirement>, PeerTransitionError> {
    required_read_guards_scoped(
        context,
        intents,
        None,
        renodx_reshade_ini,
        Some(optiscaler_config),
    )
}

/// Derives read guards for a typed RenoDX ReShade.ini transition carrying the
/// exact DLSS-Fix companion projection. When the companion is not a physical
/// endpoint in the program, its sealed preimage becomes one dedicated guard;
/// a physical companion endpoint is guarded by its endpoint preimage instead.
pub fn required_read_guards_with_renodx_reshade_ini_and_dlss(
    context: PeerTransitionContext<'_>,
    intents: &[PeerEndpointIntent],
    authority: Option<&RenoDxReshadeIniAuthority>,
    projection: &super::RenoDxDlssProjection,
) -> Result<Vec<PeerReadGuardRequirement>, PeerTransitionError> {
    projection.validate_against_peers(context.before_peer, context.after_peer)?;
    let mut requirements = if intents.is_empty() {
        if authority.is_some() || !matches!(context.route, ProxyPeerRoute::DurableDisjoint) {
            return Err(PeerTransitionError::EmptyIntentSet);
        }
        let planned_shape = super::planning::topology_shape(
            context.before_topology,
            context.planned_after_topology,
            context.route,
        )?;
        super::planning::validate_shape(context.before_topology, &planned_shape)?;
        super::claims::validate_snapshots(
            context.before_peer,
            context.after_peer,
            context.before_topology,
            planned_shape.exact(),
        )?;
        Vec::new()
    } else {
        required_read_guards_scoped(context, intents, None, authority, None)?
    };
    if !intents.iter().any(|intent| {
        normalized_path_key(intent.path().as_str())
            == normalized_path_key(projection.companion_path().as_str())
    }) {
        let expectation = match projection.before_image() {
            super::RenoDxDlssBeforeImage::Absent => PeerReadGuardExpectation::Absent,
            super::RenoDxDlssBeforeImage::Present {
                identity, sha256, ..
            } => PeerReadGuardExpectation::Receipt {
                identity: identity.clone(),
                sha256: sha256.clone(),
            },
        };
        requirements.push(PeerReadGuardRequirement::new(
            projection.companion_path().clone(),
            vec![PeerReadGuardSource::RenoDxDlssCompanion],
            expectation,
        ));
    }
    Ok(requirements)
}

fn required_read_guards_scoped(
    context: PeerTransitionContext<'_>,
    intents: &[PeerEndpointIntent],
    catalog: Option<&super::catalog_physical::PeerCatalogPhysicalContract>,
    authority: Option<&RenoDxReshadeIniAuthority>,
    optiscaler_config: Option<&ExactOptiConfigProjection>,
) -> Result<Vec<PeerReadGuardRequirement>, PeerTransitionError> {
    let PeerTransitionContext {
        before_peer,
        after_peer,
        before_topology,
        planned_after_topology,
        route,
    } = context;
    // The downstream authority comes from the sealed topology below. It is
    // not an independently supplied capability at this early guard boundary.
    super::validation::validate_intents_with_scoped_authority(
        route,
        intents,
        authority,
        optiscaler_config,
        None,
    )?;

    let planned_shape = topology_shape(before_topology, planned_after_topology, route)?;
    validate_shape(before_topology, &planned_shape)?;
    validate_snapshots(
        before_peer,
        after_peer,
        before_topology,
        planned_shape.exact(),
    )?;
    validate_planned_peer_games(before_peer, after_peer, &planned_shape)?;
    validate_topology_endpoint(route, before_topology, &planned_shape, intents)?;

    let before = PeerSnapshot::from_peer(before_peer)?;
    let after = PeerSnapshot::from_peer(after_peer)?;
    let reused_acquisition_keys = reused_acquisition_keys(&before, &after);
    let mut candidates = BTreeMap::new();

    if let Some(topology) = before_topology {
        add_guard(
            &mut candidates,
            topology.root_slot.clone(),
            PeerReadGuardSource::TopologyOuter,
            PeerReadGuardExpectation::Receipt {
                identity: topology.outer.receipt.identity().to_owned(),
                sha256: topology.outer.receipt.digest().clone(),
            },
        )?;

        if let Some(downstream) = topology.downstream.as_ref()
            && !has_topology_endpoint(intents, &downstream.path)
        {
            add_guard(
                &mut candidates,
                downstream.path.clone(),
                PeerReadGuardSource::TopologyDownstream,
                PeerReadGuardExpectation::Receipt {
                    identity: downstream.receipt.identity().to_owned(),
                    sha256: downstream.receipt.digest().clone(),
                },
            )?;
        }
    }

    for snapshot in [&before, &after] {
        for claim in snapshot.managed.values() {
            match claim.mode {
                ManagedFileMode::Reused => {
                    if !reused_acquisition_keys.contains(&normalized_path_key(claim.path.as_str()))
                    {
                        add_guard(
                            &mut candidates,
                            claim.path.clone(),
                            PeerReadGuardSource::ManagedReusedLive,
                            PeerReadGuardExpectation::Digest {
                                sha256: claim.installed.clone(),
                            },
                        )?;
                    }
                }
                ManagedFileMode::Owned => {
                    let sidecar = managed_sidecar_path(&claim.path)?;
                    if !has_exact_endpoint(intents, &sidecar) {
                        add_guard(
                            &mut candidates,
                            sidecar,
                            PeerReadGuardSource::ManagedOwnedBaseline,
                            baseline_expectation(&claim.baseline),
                        )?;
                    }
                }
            }
        }
    }

    if let Some(catalog) = catalog {
        for guard in catalog.guards() {
            add_guard(
                &mut candidates,
                guard.path.clone(),
                guard.source,
                guard.expectation.clone(),
            )?;
        }
    }

    let mut requirements = Vec::with_capacity(candidates.len());
    for candidate in candidates.into_values() {
        let sources = candidate.sources.into_iter().collect::<Vec<_>>();
        requirements.push(PeerReadGuardRequirement::new(
            candidate.path,
            sources,
            candidate.expectation,
        ));
    }
    for requirement in &requirements {
        for intent in intents {
            if normalized_path_relation(requirement.path().as_str(), intent.path().as_str())
                .overlaps()
            {
                return Err(PeerTransitionError::ReadGuardEndpointOverlap(
                    requirement.path().clone(),
                    intent.path().clone(),
                ));
            }
        }
    }
    // Re-derive the aggregate-to-endpoint contract at this trust boundary so
    // an arbitrary endpoint cannot be mistaken for a changed baseline
    // sidecar or topology participant merely because it has the right path.
    if let Some(projection) = optiscaler_config {
        super::PeerTransitionContract::derive_physical_with_renodx_reshade_ini_and_optiscaler_config(
            PeerTransitionContext::new(
                before_peer,
                after_peer,
                before_topology,
                planned_after_topology,
                route,
            ),
            authority.cloned(),
            projection.clone(),
            intents.to_vec(),
        )?;
    } else if let Some(authority) = authority {
        super::PeerTransitionContract::derive_physical_with_renodx_reshade_ini(
            before_peer,
            after_peer,
            before_topology,
            planned_after_topology,
            route,
            authority.clone(),
            intents.to_vec(),
        )?;
    } else {
        super::PeerTransitionContract::derive_physical_with_catalog(
            before_peer,
            after_peer,
            before_topology,
            planned_after_topology,
            route,
            intents.to_vec(),
            catalog,
        )?;
    }
    Ok(requirements)
}

/// Validates ordered read observations against a domain-derived requirement
/// set. Filesystem adapters may construct evidence, but cannot define what a
/// guard means or reorder the authority-bearing requirements.
pub fn validate_read_guards(
    requirements: &[PeerReadGuardRequirement],
    evidence: &[PeerReadGuardEvidence],
) -> Result<(), PeerTransitionError> {
    if requirements.len() != evidence.len() {
        return Err(PeerTransitionError::ReadGuardEvidenceCardinality {
            expected: requirements.len(),
            actual: evidence.len(),
        });
    }

    let mut previous_key = None;
    for (requirement, observed) in requirements.iter().zip(evidence) {
        let key = normalized_path_key(requirement.path().as_str());
        if requirement.sources().is_empty()
            || requirement
                .sources()
                .windows(2)
                .any(|pair| pair[0] >= pair[1])
            || previous_key
                .as_ref()
                .is_some_and(|previous| previous >= &key)
        {
            return Err(PeerTransitionError::InvalidReadGuardRequirement(
                requirement.path().clone(),
            ));
        }
        previous_key = Some(key);
        if normalized_path_key(observed.path().as_str())
            != normalized_path_key(requirement.path().as_str())
        {
            return Err(PeerTransitionError::ReadGuardEvidenceOrderMismatch(
                requirement.path().clone(),
            ));
        }
        if !matches_expectation(requirement.expectation(), observed.observed()) {
            return Err(PeerTransitionError::ReadGuardMismatch(
                requirement.path().clone(),
            ));
        }
    }
    Ok(())
}

fn add_guard(
    candidates: &mut BTreeMap<String, GuardCandidate>,
    path: PathRef,
    source: PeerReadGuardSource,
    expectation: PeerReadGuardExpectation,
) -> Result<(), PeerTransitionError> {
    let key = normalized_path_key(path.as_str());
    match candidates.entry(key) {
        std::collections::btree_map::Entry::Vacant(entry) => {
            let mut sources = BTreeSet::new();
            sources.insert(source);
            entry.insert(GuardCandidate {
                path,
                sources,
                expectation,
            });
        }
        std::collections::btree_map::Entry::Occupied(mut entry) => {
            let candidate = entry.get_mut();
            merge_expectation(&mut candidate.expectation, &expectation, &candidate.path)?;
            candidate.sources.insert(source);
            if path.as_str() < candidate.path.as_str() {
                candidate.path = path;
            }
        }
    }
    Ok(())
}

fn merge_expectation(
    current: &mut PeerReadGuardExpectation,
    incoming: &PeerReadGuardExpectation,
    path: &PathRef,
) -> Result<(), PeerTransitionError> {
    let merged = match (&*current, incoming) {
        (PeerReadGuardExpectation::Absent, PeerReadGuardExpectation::Absent) => {
            PeerReadGuardExpectation::Absent
        }
        (
            PeerReadGuardExpectation::Digest { sha256: left },
            PeerReadGuardExpectation::Digest { sha256: right },
        ) if left == right => PeerReadGuardExpectation::Digest {
            sha256: left.clone(),
        },
        (
            PeerReadGuardExpectation::Receipt {
                identity: left_identity,
                sha256: left_sha,
            },
            PeerReadGuardExpectation::Receipt {
                identity: right_identity,
                sha256: right_sha,
            },
        ) if left_identity == right_identity && left_sha == right_sha => {
            PeerReadGuardExpectation::Receipt {
                identity: left_identity.clone(),
                sha256: left_sha.clone(),
            }
        }
        (
            PeerReadGuardExpectation::Digest { sha256: digest },
            PeerReadGuardExpectation::Receipt { identity, sha256 },
        )
        | (
            PeerReadGuardExpectation::Receipt { identity, sha256 },
            PeerReadGuardExpectation::Digest { sha256: digest },
        ) if digest == sha256 => PeerReadGuardExpectation::Receipt {
            identity: identity.clone(),
            sha256: sha256.clone(),
        },
        _ => {
            return Err(PeerTransitionError::ReadGuardExpectationConflict(
                path.clone(),
            ));
        }
    };
    *current = merged;
    Ok(())
}

fn baseline_expectation(baseline: &ManagedFileBaseline) -> PeerReadGuardExpectation {
    match baseline {
        ManagedFileBaseline::Absent => PeerReadGuardExpectation::Absent,
        ManagedFileBaseline::Present { sha256 } => PeerReadGuardExpectation::Digest {
            sha256: sha256.clone(),
        },
    }
}

fn reused_acquisition_keys(before: &PeerSnapshot, after: &PeerSnapshot) -> BTreeSet<String> {
    before
        .managed
        .iter()
        .filter_map(|(key, before_claim)| {
            let after_claim = after.managed.get(key)?;
            is_reused_acquisition(before_claim, after_claim).then(|| key.clone())
        })
        .collect()
}

fn is_reused_acquisition(before: &ManagedClaim, after: &ManagedClaim) -> bool {
    before.mode == ManagedFileMode::Reused
        && after.mode == ManagedFileMode::Owned
        && before.path == after.path
        && matches!(
            &after.baseline,
            ManagedFileBaseline::Present { sha256 } if sha256 == &before.installed
        )
}

fn has_exact_endpoint(intents: &[PeerEndpointIntent], path: &PathRef) -> bool {
    intents.iter().any(|intent| {
        matches!(
            normalized_path_relation(intent.path().as_str(), path.as_str()),
            NormalizedPathRelation::Equal
        )
    })
}

fn has_topology_endpoint(intents: &[PeerEndpointIntent], path: &PathRef) -> bool {
    intents.iter().any(|intent| {
        intent.role() == PeerEndpointRole::TopologyDownstream
            && matches!(
                normalized_path_relation(intent.path().as_str(), path.as_str()),
                NormalizedPathRelation::Equal
            )
    })
}

fn validate_planned_peer_games(
    before_peer: Option<&InstalledAddon>,
    after_peer: Option<&InstalledAddon>,
    shape: &PlannedTopologyShape,
) -> Result<(), PeerTransitionError> {
    let topology_game = match shape {
        PlannedTopologyShape::Absent => None,
        PlannedTopologyShape::Exact(topology) => Some(&topology.game_id),
        PlannedTopologyShape::Observed(topology) => Some(&topology.game_id),
    };
    for peer in [before_peer, after_peer].into_iter().flatten() {
        if topology_game.is_some_and(|game| game != peer.game_id()) {
            return Err(PeerTransitionError::InvalidPeerSnapshot(
                "peer and planned topology belong to different games",
            ));
        }
    }
    Ok(())
}

fn validate_topology_endpoint(
    route: ProxyPeerRoute,
    before: Option<&GameProxyTopology>,
    planned: &PlannedTopologyShape,
    intents: &[PeerEndpointIntent],
) -> Result<(), PeerTransitionError> {
    let ProxyPeerRoute::Coordinated(operation) = route else {
        return Ok(());
    };
    let before_downstream = before.and_then(|topology| topology.downstream.as_ref());
    let after_path = planned
        .downstream()
        .map(super::planning::PlannedDownstream::path);
    let endpoint = intents
        .iter()
        .find(|intent| intent.role() == PeerEndpointRole::TopologyDownstream)
        .expect("validate_intents enforces one coordinated downstream");
    match operation {
        CoordinatedPeerOperation::Create => {
            if before_downstream.is_some()
                || after_path.is_none_or(|path| {
                    !matches!(
                        normalized_path_relation(path.as_str(), endpoint.path().as_str()),
                        NormalizedPathRelation::Equal
                    )
                })
                || endpoint.operation() != PeerEndpointOperation::Create
            {
                return Err(PeerTransitionError::OperationMismatch(
                    endpoint.path().clone(),
                ));
            }
        }
        CoordinatedPeerOperation::ReplaceSamePath => {
            if before_downstream.is_none()
                || after_path.is_none_or(|path| {
                    !matches!(
                        normalized_path_relation(path.as_str(), endpoint.path().as_str()),
                        NormalizedPathRelation::Equal
                    )
                })
                || before_downstream.is_some_and(|link| {
                    !matches!(
                        normalized_path_relation(link.path.as_str(), endpoint.path().as_str()),
                        NormalizedPathRelation::Equal
                    )
                })
                || endpoint.operation() != PeerEndpointOperation::Replace
            {
                return Err(PeerTransitionError::OperationMismatch(
                    endpoint.path().clone(),
                ));
            }
        }
        CoordinatedPeerOperation::Remove => {
            if before_downstream.is_none()
                || after_path.is_some()
                || before_downstream.is_some_and(|link| {
                    !matches!(
                        normalized_path_relation(link.path.as_str(), endpoint.path().as_str()),
                        NormalizedPathRelation::Equal
                    )
                })
                || !matches!(
                    endpoint.operation(),
                    PeerEndpointOperation::Remove | PeerEndpointOperation::Replace
                )
            {
                return Err(PeerTransitionError::OperationMismatch(
                    endpoint.path().clone(),
                ));
            }
        }
    }
    Ok(())
}

fn matches_expectation(
    expectation: &PeerReadGuardExpectation,
    observed: Option<&PeerFileImage>,
) -> bool {
    match (expectation, observed) {
        (PeerReadGuardExpectation::Absent, None) => true,
        (PeerReadGuardExpectation::Digest { sha256 }, Some(observed)) => {
            observed.sha256() == sha256
        }
        (PeerReadGuardExpectation::Receipt { identity, sha256 }, Some(observed)) => {
            observed.identity() == identity && observed.sha256() == sha256
        }
        _ => false,
    }
}
