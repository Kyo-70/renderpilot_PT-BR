//! Pure active RenoDX host classification and lowering.

mod error;
mod lowering;
mod model;
mod validation;

use crate::addons::renodx::install::PreparedInstall;
use crate::addons::renodx::peer::InstallActiveSnapshot;
use crate::addons::reshade::host_policy::HostLifecycle;
use crate::addons::reshade::scan::ReshadeIdentity;
pub(crate) use error::RenoDxHostError;
pub(crate) use lowering::lower_owned_host;
pub(crate) use model::{RenoDxHostClassification, RenoDxOwnedHostPlan};
use renderpilot_domain::{
    FileOwnership, GameProxyTopology, ManagedAddonFile, ManagedFileBaseline,
    NormalizedPathRelation, PlannedGameProxyTopology, ProxyImplementation, Sha256Hash,
    normalized_path_relation,
};

/// Classifies the exact direct `ReShade64.dll` slot from the already sealed
/// phase-one/phase-three evidence. No path is read or rediscovered here.
pub(crate) fn classify_host(
    snapshot: &InstallActiveSnapshot,
    prepared: &PreparedInstall,
) -> Result<RenoDxHostClassification, RenoDxHostError> {
    let topology = snapshot.topology().ok_or(RenoDxHostError::Assessment(
        "proxy RenoDX host classification requires an active proxy topology",
    ))?;
    topology
        .validate()
        .map_err(|_| RenoDxHostError::Assessment("active proxy topology is invalid"))?;
    validation::require_optiscaler_outer(topology.outer.implementation)?;

    let host_path = snapshot.host_path().ok_or(RenoDxHostError::Assessment(
        "proxy RenoDX snapshot is missing its exact host path",
    ))?;
    validation::require_exact_host_path(snapshot.root_seal().canonical_game_root_ref(), host_path)?;
    let host_snapshot = snapshot.host_preimage().ok_or(RenoDxHostError::Assessment(
        "proxy RenoDX snapshot is missing its exact host preimage",
    ))?;
    let assessment = snapshot
        .host_assessment()
        .ok_or(RenoDxHostError::Assessment(
            "proxy RenoDX snapshot is missing its exact host assessment",
        ))?;
    if assessment.snapshot().requires_host_download != snapshot.writes_host()
        || assessment.assessment().initial_writes_host() != snapshot.writes_host()
    {
        return Err(RenoDxHostError::Assessment(
            "sealed host assessment disagrees with the snapshot download decision",
        ));
    }
    if assessment.snapshot().exact_path
        != renderpilot_domain::normalized_path_key(host_path.as_str())
        || assessment.snapshot().present != host_snapshot.file().is_some()
    {
        return Err(RenoDxHostError::Assessment(
            "sealed host assessment path or presence disagrees with the host snapshot",
        ));
    }

    let downstream = topology.downstream.as_ref();
    match (downstream, host_snapshot.file()) {
        (None, None) => {
            if assessment.snapshot().lifecycle != HostLifecycle::InstallNew
                || !snapshot.writes_host()
            {
                return Err(RenoDxHostError::Assessment(
                    "absent active host requires an InstallNew assessment",
                ));
            }
            let prepared_bytes = require_prepared_host(prepared)?;
            let digest = digest(prepared_bytes)?;
            let planned = observed_topology(
                topology,
                host_path,
                digest.clone(),
                prepared_bytes.len() as u64,
            );
            Ok(RenoDxHostClassification::Owned(RenoDxOwnedHostPlan {
                target: host_path.clone(),
                binding: ManagedAddonFile::owned(
                    host_path.clone(),
                    ManagedFileBaseline::Absent,
                    digest,
                ),
                prepared_bytes: prepared_bytes.to_vec(),
                transition: model::RenoDxHostTransition::Create,
                planned_topology: planned,
            }))
        }
        (Some(downstream), Some(file)) => {
            let assessment_snapshot = assessment.snapshot();
            if assessment_snapshot.digest.as_ref() != Some(file.digest())
                || assessment_snapshot.length != Some(file.length())
            {
                return Err(RenoDxHostError::Evidence(
                    host_path.clone(),
                    "host assessment differs from the retained host image",
                ));
            }
            if !matches!(
                assessment_snapshot.identity,
                Some(ReshadeIdentity::Probable | ReshadeIdentity::Confirmed)
            ) {
                return Err(RenoDxHostError::Assessment(
                    "existing active host has weak or missing ReShade identity",
                ));
            }
            if downstream.implementation != ProxyImplementation::ReShade {
                return Err(RenoDxHostError::Assessment(
                    "active downstream implementation is not ReShade",
                ));
            }
            if downstream.receipt.ownership() != FileOwnership::Reused {
                return Err(RenoDxHostError::Assessment(
                    "active downstream host is already owned by another addon",
                ));
            }
            if !matches!(
                normalized_path_relation(downstream.path.as_str(), host_path.as_str()),
                NormalizedPathRelation::Equal
            ) {
                return Err(RenoDxHostError::Path(
                    downstream.path.clone(),
                    host_path.clone(),
                ));
            }
            if downstream.receipt.digest() != file.digest()
                || downstream.receipt.identity() != file.identity()
            {
                return Err(RenoDxHostError::Evidence(
                    host_path.clone(),
                    "topology receipt differs from the retained host image",
                ));
            }
            if assessment.initial_is_conflict() {
                return Err(RenoDxHostError::Assessment(
                    "existing active host is incompatible or lacks ReShade identity",
                ));
            }
            match assessment.snapshot().lifecycle {
                HostLifecycle::ReuseUser | HostLifecycle::AdoptEmpty => {
                    if snapshot.writes_host() {
                        return Err(RenoDxHostError::Assessment(
                            "compatible active host unexpectedly requests replacement",
                        ));
                    }
                    Ok(RenoDxHostClassification::Reused {
                        binding: ManagedAddonFile::reused(host_path.clone(), file.digest().clone()),
                        planned_topology: PlannedGameProxyTopology::Exact(topology.clone()),
                    })
                }
                HostLifecycle::RepairEmpty => {
                    let prepared_bytes = require_prepared_host(prepared)?;
                    let digest = digest(prepared_bytes)?;
                    let planned = observed_topology(
                        topology,
                        host_path,
                        digest.clone(),
                        prepared_bytes.len() as u64,
                    );
                    Ok(RenoDxHostClassification::Owned(RenoDxOwnedHostPlan {
                        target: host_path.clone(),
                        binding: ManagedAddonFile::owned(
                            host_path.clone(),
                            ManagedFileBaseline::Present {
                                sha256: file.digest().clone(),
                            },
                            digest,
                        ),
                        prepared_bytes: prepared_bytes.to_vec(),
                        transition: model::RenoDxHostTransition::Replace {
                            live_digest: file.digest().clone(),
                        },
                        planned_topology: planned,
                    }))
                }
                HostLifecycle::InstallNew | HostLifecycle::Conflict => {
                    Err(RenoDxHostError::Assessment(
                        "existing active host is incompatible with an initial RenoDX install",
                    ))
                }
            }
        }
        (None, Some(_)) | (Some(_), None) => Err(RenoDxHostError::Evidence(
            host_path.clone(),
            "topology downstream presence differs from retained host preimage",
        )),
    }
}

fn require_prepared_host(prepared: &PreparedInstall) -> Result<&[u8], RenoDxHostError> {
    if prepared.reshade_dll_bytes.is_empty() {
        return Err(RenoDxHostError::Prepared(
            "owned host transition has no prepared ReShade bytes",
        ));
    }
    Ok(&prepared.reshade_dll_bytes)
}

fn digest(bytes: &[u8]) -> Result<Sha256Hash, RenoDxHostError> {
    renderpilot_detection::sha256_bytes(bytes)
        .map_err(|_| RenoDxHostError::Prepared("prepared host digest could not be computed"))
}

fn observed_topology(
    topology: &GameProxyTopology,
    host_path: &renderpilot_domain::PathRef,
    planned_sha256: Sha256Hash,
    planned_length: u64,
) -> PlannedGameProxyTopology {
    PlannedGameProxyTopology::ObservedOwnedDownstream {
        id: topology.id.clone(),
        game_id: topology.game_id.clone(),
        root_slot: topology.root_slot.clone(),
        outer: topology.outer.clone(),
        implementation: ProxyImplementation::ReShade,
        downstream_path: host_path.clone(),
        downstream_origin: topology
            .downstream_origin
            .clone()
            .unwrap_or_else(|| topology.root_slot.clone()),
        root_prestate: topology.root_prestate,
        planned_sha256,
        planned_length,
    }
}
