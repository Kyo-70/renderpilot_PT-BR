use super::*;

use crate::repositories::{
    AuthorityCas, BeginFileMutationPreparation, CatalogReadiness, CompleteScanWriteUnit,
    PendingFileMutationRow, PendingFileMutationState, SqliteStorage, pending_file_mutations,
};
use renderpilot_application::{
    ComponentRepository, GameRepository, InstalledAddonRepository, OptiScalerStateRepository,
    ProxyTopologyRepository,
};
use renderpilot_domain::{
    AddonKind, ComponentKind, ControlNamespaceBinding, DurableObservation, Endpoint, ExpectedAfter,
    FileReceipt, GameId, GameIdentity, GameInstallation, GameProxyTopology, GameRuntime,
    InstalledAddon, Launcher, LibraryComponent, LibraryTechnology, ManagedAddonFile,
    ManagedFileBaseline, MaterializationState, NamespaceCapability, OperationEffect,
    OperationEndpoint, OperationRecord, OptiScalerAdoptionState, OptiScalerDirectoryReceipt,
    OptiScalerFileBaseline, OptiScalerFileCleanup, OptiScalerFileReceipt, OptiScalerFileRole,
    OptiScalerInstallState, OptiScalerModuleRuntimeBinding, PathRef, Platform, Preimage,
    PrivateArtifactSlots, PrivateWorkspaceBinding, ProxyImplementation, ProxyLink, Sha256Hash,
    Swappability, WriteEffect, WriteState,
};

mod aggregate;
mod fixtures;
mod generic;
mod pre_catalog;

pub(super) use fixtures::*;
