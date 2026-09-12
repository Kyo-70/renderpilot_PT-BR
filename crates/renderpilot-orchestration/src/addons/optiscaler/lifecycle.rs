//! Install, update, module selection, repair, relocation, and safe uninstall.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use super::identity::{matches_optional as file_matches, path_ref};
use crate::file_mutation::optiscaler::PreparedFileMutation;
use crate::file_mutation::{MutationScope, MutationTarget, apply_snapshot_overrides};
use crate::net::ProgressObserver;
use crate::{Context, ServiceError, failed};
use renderpilot_application::{
    ArtifactRepository, ComponentRepository, GameRepository, InstalledAddonRepository,
    OptiScalerStateRepository, ProxyTopologyRepository,
};
use renderpilot_domain::{
    AddonKind, ComponentId, FileOwnership, FileReceipt, GameId, GameProxyTopology, InstalledAddon,
    LibraryArtifact, ManagedAddonFile, ManagedFileBaseline, ManagedFileMode,
    OptiScalerAdoptionState, OptiScalerFileBaseline, OptiScalerFileCleanup, OptiScalerFileReceipt,
    OptiScalerFileRole, OptiScalerInstallState, OptiScalerReleaseFileBaseline, PathRef,
    ProxyImplementation, ProxyLink, ProxyRootPrestate, Sha256Hash,
};

use super::archive::{PreparedArchive, read_verified_member, selected_members, validate_and_stage};
use super::config::{ConfigMergeResult, remove_managed_values, three_way_merge};
use super::evaluation::{EvaluatedAvailability, EvaluatedProxyPlan};
use super::matcher::{
    self, native_module_component, native_module_matches_path, native_module_spec,
    reconcile_modules_for_release, validate_module_selection,
};
use super::module_artifact::{PreparedModuleArtifact, prepare_selected};
use super::types::{
    ManagedIniValue, OptiScalerManifest, OptiScalerOperationResult, OptiScalerRelease,
    OptiScalerUpdateCheck,
};

mod adoption;
mod apply;
mod config;
mod download;
mod install;
mod native;
mod operations;
mod planning;
mod policy;
mod proxy;
mod queries;
mod relocate;
mod targets;
mod uninstall;
mod update;

#[cfg(test)]
mod tests;

use config::*;
use native::*;
use operations::*;
use planning::*;
use policy::*;
use proxy::*;
use targets::*;

pub(super) use adoption::adopt_exact;
use apply::{apply_release, ini_library_path, module_library_path, release_has_private_runtime};
use download::{download_and_stage, download_config_base};
pub(crate) use install::install;
pub(crate) use native::NativeModulePaths;
pub(crate) fn managed_state_endpoint_roots(
    state: &OptiScalerInstallState,
) -> Vec<(PathBuf, PathBuf)> {
    operations::managed_state_endpoint_roots(state)
}
pub(in crate::addons::optiscaler) use operations::preflight_peer_host_transition;
pub(super) use planning::AdoptionPolicy;
pub(crate) use planning::{
    InstallOptiScalerRequest, RelocateOptiScalerRequest, SetOptiScalerModulesRequest,
    UpdateOptiScalerRequest,
};
pub(in crate::addons::optiscaler) use policy::managed_bindings_from_state;
pub(crate) use policy::managed_config_invariants;
pub(crate) use queries::check_update;
pub(crate) use relocate::relocate;
pub(crate) use uninstall::{
    OptiScalerManagedCleanupFootprint, preflight_managed_cleanup_uninstall_locked, uninstall,
    uninstall_locked,
};
pub(crate) use update::{repair, set_modules, update};
