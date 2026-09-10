//! Resolve uninstall roots, cascade effects, release plans, and mutation plans.
//!
//! Inactive records produce the existing durable workset; active proxy topologies
//! produce the complete peer transition consumed by the peer executor.

use std::path::Path;

use renderpilot_application::{GameRepository, ProxyTopologyRepository};
use renderpilot_domain::{
    AddonKind, ComponentId, GameId, GameProxyTopology, InstalledAddon, LibraryComponent,
    ManagedFileMode, NormalizedPathRelation, PlannedGameProxyTopology, normalized_path_key,
    normalized_path_relation,
};

use crate::addons::luma::dlss::PlannedDlss;
use crate::addons::luma::errors;
use crate::addons::luma::peer::{
    LumaPeerRootAuthority, PlannedManagedDlssRelease, compose_active_uninstall,
};
use crate::addons::mutation_targets::DurableWorkset;
use crate::addons::records;
use crate::catalog::cascade::{CascadeResult, ValidatedRollbackPlan};
use crate::peer_mutation_executor::ExactEndpointProgram;
use crate::{Context, ServiceError};

/// Filesystem reverse + DB commit inputs for the durable uninstall body.
pub(super) struct UninstallApply {
    pub(super) record: InstalledAddon,
    pub(super) rollback_specs: Vec<ValidatedRollbackPlan>,
    pub(super) next_components: Vec<LibraryComponent>,
    pub(super) release_plans: Vec<PlannedDlss>,
    pub(super) rolled_back_ids: Vec<ComponentId>,
}

/// Planned uninstall inputs for the durable apply/commit path.
pub(super) enum UninstallPlan {
    Inactive(Box<InactiveUninstallPlan>),
    Active(Box<ActiveUninstallApply>),
}

/// Owned inactive-topology inputs retained through durable application and its
/// catalog journal.
pub(super) struct InactiveUninstallPlan {
    pub(super) apply: UninstallApply,
    pub(super) workset: DurableWorkset,
}

/// Owned active-topology inputs kept alive through peer preparation, apply,
/// commit, and the post-commit catalog journal.
pub(super) struct ActiveUninstallApply {
    pub(super) record: InstalledAddon,
    pub(super) topology: GameProxyTopology,
    pub(super) authority: LumaPeerRootAuthority,
    pub(super) cascade: CascadeResult,
    pub(super) program: ExactEndpointProgram,
    pub(super) payloads: Vec<Option<Vec<u8>>>,
    pub(super) planned_topology: PlannedGameProxyTopology,
}

pub(super) fn plan_uninstall(
    context: &Context,
    game_id: &GameId,
) -> Result<UninstallPlan, ServiceError> {
    let record = records::record_of_kind(context, game_id, AddonKind::Luma)?
        .ok_or_else(errors::not_installed)?;
    let topology = context.storage().get_proxy_topology(game_id)?;
    match topology {
        Some(topology) => plan_active_uninstall(context, game_id, record, topology),
        None => plan_inactive_uninstall(context, game_id, record),
    }
}

fn plan_active_uninstall(
    context: &Context,
    game_id: &GameId,
    record: InstalledAddon,
    topology: GameProxyTopology,
) -> Result<UninstallPlan, ServiceError> {
    let game = context.storage().require_game(game_id)?;
    let downstream = topology.downstream.as_ref().ok_or_else(|| {
        ServiceError::invalid_input("active Luma uninstall topology has no downstream host")
    })?;
    let authority = resolve_active_runtime_authority(
        Path::new(game.install_path().as_str()),
        &topology,
        Path::new(downstream.path.as_str()),
    )?;
    authority.validate(Some(&record), None, None, topology.participant_paths())?;

    let host_key = normalized_path_key(downstream.path.as_str());
    let remaining_owned = record
        .managed_files()
        .iter()
        .filter(|managed| managed.mode() == ManagedFileMode::Owned)
        .filter(|managed| normalized_path_key(managed.path().as_str()) != host_key)
        .map(|managed| Path::new(managed.path().as_str()).to_path_buf())
        .collect::<Vec<_>>();
    let cascade = crate::catalog::cascade::cascade_for_managed_paths(
        context.storage(),
        game_id,
        &remaining_owned,
    )?;
    let releases = record
        .managed_files()
        .iter()
        .filter(|managed| normalized_path_key(managed.path().as_str()) != host_key)
        .map(|managed| {
            let consumed = cascade
                .rollback_specs
                .iter()
                .any(|spec| spec.contains_path(Path::new(managed.path().as_str())));
            crate::addons::luma::dlss::plan_release_binding(context, game_id, managed, consumed)
                .map(|plan| PlannedManagedDlssRelease::new(managed, plan))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let composition =
        compose_active_uninstall(&record, &topology, &authority, &cascade, &releases)?;
    let (program, payloads, planned_topology) = composition.into_parts();

    Ok(UninstallPlan::Active(Box::new(ActiveUninstallApply {
        record,
        topology,
        authority,
        cascade,
        program,
        payloads,
        planned_topology,
    })))
}

pub(super) fn resolve_active_runtime_authority(
    catalog_game_root: &Path,
    topology: &GameProxyTopology,
    downstream_path: &Path,
) -> Result<LumaPeerRootAuthority, ServiceError> {
    let runtime_root = Path::new(topology.root_slot.as_str())
        .parent()
        .ok_or_else(|| {
            ServiceError::invalid_input("active Luma uninstall topology root slot has no parent")
        })?;
    let runtime_root = crate::paths::canonical_candidate(runtime_root).map_err(|error| {
        ServiceError::invalid_input(format!(
            "active Luma uninstall runtime root is invalid: {error}"
        ))
    })?;
    let catalog_game_root =
        crate::paths::canonical_candidate(catalog_game_root).map_err(|error| {
            ServiceError::invalid_input(format!(
                "active Luma uninstall game root is invalid: {error}"
            ))
        })?;
    if !matches!(
        normalized_path_relation(
            &catalog_game_root.to_string_lossy(),
            &runtime_root.to_string_lossy(),
        ),
        NormalizedPathRelation::Equal | NormalizedPathRelation::LeftAncestor
    ) {
        return Err(ServiceError::invalid_input(
            "active Luma uninstall topology runtime root is outside the catalog game root",
        ));
    }
    LumaPeerRootAuthority::resolve(&runtime_root, downstream_path)
}

fn plan_inactive_uninstall(
    context: &Context,
    game_id: &GameId,
    record: InstalledAddon,
) -> Result<UninstallPlan, ServiceError> {
    // Installed add-on records intentionally survive catalog pruning, so an
    // uninstall must remain possible even if the game row was removed. In that
    // case the recorded add-on directory is the only safe mutation root.
    let game_root = crate::catalog::game_root_for_mutation(
        context.storage(),
        game_id,
        Path::new(record.addon_file().as_str())
            .parent()
            .map(Path::to_path_buf),
    )
    .map_err(|error| {
        if matches!(
            error.kind(),
            renderpilot_application::AppErrorKind::GameNotFound
        ) {
            errors::failed("Luma install record has no filesystem root".to_owned())
        } else {
            error.into()
        }
    })?;

    let owned_paths = records::owned_managed_paths(&record);
    let cascade = crate::catalog::cascade::cascade_for_managed_paths(
        context.storage(),
        game_id,
        &owned_paths,
    )?;
    let release_plans = record
        .managed_files()
        .iter()
        .map(|managed| {
            let path = Path::new(managed.path().as_str());
            let consumed = cascade
                .rollback_specs
                .iter()
                .any(|spec| spec.contains_path(path));
            crate::addons::luma::dlss::plan_release_binding(context, game_id, managed, consumed)
        })
        .collect::<Result<Vec<_>, _>>()?;

    let crate::catalog::cascade::CascadeResult {
        rollback_specs,
        next_components,
        mutation_paths,
        ..
    } = cascade;

    let targets = crate::addons::luma::mutation_targets::uninstall_targets(
        game_root,
        &record,
        mutation_paths,
    );
    let workset = targets.resolve_workset()?;

    let rolled_back_ids: Vec<_> = rollback_specs
        .iter()
        .map(|spec| spec.component_id().clone())
        .collect();

    Ok(UninstallPlan::Inactive(Box::new(InactiveUninstallPlan {
        apply: UninstallApply {
            record,
            rollback_specs,
            next_components,
            release_plans,
            rolled_back_ids,
        },
        workset,
    })))
}
