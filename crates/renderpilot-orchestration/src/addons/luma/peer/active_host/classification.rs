use renderpilot_domain::{
    FileOwnership, GameProxyTopology, ManagedAddonFile, ManagedFileBaseline,
    PlannedGameProxyTopology, ProxyImplementation,
};

use crate::{
    addons::{
        luma::peer::root_authority::LumaPeerRootAuthority,
        reshade::{
            host_policy::{HostLifecycle, TopologyHostAssessment},
            scan::ReshadeIdentity,
        },
    },
    peer_mutation_executor::PeerPathSnapshot,
};

use super::{
    model::{
        ActiveHostClassification, ActiveHostClassificationError, ActiveHostOwnedPlan,
        ActiveHostTransition,
    },
    validation::{
        correlate_live_evidence, digest, exact_host_path, require_exact_path,
        validate_assessment_path, validate_prepared_host,
    },
};

/// Classifies the exact ReShade downstream of an active OptiScaler topology.
/// Assessment and the live snapshot are supplied by the caller and are never
/// re-read here; prepared host bytes are inspected in memory.
pub(crate) fn classify_active_host(
    authority: &LumaPeerRootAuthority,
    topology: &GameProxyTopology,
    assessment: &TopologyHostAssessment,
    live_path: &renderpilot_domain::PathRef,
    live_snapshot: &PeerPathSnapshot,
    prepared_bytes: Option<Vec<u8>>,
    minimum_version: &renderpilot_domain::Version,
) -> Result<ActiveHostClassification, ActiveHostClassificationError> {
    validate_active_host_evidence(authority, topology, assessment, live_path, live_snapshot)?;

    classify_active_host_from_validated_evidence(
        topology,
        assessment,
        live_path,
        live_snapshot,
        prepared_bytes,
        minimum_version,
    )
}

/// Classifies host evidence that has already passed
/// [`validate_active_host_evidence`]. This entry point exists for composers
/// that validate shared authority before selecting an ownership-specific path.
pub(crate) fn classify_active_host_from_validated_evidence(
    topology: &GameProxyTopology,
    assessment: &TopologyHostAssessment,
    live_path: &renderpilot_domain::PathRef,
    live_snapshot: &PeerPathSnapshot,
    prepared_bytes: Option<Vec<u8>>,
    minimum_version: &renderpilot_domain::Version,
) -> Result<ActiveHostClassification, ActiveHostClassificationError> {
    match topology.downstream.as_ref() {
        None => classify_fresh(
            topology,
            assessment,
            live_path,
            live_snapshot,
            prepared_bytes,
            minimum_version,
        ),
        Some(downstream) => classify_existing(
            topology,
            downstream,
            assessment,
            live_path,
            live_snapshot,
            prepared_bytes,
            minimum_version,
        ),
    }
}

/// Validates the immutable topology, sealed host path, policy assessment, and
/// retained live image shared by initial install and persisted update paths.
/// This performs no classification or filesystem access; callers can use it
/// before selecting an ownership-specific transition.
pub(crate) fn validate_active_host_evidence(
    authority: &LumaPeerRootAuthority,
    topology: &GameProxyTopology,
    assessment: &TopologyHostAssessment,
    live_path: &renderpilot_domain::PathRef,
    live_snapshot: &PeerPathSnapshot,
) -> Result<(), ActiveHostClassificationError> {
    validate_topology(topology)?;
    let expected_path = exact_host_path(authority)?;
    require_exact_path(&expected_path, live_path)?;
    validate_assessment_path(assessment, &expected_path)?;
    correlate_live_evidence(assessment, live_path, live_snapshot)
}

fn validate_topology(topology: &GameProxyTopology) -> Result<(), ActiveHostClassificationError> {
    topology
        .validate()
        .map_err(|error| ActiveHostClassificationError::TopologyValidation(error.to_string()))?;
    if topology.outer.implementation != ProxyImplementation::OptiScaler {
        return Err(ActiveHostClassificationError::Topology(
            "outer implementation is not OptiScaler",
        ));
    }
    Ok(())
}

fn classify_fresh(
    topology: &GameProxyTopology,
    assessment: &TopologyHostAssessment,
    live_path: &renderpilot_domain::PathRef,
    live_snapshot: &PeerPathSnapshot,
    prepared_bytes: Option<Vec<u8>>,
    minimum_version: &renderpilot_domain::Version,
) -> Result<ActiveHostClassification, ActiveHostClassificationError> {
    if assessment.snapshot().lifecycle != HostLifecycle::InstallNew
        || assessment.initial_is_conflict()
    {
        return Err(ActiveHostClassificationError::Assessment(
            "a topology without a downstream requires an exact absent InstallNew assessment",
        ));
    }
    if !matches!(live_snapshot, PeerPathSnapshot::Absent) {
        return Err(ActiveHostClassificationError::Evidence {
            path: live_path.clone(),
            detail: "fresh active host slot is unexpectedly present",
        });
    }
    let prepared = validate_prepared_host(prepared_bytes.as_deref(), minimum_version)?;
    let digest = digest(prepared)?;
    let length = prepared.len() as u64;
    let binding = ManagedAddonFile::owned(
        live_path.clone(),
        ManagedFileBaseline::Absent,
        digest.clone(),
    );
    let planned_topology = observed_topology(topology, live_path, digest, length);
    Ok(ActiveHostClassification::Owned(ActiveHostOwnedPlan {
        target: live_path.clone(),
        binding,
        prepared_bytes: prepared_bytes.expect("validated prepared host bytes"),
        transition: ActiveHostTransition::Create,
        planned_topology,
    }))
}

fn classify_existing(
    topology: &GameProxyTopology,
    downstream: &renderpilot_domain::ProxyLink,
    assessment: &TopologyHostAssessment,
    live_path: &renderpilot_domain::PathRef,
    live_snapshot: &PeerPathSnapshot,
    prepared_bytes: Option<Vec<u8>>,
    minimum_version: &renderpilot_domain::Version,
) -> Result<ActiveHostClassification, ActiveHostClassificationError> {
    if downstream.implementation != ProxyImplementation::ReShade {
        return Err(ActiveHostClassificationError::Topology(
            "active downstream is not ReShade",
        ));
    }
    require_exact_path(&downstream.path, live_path)?;
    let Some(file) = live_snapshot.file() else {
        return Err(ActiveHostClassificationError::Evidence {
            path: live_path.clone(),
            detail: "topology downstream is present but its live file is absent",
        });
    };
    if downstream.receipt.ownership() != FileOwnership::Reused {
        return Err(ActiveHostClassificationError::Topology(
            "an initially owned downstream is foreign authority to Luma",
        ));
    }
    if downstream.receipt.digest() != file.digest()
        || downstream.receipt.identity() != file.identity()
    {
        return Err(ActiveHostClassificationError::Evidence {
            path: live_path.clone(),
            detail: "topology receipt differs from the retained downstream image",
        });
    }
    if assessment.initial_is_conflict()
        || assessment
            .snapshot()
            .identity
            .is_none_or(|identity| identity < ReshadeIdentity::Probable)
    {
        return Err(ActiveHostClassificationError::Assessment(
            "active downstream assessment is weak or conflicting",
        ));
    }
    match assessment.snapshot().lifecycle {
        HostLifecycle::ReuseUser | HostLifecycle::AdoptEmpty => {
            if assessment.snapshot().requires_host_download
                || assessment.assessment().initial_writes_host()
            {
                return Err(ActiveHostClassificationError::Assessment(
                    "a reused compatible downstream unexpectedly requests a host download",
                ));
            }
            Ok(ActiveHostClassification::Reused {
                binding: ManagedAddonFile::reused(live_path.clone(), file.digest().clone()),
                planned_topology: PlannedGameProxyTopology::Exact(topology.clone()),
            })
        }
        HostLifecycle::RepairEmpty => {
            let prepared = validate_prepared_host(prepared_bytes.as_deref(), minimum_version)?;
            let digest = digest(prepared)?;
            let length = prepared.len() as u64;
            let binding = ManagedAddonFile::owned(
                live_path.clone(),
                ManagedFileBaseline::Present {
                    sha256: file.digest().clone(),
                },
                digest.clone(),
            );
            Ok(ActiveHostClassification::Owned(ActiveHostOwnedPlan {
                target: live_path.clone(),
                binding,
                prepared_bytes: prepared_bytes.expect("validated prepared host bytes"),
                transition: ActiveHostTransition::Replace {
                    live_digest: file.digest().clone(),
                },
                planned_topology: observed_topology(topology, live_path, digest, length),
            }))
        }
        HostLifecycle::InstallNew | HostLifecycle::Conflict => {
            Err(ActiveHostClassificationError::Assessment(
                "an existing downstream requires a compatible ReuseUser, AdoptEmpty, or RepairEmpty assessment",
            ))
        }
    }
}

pub(crate) fn observed_topology(
    topology: &GameProxyTopology,
    downstream_path: &renderpilot_domain::PathRef,
    planned_sha256: renderpilot_domain::Sha256Hash,
    planned_length: u64,
) -> PlannedGameProxyTopology {
    PlannedGameProxyTopology::ObservedOwnedDownstream {
        id: topology.id.clone(),
        game_id: topology.game_id.clone(),
        root_slot: topology.root_slot.clone(),
        outer: topology.outer.clone(),
        implementation: ProxyImplementation::ReShade,
        downstream_path: downstream_path.clone(),
        downstream_origin: topology
            .downstream_origin
            .clone()
            .unwrap_or_else(|| topology.root_slot.clone()),
        root_prestate: topology.root_prestate,
        planned_sha256,
        planned_length,
    }
}
