//! Pure domain model for RenderPilot.
//!
//! This crate must stay independent from UI frameworks, persistence adapters,
//! operating-system APIs, and detection implementation details.

mod addon;
mod catalog_package;
mod component;
pub mod dlss;
mod exe_graphics;
pub mod fsr;
mod game;
mod ids;
mod install_root;
mod model;
pub mod mutation_features;
pub mod openvr;
mod package_version;
mod path;
mod text;
mod version;
pub mod xiph;

pub use addon::{
    ArtifactSlot, CleanupState, ControlNamespaceBinding, CoordinatedPeerOperation,
    CreateDirectoryEffect, CreateDirectoryState, DeleteEffect, DeleteState, DurableObservation,
    Endpoint, EngineConfigContribution, EngineConfigJournal, EngineConfigJournalError,
    EngineConfigReceipt, EngineConfigTransition, ExactOptiConfigProjection, ExpectedAfter,
    FileOwnership, FileReceipt, GameProxyTopology, InstalledAddon, InstalledAddonHostKind,
    InstalledAddonInvariantError, InstalledAddonParts, LumaInstallState, ManagedAddonFile,
    ManagedFileBaseline, ManagedFileMode, MaterializationState, NamespaceCapability,
    OperationEffect, OperationEndpoint, OperationRecord, OptiConfigOperation,
    OptiScalerAdoptionState, OptiScalerCleanupLifecycle, OptiScalerConfigAuthority,
    OptiScalerConfigCapability, OptiScalerConfigurationBaseline, OptiScalerDirectoryReceipt,
    OptiScalerFileBaseline, OptiScalerFileCleanup, OptiScalerFileReceipt, OptiScalerFileRole,
    OptiScalerInstallState, OptiScalerInstallStateParts, OptiScalerJournal, OptiScalerJournalError,
    OptiScalerJournalKind, OptiScalerModuleRuntimeBinding, OptiScalerPrerequisiteBinding,
    OptiScalerReleaseFileBaseline, OptiScalerStateError, OptiScalerTransitionDirection,
    PeerCatalogDeletedBaseline, PeerCatalogPhysicalContract, PeerCatalogRollbackClaim,
    PeerEndpointEvidence, PeerEndpointIntent, PeerEndpointOperation, PeerEndpointRole,
    PeerFileImage, PeerReadGuardEvidence, PeerReadGuardExpectation, PeerReadGuardRequirement,
    PeerReadGuardSource, PeerReusedClaimMembershipContract, PeerTransitionAuthorities,
    PeerTransitionContext, PeerTransitionContract, PeerTransitionError, PlannedGameProxyTopology,
    Preimage, PrivateArtifactSlots, PrivateWorkspaceBinding, ProxyImplementation, ProxyLink,
    ProxyPeerRoute, ProxyRootPrestate, ProxyTopologyError, RENODX_DLSS_FIX_INSTALL,
    RENODX_DLSS_FIX_UNINSTALL, RENODX_DLSS_FIX_UPDATE, RENODX_INSTALL, RENODX_INSTALL_FROM_FILE,
    RENODX_UNINSTALL, RelocateEffect, RelocateState, RemoveDirectoryEffect, RemoveDirectoryState,
    RenoDxConfigReceipt, RenoDxDlssBeforeImage, RenoDxDlssClaim, RenoDxDlssProjection,
    RenoDxHostKind, RenoDxInstallState, RenoDxReshadeIniAuthority, RenoDxReshadeIniFeature,
    RenoDxSetPathBaseline, RenoDxSetPathValue, SharedArtifactKind, SharedArtifactOrigin,
    SharedArtifactRecord, SharedArtifactSource, ThreatModel, TrackedSource, TrackedSourceRole,
    VerifyEffect, VerifyState, WriteEffect, WriteState, from_new_adoption, from_new_install,
    from_persisted, managed_sidecar_path, required_read_guards, required_read_guards_with_catalog,
    required_read_guards_with_renodx_reshade_ini,
    required_read_guards_with_renodx_reshade_ini_and_dlss,
    required_read_guards_with_renodx_reshade_ini_and_optiscaler_config, validate_evidence,
    validate_intents, validate_intents_with_authorities, validate_intents_with_renodx_reshade_ini,
    validate_optiscaler_cleanup_artifact, validate_optiscaler_cleanup_overlay,
    validate_optiscaler_effect_transition, validate_optiscaler_operation_transition,
    validate_peer_metadata_only, validate_read_guards,
};
pub use catalog_package::{
    CatalogLegalDocumentFormat, CatalogLegalDocumentKind, CatalogLegalDocumentReceipt,
    CatalogPackageAvailability, CatalogPackageMemberReceipt, CatalogPackageProvenanceReceipt,
    CatalogPackageReceipt, CatalogPackageReceiptV1, CatalogPackageReceiptV2,
    CatalogProvenanceReceipt, CatalogReceiptSchemaV1, CatalogReceiptSchemaV2,
    CatalogSignatureReceipt, CatalogSourceBuildToolchainReceipt, CatalogSourcePatchReceipt,
    CatalogSourceReceipt, CatalogTargetReceipt, PackageRelease, ReleaseChannel,
};
pub use component::{
    ArtifactMetadata, ArtifactTrustLevel, ComponentError, ComponentFile, ComponentRollbackBaseline,
    ComponentVersionReport, D3d12ExecutableBaseline, D3d12ExecutableIdentity, LibraryArtifact,
    LibraryComponent, PeCompatibilityProfile, PeExportSet, PeExportSetError, PeImportProfile,
    PeImportSet, PeImportSetError, ReleaseMetadata, RuntimeCompatibility, RuntimeTarget,
    Sha256Hash, UpstreamPackage, UpstreamPackageProvider, component_version_report,
};
pub use exe_graphics::ExeGraphicsInfo;
pub use game::{GameIdentity, GameInstallation, GameModelError, RootAuthority};
pub use ids::{ArtifactId, ComponentId, GameId, IdentifierError, OperationId};
pub use install_root::{InstallKey, InstallRoot};
pub use model::{
    AddonKind, Architecture, ComponentKind, GameRuntime, GraphicsApi, Launcher, LibraryTechnology,
    Platform, Swappability,
};
pub use package_version::{PackageVersion, PackageVersionParseError};
pub use path::{
    PathRef, PathRefError,
    capability::CapabilityToken,
    capability::CapabilityTokenError,
    durable_wire,
    durable_wire::DurablePathWireError,
    normalized_path_key,
    relation::{NormalizedPathRelation, normalized_path_relation},
};
pub use version::{Version, VersionParseError};

/// Human-readable product name used across user-facing entry points.
pub const APP_NAME: &str = "RenderPilot";
