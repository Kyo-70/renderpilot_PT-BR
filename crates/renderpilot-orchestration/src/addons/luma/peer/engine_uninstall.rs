//! Read-only projection of Luma's generic engine-owned uninstall files.
//!
//! Only `created_files` and `backed_up_files` belong to this adapter.  Managed
//! bindings and active-topology custody are separate contracts and are never
//! lowered here.

use std::{
    collections::BTreeMap,
    error::Error,
    fmt,
    path::{Component, Path},
};

use renderpilot_domain::{
    AddonKind, GameProxyTopology, InstalledAddon, ManagedAddonFile, PathRef, PeerTransitionError,
    ProxyTopologyError, normalized_path_key, normalized_path_relation,
};

use crate::ServiceError;
use crate::addons::luma::dgvoodoo::is_dependency_basename;
use crate::peer_mutation_executor::PeerPathSnapshot;

use super::dgvoodoo::{DgVoodooDecision, DgVoodooLoweringError, lower_dgvoodoo_decision};
use super::effects::{LumaPeerEffectAccumulator, LumaPeerEffectError};
use super::generic::{LumaGenericDecision, LumaGenericLoweringError, lower_generic_decision};
use super::snapshot_input::LumaSnapshotInputError;

/// Failure to project generic engine-owned Luma files into peer effects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum EngineUninstallError {
    InvalidRecord(&'static str),
    Topology(ProxyTopologyError),
    Domain(PeerTransitionError),
    DuplicateCreated(PathRef),
    DuplicateBacked(PathRef),
    BackedWithoutCreated(PathRef),
    EngineManagedOverlap(PathRef),
    TopologyOverlap { engine: PathRef, topology: PathRef },
    NonCanonicalPath(PathRef),
    PathOutsideAuthorizedRoots(PathRef),
    DependencyOutsideGameRoot(PathRef),
    Observation { path: PathRef, error: ServiceError },
    Snapshot(LumaSnapshotInputError),
    Effects(LumaPeerEffectError),
    Generic(LumaGenericLoweringError),
    DgVoodoo(DgVoodooLoweringError),
}

impl fmt::Display for EngineUninstallError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRecord(reason) => {
                write!(
                    formatter,
                    "invalid Luma generic-engine uninstall record: {reason}"
                )
            }
            Self::Topology(error) => error.fmt(formatter),
            Self::Domain(error) => error.fmt(formatter),
            Self::DuplicateCreated(path) => {
                write!(formatter, "duplicate created engine path: {path}")
            }
            Self::DuplicateBacked(path) => {
                write!(formatter, "duplicate backed engine path: {path}")
            }
            Self::BackedWithoutCreated(path) => {
                write!(formatter, "backed engine path has no created claim: {path}")
            }
            Self::EngineManagedOverlap(path) => {
                write!(
                    formatter,
                    "generic engine path overlaps a managed claim: {path}"
                )
            }
            Self::TopologyOverlap { engine, topology } => write!(
                formatter,
                "generic engine path {engine} overlaps active topology path {topology}"
            ),
            Self::NonCanonicalPath(path) => {
                write!(formatter, "generic engine path is not canonical: {path}")
            }
            Self::PathOutsideAuthorizedRoots(path) => {
                write!(
                    formatter,
                    "generic engine path is outside authorized roots: {path}"
                )
            }
            Self::DependencyOutsideGameRoot(path) => write!(
                formatter,
                "dgVoodoo dependency must be under the authorized game root: {path}"
            ),
            Self::Observation { path, error } => {
                write!(
                    formatter,
                    "failed to observe generic engine path {path}: {error}"
                )
            }
            Self::Snapshot(error) => error.fmt(formatter),
            Self::Effects(error) => error.fmt(formatter),
            Self::Generic(error) => error.fmt(formatter),
            Self::DgVoodoo(error) => error.fmt(formatter),
        }
    }
}

impl Error for EngineUninstallError {}

impl From<ProxyTopologyError> for EngineUninstallError {
    fn from(error: ProxyTopologyError) -> Self {
        Self::Topology(error)
    }
}

impl From<PeerTransitionError> for EngineUninstallError {
    fn from(error: PeerTransitionError) -> Self {
        Self::Domain(error)
    }
}

impl From<LumaSnapshotInputError> for EngineUninstallError {
    fn from(error: LumaSnapshotInputError) -> Self {
        Self::Snapshot(error)
    }
}

impl From<LumaPeerEffectError> for EngineUninstallError {
    fn from(error: LumaPeerEffectError) -> Self {
        Self::Effects(error)
    }
}

impl From<LumaGenericLoweringError> for EngineUninstallError {
    fn from(error: LumaGenericLoweringError) -> Self {
        Self::Generic(error)
    }
}

impl From<DgVoodooLoweringError> for EngineUninstallError {
    fn from(error: DgVoodooLoweringError) -> Self {
        Self::DgVoodoo(error)
    }
}

#[derive(Default)]
struct EngineFiles<'a> {
    created: Option<&'a PathRef>,
    backed: Option<&'a PathRef>,
}

/// Lowers generic engine-owned Luma uninstall files into an existing effect
/// accumulator. Structural validation completes before the first snapshot
/// observation, and all physical reads are scoped to one of the explicit
/// authorized roots.
pub(super) fn lower_engine_uninstall(
    record: &InstalledAddon,
    topology: &GameProxyTopology,
    game_root: &PathRef,
    payload_root: Option<&PathRef>,
    accumulator: &mut LumaPeerEffectAccumulator,
) -> Result<(), EngineUninstallError> {
    if record.kind() != AddonKind::Luma {
        return Err(EngineUninstallError::InvalidRecord(
            "record kind is not Luma",
        ));
    }
    topology.validate()?;
    let files = collect_engine_files(record)?;
    let authorities = validate_structure(&files, record, topology, game_root, payload_root)?;
    for (key, entry) in &files {
        let Some(live_path) = entry.created else {
            continue;
        };
        let authority = authorities
            .get(key)
            .expect("validated engine path authority");
        let live_snapshot = observe(live_path, authority.root())?;
        let dependency = is_dependency_basename(Path::new(live_path.as_str()));
        if entry.backed.is_none() {
            if dependency {
                lower_dgvoodoo_decision(
                    DgVoodooDecision::ReleaseOwnedAbsent {
                        live_path,
                        live_snapshot: &live_snapshot,
                    },
                    accumulator,
                )?;
            } else {
                lower_generic_decision(
                    LumaGenericDecision::RemoveCreated {
                        live_path,
                        live_snapshot: &live_snapshot,
                    },
                    accumulator,
                )?;
            }
        } else {
            let sidecar_path = renderpilot_domain::managed_sidecar_path(live_path)?;
            let sidecar_snapshot = observe(&sidecar_path, authority.root())?;
            if dependency {
                lower_dgvoodoo_decision(
                    DgVoodooDecision::ReleaseOwnedPresent {
                        live_path,
                        sidecar_path: &sidecar_path,
                        live_snapshot: &live_snapshot,
                        sidecar_snapshot: &sidecar_snapshot,
                    },
                    accumulator,
                )?;
            } else {
                lower_generic_decision(
                    LumaGenericDecision::ReleaseBacked {
                        live_path,
                        sidecar_path: &sidecar_path,
                        live_snapshot: &live_snapshot,
                        sidecar_snapshot: &sidecar_snapshot,
                    },
                    accumulator,
                )?;
            }
        }
    }
    Ok(())
}

fn collect_engine_files<'a>(
    record: &'a InstalledAddon,
) -> Result<BTreeMap<String, EngineFiles<'a>>, EngineUninstallError> {
    let mut files: BTreeMap<String, EngineFiles<'a>> = BTreeMap::new();
    for path in record.created_files() {
        let key = normalized_path_key(path.as_str());
        let entry = files.entry(key).or_default();
        if entry.created.replace(path).is_some() {
            return Err(EngineUninstallError::DuplicateCreated(path.clone()));
        }
    }
    for path in record.backed_up_files() {
        let key = normalized_path_key(path.as_str());
        let entry = files.entry(key).or_default();
        if entry.backed.replace(path).is_some() {
            return Err(EngineUninstallError::DuplicateBacked(path.clone()));
        }
    }
    for entry in files.values() {
        if entry.created.is_none() {
            return Err(EngineUninstallError::BackedWithoutCreated(
                entry.backed.expect("backed-only entry").clone(),
            ));
        }
    }
    Ok(files)
}

fn validate_structure<'root>(
    files: &BTreeMap<String, EngineFiles<'_>>,
    record: &InstalledAddon,
    topology: &GameProxyTopology,
    game_root: &'root PathRef,
    payload_root: Option<&'root PathRef>,
) -> Result<BTreeMap<String, EngineAuthority<'root>>, EngineUninstallError> {
    let topology_paths: Vec<_> = topology.participant_paths().collect();
    let managed_paths: Vec<_> = record
        .managed_files()
        .iter()
        .map(ManagedAddonFile::path)
        .collect();
    let mut authorities = BTreeMap::new();
    for entry in files.values() {
        let path = entry.created.expect("validated created path");
        require_canonical(path)?;
        if managed_paths
            .iter()
            .any(|managed| normalized_path_relation(path.as_str(), managed.as_str()).overlaps())
        {
            return Err(EngineUninstallError::EngineManagedOverlap(path.clone()));
        }
        if let Some(topology_path) = topology_paths.iter().find(|topology_path| {
            normalized_path_relation(path.as_str(), topology_path.as_str()).overlaps()
        }) {
            return Err(EngineUninstallError::TopologyOverlap {
                engine: path.clone(),
                topology: (*topology_path).clone(),
            });
        }
        let authority = select_authority(path, game_root, payload_root)?;
        if is_dependency_basename(Path::new(path.as_str())) && !authority.is_game() {
            return Err(EngineUninstallError::DependencyOutsideGameRoot(
                path.clone(),
            ));
        }
        if entry.backed.is_some() {
            let sidecar_path = renderpilot_domain::managed_sidecar_path(path)?;
            if !crate::paths::is_within(
                Path::new(sidecar_path.as_str()),
                Path::new(authority.root().as_str()),
            ) {
                return Err(EngineUninstallError::PathOutsideAuthorizedRoots(
                    sidecar_path,
                ));
            }
        }
        authorities.insert(normalized_path_key(path.as_str()), authority);
    }
    Ok(authorities)
}

enum EngineAuthority<'a> {
    Game(&'a PathRef),
    Payload(&'a PathRef),
}

impl EngineAuthority<'_> {
    fn root(&self) -> &PathRef {
        match self {
            Self::Game(root) | Self::Payload(root) => root,
        }
    }

    fn is_game(&self) -> bool {
        matches!(self, Self::Game(_))
    }
}

fn select_authority<'a>(
    path_ref: &PathRef,
    game_root: &'a PathRef,
    payload_root: Option<&'a PathRef>,
) -> Result<EngineAuthority<'a>, EngineUninstallError> {
    let path = Path::new(path_ref.as_str());
    if crate::paths::is_within(path, Path::new(game_root.as_str())) {
        return Ok(EngineAuthority::Game(game_root));
    }
    if let Some(payload_root) = payload_root
        && crate::paths::is_within(path, Path::new(payload_root.as_str()))
    {
        return Ok(EngineAuthority::Payload(payload_root));
    }
    Err(EngineUninstallError::PathOutsideAuthorizedRoots(
        path_ref.clone(),
    ))
}

fn require_canonical(path: &PathRef) -> Result<(), EngineUninstallError> {
    if Path::new(path.as_str())
        .components()
        .any(|component| matches!(component, Component::CurDir | Component::ParentDir))
    {
        return Err(EngineUninstallError::NonCanonicalPath(path.clone()));
    }
    Ok(())
}

fn observe(
    path: &PathRef,
    authorized_root: &PathRef,
) -> Result<PeerPathSnapshot, EngineUninstallError> {
    crate::peer_mutation_executor::observe_peer_path_snapshot(path, authorized_root).map_err(
        |error| EngineUninstallError::Observation {
            path: path.clone(),
            error,
        },
    )
}
