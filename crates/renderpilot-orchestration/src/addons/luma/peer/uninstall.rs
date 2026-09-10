//! Decision-complete, read-only composition for an active Luma uninstall.

use renderpilot_domain::{
    AddonKind, GameProxyTopology, InstalledAddon, ManagedAddonFile, ManagedFileMode, PathRef,
    PlannedGameProxyTopology, ProxyRootPrestate, normalized_path_key,
};
use std::{borrow::Borrow, path::Path};

use crate::ServiceError;
use crate::addons::luma::dlss::PlannedDlss;
use crate::catalog::cascade::CascadeResult;
use crate::coordinated_files::CoordinatedFilePlan;
use crate::peer_mutation_executor::{ExactEndpoint, ExactEndpointProgram};

use super::catalog_cascade::lower_catalog_cascade;
use super::effects::{LumaPeerEffectAccumulator, LumaPeerOperationOrder};
use super::engine_uninstall::lower_engine_uninstall;
use super::managed_dlss::lower_managed_dlss_uninstall;
use super::managed_host::lower_managed_host_release;
use super::root_authority::LumaPeerRootAuthority;

mod error;
pub(super) use error::LumaActiveUninstallCompositionError;

/// One persisted managed binding paired with its already-computed DLSS plan.
#[derive(Debug, Clone)]
pub(crate) struct PlannedManagedDlssRelease {
    binding: ManagedAddonFile,
    plan: PlannedDlss,
}

impl PlannedManagedDlssRelease {
    pub(crate) fn new(binding: impl Borrow<ManagedAddonFile>, plan: PlannedDlss) -> Self {
        Self {
            binding: binding.borrow().clone(),
            plan,
        }
    }

    pub(crate) fn binding(&self) -> &ManagedAddonFile {
        &self.binding
    }

    pub(crate) fn plan(&self) -> &PlannedDlss {
        &self.plan
    }
}

/// Final active-Luma projection for the later peer package writer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LumaActiveUninstallComposition {
    program: ExactEndpointProgram,
    payloads: Vec<Option<Vec<u8>>>,
    planned_topology: PlannedGameProxyTopology,
}

impl LumaActiveUninstallComposition {
    pub(crate) fn into_parts(
        self,
    ) -> (
        ExactEndpointProgram,
        Vec<Option<Vec<u8>>>,
        PlannedGameProxyTopology,
    ) {
        (self.program, self.payloads, self.planned_topology)
    }
}

/// Composes the exact physical active-Luma uninstall projection.
pub(crate) fn compose_active_uninstall(
    record: &InstalledAddon,
    topology: &GameProxyTopology,
    authority: &LumaPeerRootAuthority,
    cascade: &CascadeResult,
    releases: impl AsRef<[PlannedManagedDlssRelease]>,
) -> Result<LumaActiveUninstallComposition, ServiceError> {
    compose_active_uninstall_inner(record, topology, authority, cascade, releases).map_err(
        |error| {
            ServiceError::invalid_input(format!(
                "active Luma uninstall composition rejected: {error}"
            ))
        },
    )
}

fn compose_active_uninstall_inner(
    record: &InstalledAddon,
    topology: &GameProxyTopology,
    authority: &LumaPeerRootAuthority,
    cascade: &CascadeResult,
    releases: impl AsRef<[PlannedManagedDlssRelease]>,
) -> Result<LumaActiveUninstallComposition, LumaActiveUninstallCompositionError> {
    validate_record_and_topology(record, topology)?;
    if cascade
        .catalog_claim()
        .is_some_and(|claim| claim.game_id() != record.game_id())
    {
        return Err(LumaActiveUninstallCompositionError::InvalidRecord(
            "catalog rollback claim belongs to another game",
        ));
    }

    authority
        .validate(
            Some(record),
            None,
            cascade.catalog_claim(),
            topology.participant_paths(),
        )
        .map_err(LumaActiveUninstallCompositionError::Authority)?;
    validate_topology_roots(authority, topology)?;

    let host_claim = find_host_claim(record, topology);
    let releases = releases.as_ref();
    let expected = validate_releases(record, host_claim, cascade, releases)?;
    let mut accumulator = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::Uninstall);

    lower_catalog_cascade(&cascade.rollback_specs, authority, &mut accumulator)?;
    lower_managed_releases(&expected, cascade, authority, &mut accumulator)?;
    lower_engine_uninstall(
        record,
        topology,
        authority.canonical_game_root_ref(),
        authority.external_capability_root_ref(),
        &mut accumulator,
    )?;

    let planned_topology = match host_claim {
        Some(claim) => {
            lower_managed_host_release(
                topology,
                claim,
                authority.canonical_game_root_ref(),
                &mut accumulator,
            )?;
            if claim.mode() == ManagedFileMode::Owned {
                PlannedGameProxyTopology::Exact(without_host(topology))
            } else {
                PlannedGameProxyTopology::Exact(topology.clone())
            }
        }
        None => PlannedGameProxyTopology::Exact(topology.clone()),
    };

    let effects = accumulator
        .finalize()
        .map_err(LumaActiveUninstallCompositionError::Effects)?
        .ok_or(LumaActiveUninstallCompositionError::EmptyProgram)?;
    let (program, payloads) = effects.into_parts();
    authority
        .validate(
            Some(record),
            None,
            cascade.catalog_claim(),
            program.endpoints().iter().map(ExactEndpoint::path),
        )
        .map_err(LumaActiveUninstallCompositionError::Authority)?;

    Ok(LumaActiveUninstallComposition {
        program,
        payloads,
        planned_topology,
    })
}
fn validate_record_and_topology(
    record: &InstalledAddon,
    topology: &GameProxyTopology,
) -> Result<(), LumaActiveUninstallCompositionError> {
    if record.kind() != AddonKind::Luma {
        return Err(LumaActiveUninstallCompositionError::InvalidRecord(
            "record kind is not Luma",
        ));
    }
    topology
        .validate()
        .map_err(LumaActiveUninstallCompositionError::Topology)?;
    if topology.outer.implementation != renderpilot_domain::ProxyImplementation::OptiScaler {
        return Err(LumaActiveUninstallCompositionError::InvalidRecord(
            "outer implementation is not OptiScaler",
        ));
    }
    let Some(downstream) = topology.downstream.as_ref() else {
        return Err(LumaActiveUninstallCompositionError::InvalidRecord(
            "active Luma topology has no downstream",
        ));
    };
    if downstream.implementation != renderpilot_domain::ProxyImplementation::ReShade {
        return Err(LumaActiveUninstallCompositionError::InvalidRecord(
            "active Luma topology downstream is not ReShade",
        ));
    }
    if record.game_id() != &topology.game_id {
        return Err(LumaActiveUninstallCompositionError::InvalidRecord(
            "record and topology belong to different games",
        ));
    }
    Ok(())
}

fn validate_topology_roots(
    authority: &LumaPeerRootAuthority,
    topology: &GameProxyTopology,
) -> Result<(), LumaActiveUninstallCompositionError> {
    for path in topology.participant_paths() {
        let root = authority
            .authorized_root(path)
            .map_err(LumaActiveUninstallCompositionError::Authority)?;
        if normalized_path_key(root.as_str())
            != normalized_path_key(authority.canonical_game_root_ref().as_str())
        {
            return Err(LumaActiveUninstallCompositionError::Authority(
                crate::failed(format!(
                    "Luma topology path is outside the sealed game root: {}",
                    path.as_str()
                )),
            ));
        }
    }
    Ok(())
}
fn find_host_claim<'a>(
    record: &'a InstalledAddon,
    topology: &GameProxyTopology,
) -> Option<&'a ManagedAddonFile> {
    let downstream = topology.downstream.as_ref()?;
    record.managed_files().iter().find(|managed| {
        normalized_path_key(managed.path().as_str())
            == normalized_path_key(downstream.path.as_str())
    })
}

fn validate_releases<'a>(
    record: &InstalledAddon,
    host_claim: Option<&ManagedAddonFile>,
    cascade: &CascadeResult,
    releases: &'a [PlannedManagedDlssRelease],
) -> Result<Vec<&'a PlannedManagedDlssRelease>, LumaActiveUninstallCompositionError> {
    let mut expected = std::collections::BTreeMap::new();
    for binding in record.managed_files() {
        if host_claim.is_some_and(|host| {
            normalized_path_key(host.path().as_str())
                == normalized_path_key(binding.path().as_str())
        }) {
            continue;
        }
        if !is_dlss_path(binding.path()) {
            return Err(LumaActiveUninstallCompositionError::NonDlssBinding(
                binding.path().clone(),
            ));
        }
        expected.insert(normalized_path_key(binding.path().as_str()), binding);
    }

    let mut seen = std::collections::BTreeSet::new();
    let mut matched = Vec::with_capacity(expected.len());
    for release in releases {
        let path = release.binding().path();
        let key = normalized_path_key(path.as_str());
        let Some(record_binding) = expected.get(&key) else {
            return Err(LumaActiveUninstallCompositionError::ExtraneousRelease(
                path.clone(),
            ));
        };
        if !seen.insert(key) {
            return Err(LumaActiveUninstallCompositionError::DuplicateRelease(
                path.clone(),
            ));
        }
        if *record_binding != release.binding() {
            return Err(LumaActiveUninstallCompositionError::ReleaseBindingMismatch(
                path.clone(),
            ));
        }
        matched.push(release);
    }
    for (key, binding) in expected {
        if !seen.contains(&key) {
            return Err(LumaActiveUninstallCompositionError::MissingRelease(
                binding.path().clone(),
            ));
        }
    }

    for release in &matched {
        let binding = release.binding();
        if consumed_by_catalog(binding, cascade) {
            if binding.mode() == ManagedFileMode::Reused {
                return Err(LumaActiveUninstallCompositionError::ReusedCatalogPath(
                    binding.path().clone(),
                ));
            }
            if !is_unbound_keep(release.plan()) {
                return Err(LumaActiveUninstallCompositionError::ConsumedReleasePlan(
                    binding.path().clone(),
                ));
            }
        }
    }
    Ok(matched)
}

fn lower_managed_releases(
    expected: &[&PlannedManagedDlssRelease],
    cascade: &CascadeResult,
    authority: &LumaPeerRootAuthority,
    accumulator: &mut LumaPeerEffectAccumulator,
) -> Result<(), LumaActiveUninstallCompositionError> {
    for release in expected {
        let binding = release.binding();
        if consumed_by_catalog(binding, cascade) {
            continue;
        }
        let root = authority
            .authorized_root(binding.path())
            .map_err(LumaActiveUninstallCompositionError::Authority)?;
        lower_managed_dlss_uninstall(binding, release.plan(), root, accumulator)?;
    }
    Ok(())
}
fn consumed_by_catalog(binding: &ManagedAddonFile, cascade: &CascadeResult) -> bool {
    cascade
        .rollback_specs
        .iter()
        .any(|spec| spec.contains_path(Path::new(binding.path().as_str())))
}

fn is_unbound_keep(plan: &PlannedDlss) -> bool {
    plan.binding.is_none() && matches!(plan.action, CoordinatedFilePlan::Keep)
}

fn is_dlss_path(path: &PathRef) -> bool {
    Path::new(path.as_str())
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case(renderpilot_detection::NVNGX_DLSS_FILE_NAME))
}

fn without_host(topology: &GameProxyTopology) -> GameProxyTopology {
    let mut after = topology.clone();
    after.downstream = None;
    after.downstream_origin = None;
    after.root_prestate = ProxyRootPrestate::Absent;
    after
}
