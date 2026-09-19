//! Active RenoDX installation authority and phase-one snapshots.
//!
//! This boundary seals all path/configuration facts before a download starts.
//! It intentionally contains no filesystem mutation and no storage integration;
//! later phases consume the immutable projections defined here.

pub(crate) mod active_dlss;
pub(crate) mod active_install;
pub(crate) mod active_uninstall;
pub(crate) mod active_update;
pub(crate) mod config;
pub(crate) mod effects;
pub(crate) mod host;
mod model;
pub(crate) mod record;
mod root_authority;
pub(crate) mod shared;

pub(crate) use active_dlss::{
    ActiveDlssComposition, ActiveDlssEffect, ActiveDlssEndpointInput, ActiveDlssError,
    ActiveDlssInput, compose_active_dlss,
};
pub(crate) use active_install::{
    PreparedActiveInstall, RenoDxActiveInstallError, compose_active_install,
};
pub(crate) use active_uninstall::{
    ActiveUninstallComposition, RenoDxActiveUninstallError, compose_active_uninstall,
    snapshot_active_uninstall,
};
pub(crate) use active_update::{
    RenoDxActiveUpdateComposition, RenoDxActiveUpdateConfigInput, RenoDxActiveUpdateHostInput,
    RenoDxActiveUpdateInput, compose_active_update,
};
pub(crate) use model::{
    InstallActiveSnapshot, InstallCommandVariant, RenoDxConfigSourceSeal, RenoDxRootSeal,
};
pub(crate) use root_authority::RenoDxRootAuthority;
pub(crate) use shared::{
    ActiveSharedMutationError, ActiveSharedMutationRequest, SharedLayerSource,
    execute_active_shared_mutation,
};
