//! Exact downstream ReShade-slot assessment for persisted proxy topologies.

use std::path::{Component, Path};

use renderpilot_domain::{Architecture, PathRef, Sha256Hash, Version, normalized_path_key};

use crate::ServiceError;
use crate::addons::reshade::scan::{
    self, ReshadeAddonSupport, ReshadeContent, ReshadeHost, ReshadeHostAction, ReshadeIdentity,
};
use crate::peer_mutation_executor::PeerPathSnapshot;

use super::{HostAssessment, HostConflictKind, HostLifecycle};

struct ObservedTopologyHost {
    host: ReshadeHost,
    digest: Option<Sha256Hash>,
    length: Option<u64>,
    identity: Option<ReshadeIdentity>,
    addon_support: Option<ReshadeAddonSupport>,
    version: Option<Version>,
    conflict_kind: Option<HostConflictKind>,
}

/// The immutable facts and policy outcome for the topology's exact downstream
/// ReShade slot. Unlike the ordinary folder scan, this value never considers
/// another proxy candidate, so an outer OptiScaler DLL cannot turn into a
/// second-host conflict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TopologyHostAssessment {
    /// The ordinary host-policy decision used by the later install phases.
    pub(crate) assessment: HostAssessment,
    /// Equality key for the exact downstream observation.
    pub(crate) snapshot: TopologyHostSnapshot,
}

/// Stable, non-cosmetic key for one topology downstream assessment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TopologyHostSnapshot {
    pub(crate) exact_path: String,
    pub(crate) present: bool,
    pub(crate) digest: Option<Sha256Hash>,
    pub(crate) length: Option<u64>,
    pub(crate) identity: Option<ReshadeIdentity>,
    pub(crate) addon_support: Option<ReshadeAddonSupport>,
    pub(crate) version: Option<Version>,
    pub(crate) content: ReshadeContent,
    pub(crate) lifecycle: HostLifecycle,
    pub(crate) action: ReshadeHostAction,
    pub(crate) requires_host_download: bool,
}

impl TopologyHostAssessment {
    #[must_use]
    pub(crate) fn assessment(&self) -> &HostAssessment {
        &self.assessment
    }

    #[must_use]
    pub(crate) fn snapshot(&self) -> &TopologyHostSnapshot {
        &self.snapshot
    }

    #[must_use]
    pub(crate) fn requires_host_download(&self) -> bool {
        self.assessment.initial_writes_host()
    }

    #[must_use]
    pub(crate) fn initial_is_conflict(&self) -> bool {
        self.assessment.initial_is_conflict()
    }
}

/// Assesses only the exact downstream slot represented by a persisted proxy
/// topology. This active-only route intentionally does not perform a folder
/// scan: the known OptiScaler outer slot and unrelated proxy DLLs are not host
/// candidates for this decision.
#[cfg(test)]
pub(crate) fn assess_topology_downstream_for_tool(
    game_root: &Path,
    downstream_path: &Path,
    tool_name: &'static str,
    min_host_version: Option<&Version>,
) -> Result<TopologyHostAssessment, ServiceError> {
    validate_topology_downstream_path(game_root, downstream_path)?;

    let observed = scan::observe_exact_host_file(downstream_path)?;
    let observed = match observed {
        scan::ExactHostObservation::Absent => ObservedTopologyHost {
            host: ReshadeHost::Absent,
            digest: None,
            length: None,
            identity: None,
            addon_support: None,
            version: None,
            conflict_kind: None,
        },
        scan::ExactHostObservation::Present {
            digest,
            length,
            inspection,
        } => observed_host_from_inspection(downstream_path, digest, length, &inspection),
    };

    let assessment = super::finish_assessment(
        game_root,
        super::AbsentHostTarget {
            path: downstream_path.to_path_buf(),
            slot: "ReShade64.dll",
        },
        observed.host,
        observed.conflict_kind,
        tool_name,
        min_host_version,
        &[],
    );
    Ok(topology_assessment(
        downstream_path,
        observed.digest,
        observed.length,
        observed.identity,
        observed.addon_support,
        observed.version,
        assessment,
    ))
}

/// Determines whether an active topology operation must download a replacement
/// ReShade host from one retained exact-slot snapshot.
///
/// This is deliberately content-blind: host download selection cannot depend on
/// a second read of `ReShade.ini` or an effect directory. A present snapshot is
/// first checked for internal digest/length coherence, then its PE facts are
/// classified using the same identity and add-on API policy as the ordinary
/// topology assessor.
pub(crate) fn probe_topology_host_download(
    game_root: &Path,
    downstream_path: &PathRef,
    snapshot: &PeerPathSnapshot,
    min_host_version: Option<&Version>,
) -> Result<bool, ServiceError> {
    validate_topology_downstream_path(game_root, Path::new(downstream_path.as_str()))?;
    let Some((_, _, inspection)) = retained_host_facts(snapshot, downstream_path)? else {
        return Ok(true);
    };
    ensure_active_host_architecture(&inspection, downstream_path)?;
    let (identity, addon_support, version, custom) = classify_inspection(&inspection);
    if custom {
        return Err(ServiceError::invalid_input(
            "retained exact ReShade host identifies a recognized custom build",
        ));
    }
    if identity == ReshadeIdentity::Weak {
        return Err(ServiceError::invalid_input(
            "retained exact ReShade host has no trustworthy ReShade identity",
        ));
    }
    if addon_support != ReshadeAddonSupport::Full {
        return Ok(true);
    }
    Ok(match min_host_version {
        Some(minimum) => version.as_ref().is_none_or(|version| version < minimum),
        None => false,
    })
}

/// Builds the phase-3 active topology decision from retained host bytes and a
/// caller-supplied ReShade content classification. No filesystem or config read
/// occurs here; changing `ReShade.ini` after the retained snapshot therefore
/// cannot alter this result.
pub(crate) fn assess_topology_downstream_from_snapshot(
    game_root: &Path,
    downstream_path: &PathRef,
    snapshot: &PeerPathSnapshot,
    content: ReshadeContent,
    tool_name: &'static str,
    min_host_version: Option<&Version>,
) -> Result<TopologyHostAssessment, ServiceError> {
    validate_topology_downstream_path(game_root, Path::new(downstream_path.as_str()))?;
    let retained = retained_host_facts(snapshot, downstream_path)?;
    let observed = match retained {
        None => ObservedTopologyHost {
            host: ReshadeHost::Absent,
            digest: None,
            length: None,
            identity: None,
            addon_support: None,
            version: None,
            conflict_kind: None,
        },
        Some((digest, length, inspection)) => {
            ensure_active_host_architecture(&inspection, downstream_path)?;
            observed_host_from_inspection(
                Path::new(downstream_path.as_str()),
                digest,
                length,
                &inspection,
            )
        }
    };

    let assessment = super::finish_assessment_with_content(
        Path::new(downstream_path.as_str()).to_path_buf(),
        "ReShade64.dll",
        observed.host,
        observed.conflict_kind,
        tool_name,
        min_host_version,
        content,
    );
    Ok(topology_assessment(
        Path::new(downstream_path.as_str()),
        observed.digest,
        observed.length,
        observed.identity,
        observed.addon_support,
        observed.version,
        assessment,
    ))
}

fn topology_assessment(
    downstream_path: &Path,
    digest: Option<Sha256Hash>,
    length: Option<u64>,
    identity: Option<ReshadeIdentity>,
    addon_support: Option<ReshadeAddonSupport>,
    version: Option<Version>,
    assessment: HostAssessment,
) -> TopologyHostAssessment {
    let snapshot = TopologyHostSnapshot {
        exact_path: normalized_path_key(&downstream_path.to_string_lossy()),
        present: digest.is_some(),
        digest,
        length,
        identity,
        addon_support,
        version,
        content: assessment.content(),
        lifecycle: assessment.lifecycle,
        action: assessment.action,
        requires_host_download: assessment.initial_writes_host(),
    };
    TopologyHostAssessment {
        assessment,
        snapshot,
    }
}

fn ensure_active_host_architecture(
    inspection: &renderpilot_detection::PeInspection,
    path: &PathRef,
) -> Result<(), ServiceError> {
    if inspection.architecture != Some(Architecture::X64) {
        return Err(ServiceError::invalid_input(format!(
            "retained active ReShade host is not an x64 PE image: {}",
            path.as_str()
        )));
    }
    Ok(())
}

fn retained_host_facts(
    snapshot: &PeerPathSnapshot,
    path: &PathRef,
) -> Result<Option<(Sha256Hash, u64, renderpilot_detection::PeInspection)>, ServiceError> {
    let PeerPathSnapshot::File(_) = snapshot else {
        return Ok(None);
    };
    let file = snapshot.file().ok_or_else(|| {
        ServiceError::invalid_input(format!(
            "retained exact ReShade host snapshot has no file metadata: {}",
            path.as_str()
        ))
    })?;
    if file.identity().trim().is_empty() || file.identity().contains('\0') {
        return Err(ServiceError::invalid_input(format!(
            "retained exact ReShade host has an invalid file identity: {}",
            path.as_str()
        )));
    }
    let bytes = snapshot.bytes().ok_or_else(|| {
        ServiceError::invalid_input(format!(
            "retained exact ReShade host snapshot has no bytes: {}",
            path.as_str()
        ))
    })?;
    let length = u64::try_from(bytes.len()).map_err(|error| {
        ServiceError::invalid_input(format!(
            "retained exact ReShade host is too large at {}: {error}",
            path.as_str()
        ))
    })?;
    if file.length() != length {
        return Err(ServiceError::invalid_input(format!(
            "retained exact ReShade host length drifted at {}",
            path.as_str()
        )));
    }
    let digest = renderpilot_detection::sha256_bytes(bytes).map_err(|error| {
        ServiceError::invalid_input(format!(
            "failed to hash retained exact ReShade host {}: {error}",
            path.as_str()
        ))
    })?;
    if &digest != file.digest() {
        return Err(ServiceError::invalid_input(format!(
            "retained exact ReShade host digest drifted at {}",
            path.as_str()
        )));
    }
    Ok(Some((
        digest,
        length,
        renderpilot_detection::inspect_pe_bytes(bytes),
    )))
}

fn classify_inspection(
    inspection: &renderpilot_detection::PeInspection,
) -> (ReshadeIdentity, ReshadeAddonSupport, Option<Version>, bool) {
    let has_reshade_export = inspection
        .export_names
        .as_deref()
        .unwrap_or(&[])
        .iter()
        .any(|name| name.eq_ignore_ascii_case("ReShadeVersion"));
    let metadata_points_to_reshade = scan::version_strings_point_to_reshade(&inspection.identity);
    let identity = if has_reshade_export {
        ReshadeIdentity::Confirmed
    } else if metadata_points_to_reshade {
        ReshadeIdentity::Probable
    } else {
        ReshadeIdentity::Weak
    };
    let addon_support = scan::addon_support_from_exports_for_topology(
        inspection.export_names.as_deref(),
        has_reshade_export,
    );
    let custom = scan::is_known_custom_identity(&inspection.identity);
    (identity, addon_support, inspection.version.clone(), custom)
}

fn observed_host_from_inspection(
    downstream_path: &Path,
    digest: Sha256Hash,
    length: u64,
    inspection: &renderpilot_detection::PeInspection,
) -> ObservedTopologyHost {
    let (identity, addon_support, version, custom) = classify_inspection(inspection);
    let host = ReshadeHost::Present {
        path: downstream_path.to_path_buf(),
        slot: downstream_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("ReShade64.dll")
            .to_owned(),
        version: version.clone(),
        addon_support,
        identity,
        active: scan::ActiveSlotState {
            state: scan::SlotActivity::Active,
            reason: scan::ActiveSlotReason::DetectedByMatcher,
        },
    };
    let conflict_kind = if custom {
        Some(HostConflictKind::KnownCustomBuild)
    } else if identity == ReshadeIdentity::Weak {
        Some(HostConflictKind::WeakIdentity)
    } else {
        None
    };
    ObservedTopologyHost {
        host,
        digest: Some(digest),
        length: Some(length),
        identity: Some(identity),
        addon_support: Some(addon_support),
        version,
        conflict_kind,
    }
}

fn validate_topology_downstream_path(
    game_root: &Path,
    downstream_path: &Path,
) -> Result<(), ServiceError> {
    if !game_root.is_absolute() || !downstream_path.is_absolute() {
        return Err(ServiceError::invalid_input(
            "topology downstream host path must be absolute",
        ));
    }
    if game_root
        .components()
        .chain(downstream_path.components())
        .any(|component| matches!(component, Component::CurDir | Component::ParentDir))
    {
        return Err(ServiceError::invalid_input(
            "topology downstream host path must not contain dot or parent traversal components",
        ));
    }
    let expected = game_root.join("ReShade64.dll");
    if normalized_path_key(&expected.to_string_lossy())
        != normalized_path_key(&downstream_path.to_string_lossy())
    {
        return Err(ServiceError::invalid_input(
            "topology downstream host path must be the direct ReShade64.dll child of the game root",
        ));
    }
    Ok(())
}
