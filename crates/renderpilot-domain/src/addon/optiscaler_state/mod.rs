//! Domain-owned OptiScaler lifecycle state.
//!
//! OptiScaler is not represented by [`super::InstalledAddon`]. It owns a
//! proxy slot, a release directory, and private receipts whose cleanup must
//! remain coupled to the proxy topology.

use std::{error::Error, fmt};

use serde::{Deserialize, Serialize};

use crate::{ComponentId, GameId, PathRef, Sha256Hash, normalized_path_key};

mod baseline;
mod factories;
mod parts;

pub use baseline::OptiScalerConfigurationBaseline;
pub use factories::{from_new_adoption, from_new_install, from_persisted};
pub use parts::OptiScalerInstallStateParts;

/// Ownership recorded for an exact OptiScaler file receipt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileOwnership {
    /// The operation wrote the file and may remove or restore it subject to
    /// the recorded preimage.
    Owned,
    /// The operation accepted an already suitable file without writing it.
    Reused,
}

/// Immutable identity, content, and custody evidence for a file participating
/// in the OptiScaler aggregate.
///
/// The receipt is deliberately unversioned: its shape is the domain contract,
/// while schema and transaction revisions belong to their respective
/// persistence/protocol envelopes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FileReceipt {
    /// Platform-native stable file identity.
    identity: String,
    /// SHA-256 digest of the exact bytes observed with that identity.
    digest: Sha256Hash,
    /// Whether the operation wrote or reused this file.
    ownership: FileOwnership,
}

impl FileReceipt {
    /// Constructs evidence for a file written by the operation.
    pub fn owned(
        identity: impl Into<String>,
        digest: Sha256Hash,
    ) -> Result<Self, OptiScalerStateError> {
        Self::checked(identity.into(), digest, FileOwnership::Owned)
    }

    /// Constructs evidence for a file accepted without writing it.
    pub fn reused(
        identity: impl Into<String>,
        digest: Sha256Hash,
    ) -> Result<Self, OptiScalerStateError> {
        Self::checked(identity.into(), digest, FileOwnership::Reused)
    }

    fn checked(
        identity: String,
        digest: Sha256Hash,
        ownership: FileOwnership,
    ) -> Result<Self, OptiScalerStateError> {
        let receipt = Self {
            identity,
            digest,
            ownership,
        };
        receipt.validate().map(|()| receipt)
    }

    /// Returns the digest carried by this evidence.
    #[must_use]
    pub const fn digest(&self) -> &Sha256Hash {
        &self.digest
    }

    /// Returns the recorded ownership.
    #[must_use]
    pub fn ownership(&self) -> FileOwnership {
        self.ownership
    }

    /// Returns whether this receipt is strong enough to authorize destructive
    /// cleanup of the recorded file.
    #[must_use]
    pub fn authorizes_destructive_cleanup(&self) -> bool {
        self.ownership == FileOwnership::Owned
    }

    /// Returns the stable identity.
    #[must_use]
    pub fn identity(&self) -> &str {
        &self.identity
    }

    /// Validates the non-filesystem invariants carried by the receipt.
    pub fn validate(&self) -> Result<(), OptiScalerStateError> {
        if self.identity.trim().is_empty() {
            return Err(OptiScalerStateError::EmptyField("file receipt identity"));
        }
        Ok(())
    }
}

impl<'de> Deserialize<'de> for FileReceipt {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            identity: String,
            digest: Sha256Hash,
            ownership: FileOwnership,
        }

        let wire = Wire::deserialize(deserializer)?;
        Self::checked(wire.identity, wire.digest, wire.ownership).map_err(serde::de::Error::custom)
    }
}

/// Exact pre-first-write baseline for an OptiScaler file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum OptiScalerFileBaseline {
    /// The path was absent before the operation.
    Absent,
    /// The path existed and must be preserved as recorded.
    Present {
        /// Immutable evidence for the pre-operation file.
        receipt: FileReceipt,
    },
}

impl OptiScalerFileBaseline {
    /// Validates the receipt, without touching the filesystem.
    pub fn validate(&self) -> Result<(), OptiScalerStateError> {
        if let Self::Present { receipt } = self {
            receipt.validate()?;
        }
        Ok(())
    }
}

/// How RenderPilot acquired responsibility for an OptiScaler installation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OptiScalerAdoptionState {
    /// Installed from a RenderPilot manifest and durable transaction.
    Managed,
    /// Adopted after every release-owned file matched immutable evidence.
    AdoptedExact,
    /// Taken over while preserving unknown baselines for conservative cleanup.
    TakenOver,
}

/// A prerequisite accepted for the exact compatibility variant that produced
/// this managed OptiScaler installation.
///
/// This is deliberately installation state, rather than a live catalogue
/// decision: lifecycle operations must not silently change an already
/// accepted dependency when the catalogue later changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OptiScalerPrerequisiteBinding {
    /// The installed variant has no add-on prerequisite.
    None,
    /// The installed variant requires Luma to remain installed.
    Luma,
}

impl OptiScalerPrerequisiteBinding {
    /// Returns the exact persisted representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Luma => "luma",
        }
    }
}

impl std::str::FromStr for OptiScalerPrerequisiteBinding {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "none" => Ok(Self::None),
            "luma" => Ok(Self::Luma),
            _ => Err(()),
        }
    }
}

impl OptiScalerAdoptionState {
    /// Returns the stable wire/storage value.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Managed => "managed",
            Self::AdoptedExact => "adopted_exact",
            Self::TakenOver => "taken_over",
        }
    }
}

impl std::str::FromStr for OptiScalerAdoptionState {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "managed" => Ok(Self::Managed),
            "adopted_exact" => Ok(Self::AdoptedExact),
            "taken_over" => Ok(Self::TakenOver),
            _ => Err(()),
        }
    }
}

/// Lifecycle role of a release-owned OptiScaler file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OptiScalerFileRole {
    /// `OptiScaler.ini`: a present user baseline is retained by content and
    /// restored after preserving the current configuration, while a reused
    /// live file remains untouched.
    Configuration,
    /// A release-bundled runtime file with an absent baseline.
    Runtime,
}

/// Cleanup policy captured with a release-owned file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OptiScalerFileCleanup {
    /// Remove only while the file still has the recorded installed hash.
    RemoveIfUnchanged,
    /// Preserve the current configuration before restoring its present baseline.
    PreserveCurrentThenRestoreBaseline,
    /// Leave a reused pre-existing configuration untouched.
    PreserveUnchanged,
}

/// Immutable original-file custody retained when OptiScaler replaces a game's
/// supported AMD FSR entry point.
///
/// This is not installation adoption.  `RetainedFsrEntryPoint` records the
/// pre-existing game DLL solely so removal can restore that exact DLL from its
/// deterministic same-directory custody sidecar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum OptiScalerReleaseFileBaseline {
    /// No external runtime occupied this release target before OptiScaler.
    Absent,
    /// Exact original AMD FSR entry point relocated beside its active target.
    RetainedFsrEntryPoint {
        /// Stable detected-component identity that authorized acquisition.
        component_id: ComponentId,
        /// Deterministic sibling custody location for the original DLL.
        custody_path: PathRef,
        /// Exact external DLL evidence retained in custody.
        original: FileReceipt,
    },
}

impl OptiScalerReleaseFileBaseline {
    /// Returns the no-custody baseline.
    #[must_use]
    pub const fn absent() -> Self {
        Self::Absent
    }

    fn validate_for(
        &self,
        receipt: &OptiScalerFileReceipt,
        target_dir: &PathRef,
    ) -> Result<(), OptiScalerStateError> {
        match self {
            Self::Absent => Ok(()),
            Self::RetainedFsrEntryPoint {
                component_id: _,
                custody_path,
                original,
            } => {
                if receipt.role != OptiScalerFileRole::Runtime
                    || receipt.cleanup != OptiScalerFileCleanup::RemoveIfUnchanged
                    || receipt.installed.ownership() != FileOwnership::Owned
                {
                    return Err(OptiScalerStateError::RetainedFsrBaselineRole);
                }
                let target_name = std::path::Path::new(receipt.path.as_str())
                    .file_name()
                    .and_then(|name| name.to_str())
                    .ok_or(OptiScalerStateError::RetainedFsrBaselinePath)?;
                if !crate::fsr::is_entry_point(target_name) {
                    return Err(OptiScalerStateError::RetainedFsrBaselinePath);
                }
                validate_file_path(custody_path, target_dir)?;
                let expected_custody = format!(".{target_name}.renderpilot-optiscaler-original");
                let custody_name = std::path::Path::new(custody_path.as_str())
                    .file_name()
                    .and_then(|name| name.to_str());
                if custody_name != Some(expected_custody.as_str())
                    || std::path::Path::new(custody_path.as_str()).parent()
                        != std::path::Path::new(receipt.path.as_str()).parent()
                {
                    return Err(OptiScalerStateError::RetainedFsrBaselinePath);
                }
                original.validate()?;
                if original.ownership() != FileOwnership::Reused {
                    return Err(OptiScalerStateError::RetainedFsrBaselineOwnership);
                }
                Ok(())
            }
        }
    }

    /// Returns retained original FSR evidence, when this release target owns a
    /// sidecar custody obligation.
    #[must_use]
    pub fn retained_original(&self) -> Option<(&ComponentId, &PathRef, &FileReceipt)> {
        let Self::RetainedFsrEntryPoint {
            component_id,
            custody_path,
            original,
        } = self
        else {
            return None;
        };
        Some((component_id, custody_path, original))
    }
}

/// Immutable uninstall evidence for one private release-owned file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OptiScalerFileReceipt {
    /// Exact path of the release-owned file.
    pub path: PathRef,
    /// Immutable post-operation file evidence.
    pub installed: FileReceipt,
    /// Cleanup policy role for uninstall and recovery.
    pub role: OptiScalerFileRole,
    /// Explicit removal policy.
    pub cleanup: OptiScalerFileCleanup,
    /// Original-game custody, if this Runtime replaced a supported FSR entry
    /// point.  The field is mandatory and always serialized so an unshipped
    /// state cannot silently lose restore authority.
    pub baseline: OptiScalerReleaseFileBaseline,
}

/// Exact runtime placement for one selected module.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OptiScalerModuleRuntimeBinding {
    /// Stable module identifier from the validated manifest.
    pub module: String,
    /// Runtime file path controlled by the module.
    pub path: PathRef,
    /// Immutable post-operation file evidence.
    pub installed: FileReceipt,
    /// Exact pre-first-write baseline used by cleanup and recovery.
    pub baseline: OptiScalerFileBaseline,
}

/// Receipt for a private directory created by an OptiScaler mutation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OptiScalerDirectoryReceipt {
    /// Exact created directory path.
    pub path: PathRef,
    /// Platform-native identity captured after creation.
    pub identity: String,
}

/// Tool-specific state kept separate from generic add-on receipts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OptiScalerInstallState {
    /// Owning game.
    pub game_id: GameId,
    /// Immutable manifest release identifier.
    pub release_id: String,
    /// Compatibility/module manifest revision used for the operation.
    pub manifest_revision: String,
    /// Exact archive digest when the release source is available.
    #[serde(default)]
    pub archive_sha256: Option<Sha256Hash>,
    /// Canonical source locator for the selected release, when known.
    #[serde(default)]
    pub source: Option<String>,
    /// Executable selected at install time.
    pub target_exe_path: PathRef,
    /// Directory actually receiving runtime files.
    pub target_dir: PathRef,
    /// Stable selected module identifiers after dependency closure.
    pub modules: Vec<String>,
    /// Non-proxy release-owned files and their effective post-install hashes.
    pub release_files: Vec<OptiScalerFileReceipt>,
    /// Exact module-to-runtime path bindings.
    pub runtime_bindings: Vec<OptiScalerModuleRuntimeBinding>,
    /// Private directories created by the release transaction.
    pub directory_receipts: Vec<OptiScalerDirectoryReceipt>,
    /// Associated root-proxy topology aggregate.
    pub proxy_topology_id: Option<String>,
    /// Installed INI semantic schema version.
    pub config_schema: u32,
    /// Release whose defaults are the three-way merge base.
    pub config_base_release: String,
    /// Provenance/adoption classification.
    pub adoption_state: OptiScalerAdoptionState,
    /// Exact prerequisite accepted when this installation was created.
    pub prerequisite_binding: OptiScalerPrerequisiteBinding,
    /// Persistence creation time, in Unix epoch milliseconds.
    pub created_at: Option<i64>,
    /// Persistence update time, in Unix epoch milliseconds.
    pub updated_at: Option<i64>,
    /// Immutable pre-first-write configuration baseline.
    pub(super) configuration_baseline: OptiScalerConfigurationBaseline,
}

impl OptiScalerInstallState {
    /// Validates path, ordering, ownership, and timestamp invariants.
    pub fn validate(&self) -> Result<(), OptiScalerStateError> {
        require_text("release_id", &self.release_id)?;
        require_text("manifest_revision", &self.manifest_revision)?;
        if let Some(source) = &self.source {
            require_text("source", source)?;
        }
        if self.archive_sha256.is_some() != self.source.is_some() {
            return Err(OptiScalerStateError::InvalidField(
                "archive_sha256 and source must be provided together",
            ));
        }
        require_text("config_base_release", &self.config_base_release)?;
        if self.config_schema == 0 {
            return Err(OptiScalerStateError::InvalidField(
                "config_schema must be greater than zero",
            ));
        }
        if self.modules.is_empty() {
            return Err(OptiScalerStateError::InvalidField(
                "modules must not be empty",
            ));
        }
        validate_sorted_unique_text("modules", &self.modules)?;
        validate_under_target("target_exe_path", &self.target_exe_path, &self.target_dir)?;

        let mut paths = std::collections::BTreeSet::new();
        let mut configuration_receipts = self
            .release_files
            .iter()
            .filter(|receipt| receipt.role == OptiScalerFileRole::Configuration);
        let Some(configuration) = configuration_receipts.next() else {
            return Err(OptiScalerStateError::ConfigurationReceiptCardinality);
        };
        if configuration_receipts.next().is_some() {
            return Err(OptiScalerStateError::ConfigurationReceiptCardinality);
        }
        let expected_configuration_path = normalized_path_key(&format!(
            "{}/OptiScaler.ini",
            self.target_dir.as_str().trim_end_matches('/')
        ));
        if normalized_path_key(configuration.path.as_str()) != expected_configuration_path {
            return Err(OptiScalerStateError::ConfigurationReceiptPath(
                configuration.path.clone(),
            ));
        }
        for receipt in &self.release_files {
            receipt.installed.validate()?;
            validate_file_path(&receipt.path, &self.target_dir)?;
            receipt.baseline.validate_for(receipt, &self.target_dir)?;
            if !paths.insert(normalized_path_key(receipt.path.as_str())) {
                return Err(OptiScalerStateError::DuplicatePath(receipt.path.clone()));
            }
            if let Some((_, custody_path, _)) = receipt.baseline.retained_original()
                && !paths.insert(normalized_path_key(custody_path.as_str()))
            {
                return Err(OptiScalerStateError::DuplicatePath(custody_path.clone()));
            }
        }

        let mut binding_modules = std::collections::BTreeSet::new();
        for binding in &self.runtime_bindings {
            require_text("runtime_bindings.module", &binding.module)?;
            if self.modules.binary_search(&binding.module).is_err() {
                return Err(OptiScalerStateError::UnknownModule(binding.module.clone()));
            }
            validate_file_path(&binding.path, &self.target_dir)?;
            if !binding_modules.insert(binding.module.as_str()) {
                return Err(OptiScalerStateError::DuplicateModule(
                    binding.module.clone(),
                ));
            }
            binding.installed.validate()?;
            binding.baseline.validate()?;
            validate_runtime_binding_baseline(binding)?;
            if !paths.insert(normalized_path_key(binding.path.as_str())) {
                return Err(OptiScalerStateError::DuplicatePath(binding.path.clone()));
            }
        }

        let mut directories = std::collections::BTreeSet::new();
        for receipt in &self.directory_receipts {
            require_text("directory_receipts.identity", &receipt.identity)?;
            validate_under_target("directory_receipts.path", &receipt.path, &self.target_dir)?;
            if !directories.insert(normalized_path_key(receipt.path.as_str())) {
                return Err(OptiScalerStateError::DuplicatePath(receipt.path.clone()));
            }
        }

        let Some(topology_id) = &self.proxy_topology_id else {
            return Err(OptiScalerStateError::InvalidField(
                "proxy_topology_id is required",
            ));
        };
        require_text("proxy_topology_id", topology_id)?;
        validate_timestamp("created_at", self.created_at)?;
        validate_timestamp("updated_at", self.updated_at)?;
        if let (Some(created), Some(updated)) = (self.created_at, self.updated_at)
            && updated < created
        {
            return Err(OptiScalerStateError::InvalidField(
                "updated_at must not precede created_at",
            ));
        }
        self.configuration_baseline.validate()?;
        Ok(())
    }

    /// Returns the immutable configuration preimage captured before the first
    /// OptiScaler write.
    #[must_use]
    pub fn configuration_baseline(&self) -> &OptiScalerConfigurationBaseline {
        &self.configuration_baseline
    }

    /// Returns the one canonical Configuration receipt.
    pub fn configuration_receipt(&self) -> Result<&OptiScalerFileReceipt, OptiScalerStateError> {
        configuration_receipt(self)
    }

    /// Rebuilds this state with exactly one new Configuration receipt while
    /// retaining its immutable baseline and every unrelated field. This is
    /// the only successor shape admitted for a peer-coordinated
    /// `Plugins.LoadReshade` update.
    pub fn with_configuration_receipt(
        &self,
        receipt: &OptiScalerFileReceipt,
    ) -> Result<Self, OptiScalerStateError> {
        if receipt.role != OptiScalerFileRole::Configuration {
            return Err(OptiScalerStateError::ConfigurationReceiptCardinality);
        }
        let mut parts = OptiScalerInstallStateParts::from(self);
        let mut replaced = false;
        for existing in &mut parts.release_files {
            if existing.role == OptiScalerFileRole::Configuration {
                *existing = receipt.clone();
                replaced = true;
            }
        }
        if !replaced {
            return Err(OptiScalerStateError::ConfigurationReceiptCardinality);
        }
        Self::from_existing(self, parts)
    }

    /// Compares the complete durable state while ignoring database-managed
    /// persistence timestamps.
    ///
    /// The exhaustive destructuring is intentional: adding a new field to the
    /// state must make this comparison fail to compile until that field is
    /// classified explicitly.
    #[must_use]
    pub fn eq_ignoring_persistence_timestamps(&self, other: &Self) -> bool {
        let Self {
            game_id,
            release_id,
            manifest_revision,
            archive_sha256,
            source,
            target_exe_path,
            target_dir,
            modules,
            release_files,
            runtime_bindings,
            directory_receipts,
            proxy_topology_id,
            config_schema,
            config_base_release,
            adoption_state,
            prerequisite_binding,
            created_at: _,
            updated_at: _,
            configuration_baseline,
        } = self;
        let Self {
            game_id: other_game_id,
            release_id: other_release_id,
            manifest_revision: other_manifest_revision,
            archive_sha256: other_archive_sha256,
            source: other_source,
            target_exe_path: other_target_exe_path,
            target_dir: other_target_dir,
            modules: other_modules,
            release_files: other_release_files,
            runtime_bindings: other_runtime_bindings,
            directory_receipts: other_directory_receipts,
            proxy_topology_id: other_proxy_topology_id,
            config_schema: other_config_schema,
            config_base_release: other_config_base_release,
            adoption_state: other_adoption_state,
            prerequisite_binding: other_prerequisite_binding,
            created_at: _,
            updated_at: _,
            configuration_baseline: other_configuration_baseline,
        } = other;

        game_id == other_game_id
            && release_id == other_release_id
            && manifest_revision == other_manifest_revision
            && archive_sha256 == other_archive_sha256
            && source == other_source
            && target_exe_path == other_target_exe_path
            && target_dir == other_target_dir
            && modules == other_modules
            && release_files == other_release_files
            && runtime_bindings == other_runtime_bindings
            && directory_receipts == other_directory_receipts
            && proxy_topology_id == other_proxy_topology_id
            && config_schema == other_config_schema
            && config_base_release == other_config_base_release
            && adoption_state == other_adoption_state
            && prerequisite_binding == other_prerequisite_binding
            && configuration_baseline == other_configuration_baseline
    }

    /// Creates the next state while retaining the original configuration
    /// baseline. The caller cannot rebase an existing installation.
    pub fn from_existing(
        previous: &Self,
        parts: OptiScalerInstallStateParts,
    ) -> Result<Self, OptiScalerStateError> {
        factories::from_existing(previous, parts)
    }

    /// Creates the first Owned successor for an exact Reused configuration.
    /// The new baseline must be the user bytes observed immediately before
    /// that first write, or the immutable baseline retained for an exact
    /// absent-file repair.
    pub fn from_existing_with_configuration_acquisition(
        previous: &Self,
        parts: OptiScalerInstallStateParts,
        configuration_baseline: OptiScalerConfigurationBaseline,
    ) -> Result<Self, OptiScalerStateError> {
        factories::from_existing_with_configuration_acquisition(
            previous,
            parts,
            configuration_baseline,
        )
    }

    /// Creates the first Owned successor at a new canonical configuration
    /// path, while leaving the former adopted Reused configuration untouched.
    pub fn from_existing_with_configuration_retarget(
        previous: &Self,
        parts: OptiScalerInstallStateParts,
    ) -> Result<Self, OptiScalerStateError> {
        factories::from_existing_with_configuration_retarget(previous, parts)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum OptiScalerLifecycleContext {
    NewInstall,
    NewAdoption,
    Existing,
}

fn configuration_receipt(
    state: &OptiScalerInstallState,
) -> Result<&OptiScalerFileReceipt, OptiScalerStateError> {
    state
        .release_files
        .iter()
        .find(|receipt| receipt.role == OptiScalerFileRole::Configuration)
        .ok_or(OptiScalerStateError::ConfigurationReceiptCardinality)
}

/// Validates the complete configuration lifecycle matrix at one domain boundary.
pub(super) fn validate_configuration_lifecycle(
    state: &OptiScalerInstallState,
    context: OptiScalerLifecycleContext,
) -> Result<(), OptiScalerStateError> {
    let configuration = configuration_receipt(state)?;
    let baseline = state.configuration_baseline();

    let canonical_install = match (
        baseline,
        configuration.installed.ownership(),
        configuration.cleanup,
    ) {
        (
            OptiScalerConfigurationBaseline::Absent,
            FileOwnership::Owned,
            OptiScalerFileCleanup::RemoveIfUnchanged,
        ) => true,
        (
            OptiScalerConfigurationBaseline::Present { receipt, .. },
            FileOwnership::Owned,
            OptiScalerFileCleanup::PreserveCurrentThenRestoreBaseline,
        ) => {
            receipt.ownership() == FileOwnership::Reused
                && (matches!(context, OptiScalerLifecycleContext::Existing)
                    || receipt.identity() == configuration.installed.identity())
        }
        (
            OptiScalerConfigurationBaseline::Present { receipt, .. },
            FileOwnership::Reused,
            OptiScalerFileCleanup::PreserveUnchanged,
        ) => receipt == &configuration.installed,
        _ => false,
    };

    if !canonical_install {
        return Err(OptiScalerStateError::ConfigurationBaselineTransition(
            "configuration lifecycle tuple is not canonical",
        ));
    }

    if matches!(context, OptiScalerLifecycleContext::NewAdoption)
        && !matches!(
            (
                baseline,
                configuration.installed.ownership(),
                configuration.cleanup,
            ),
            (
                OptiScalerConfigurationBaseline::Present { receipt, .. },
                FileOwnership::Reused,
                OptiScalerFileCleanup::PreserveUnchanged,
            ) if receipt == &configuration.installed
        )
    {
        return Err(OptiScalerStateError::ConfigurationBaselineTransition(
            "adoption requires a present reused configuration with an exact receipt",
        ));
    }

    Ok(())
}

/// Invalid typed OptiScaler aggregate data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OptiScalerStateError {
    /// A required string was empty.
    EmptyField(&'static str),
    /// A scalar invariant was violated.
    InvalidField(&'static str),
    /// A path occurred more than once.
    DuplicatePath(PathRef),
    /// A module occurred more than once or lacked a unique binding.
    DuplicateModule(String),
    /// A runtime binding references a module not selected in the state.
    UnknownModule(String),
    /// An operation-owned runtime file may only have an absent pre-operation
    /// baseline.  Keeping a present baseline would make the same receipt
    /// simultaneously claim pre-existing bytes and destructive ownership.
    OwnedRuntimeBindingHasPresentBaseline {
        /// Stable selected module identifier.
        module: String,
    },
    /// An exact reused runtime binding omitted its exact pre-operation file.
    ReusedRuntimeBindingHasAbsentBaseline(String),
    /// An exact reused runtime binding used an owned baseline, which cannot
    /// be promoted into reused custody.
    ReusedRuntimeBindingOwnedBaseline(String),
    /// An exact reused runtime binding baseline differs from its installed file.
    ReusedRuntimeBindingBaselineMismatch(String),
    /// A path escaped the persisted game target directory.
    /// The named path escapes the install target directory.
    PathOutsideTarget {
        /// The state field containing the path.
        field: &'static str,
        /// The offending path.
        path: PathRef,
    },
    /// The configuration baseline receipt must represent reused user data.
    ConfigurationBaselineNotReused,
    /// The retained configuration bytes do not match their declared length.
    ConfigurationBaselineLengthMismatch,
    /// The retained configuration bytes exceed the bounded storage limit.
    ConfigurationBaselineOversize,
    /// The retained configuration bytes do not match the receipt digest.
    ConfigurationBaselineDigestMismatch,
    /// The configuration receipt is missing or not unique.
    ConfigurationReceiptCardinality,
    /// The configuration receipt is not exactly target_dir/OptiScaler.ini.
    ConfigurationReceiptPath(PathRef),
    /// Retained FSR custody is only valid for an owned runtime replacement
    /// with the ordinary exact-removal policy.
    RetainedFsrBaselineRole,
    /// Retained FSR custody is not the deterministic sibling of its target.
    RetainedFsrBaselinePath,
    /// A retained FSR original must remain external (`Reused`) custody.
    RetainedFsrBaselineOwnership,
    /// A newly installed configuration has an invalid ownership/baseline pair.
    ConfigurationBaselineTransition(&'static str),
    /// An existing managed installation cannot gain or lose a prerequisite
    /// without a new installation/adoption decision.
    PrerequisiteBindingTransition,
}

impl fmt::Display for OptiScalerStateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyField(field) => write!(formatter, "{field} must not be empty"),
            Self::InvalidField(message) => formatter.write_str(message),
            Self::DuplicatePath(path) => write!(formatter, "duplicate OptiScaler path: {path}"),
            Self::DuplicateModule(module) => {
                write!(formatter, "duplicate OptiScaler module binding: {module}")
            }
            Self::UnknownModule(module) => write!(formatter, "unknown OptiScaler module: {module}"),
            Self::OwnedRuntimeBindingHasPresentBaseline { module } => write!(
                formatter,
                "owned runtime binding has a present baseline: {module}"
            ),
            Self::ReusedRuntimeBindingHasAbsentBaseline(module) => write!(
                formatter,
                "reused runtime binding has an absent baseline: {module}"
            ),
            Self::ReusedRuntimeBindingOwnedBaseline(module) => write!(
                formatter,
                "reused runtime binding has an owned baseline: {module}"
            ),
            Self::ReusedRuntimeBindingBaselineMismatch(module) => write!(
                formatter,
                "reused runtime binding baseline differs from its installed receipt: {module}"
            ),
            Self::PathOutsideTarget { field, path } => {
                write!(formatter, "{field} escapes the OptiScaler target: {path}")
            }
            Self::ConfigurationBaselineNotReused => {
                formatter.write_str("configuration baseline receipt must be reused")
            }
            Self::ConfigurationBaselineLengthMismatch => {
                formatter.write_str("configuration baseline length does not match bytes")
            }
            Self::ConfigurationBaselineOversize => {
                formatter.write_str("configuration baseline exceeds the 16 MiB limit")
            }
            Self::ConfigurationBaselineDigestMismatch => {
                formatter.write_str("configuration baseline digest does not match bytes")
            }
            Self::ConfigurationReceiptCardinality => {
                formatter.write_str("state must contain exactly one configuration receipt")
            }
            Self::ConfigurationReceiptPath(path) => {
                write!(
                    formatter,
                    "invalid OptiScaler configuration receipt path: {path}"
                )
            }
            Self::ConfigurationBaselineTransition(reason) => formatter.write_str(reason),
            Self::PrerequisiteBindingTransition => {
                formatter.write_str("existing OptiScaler prerequisite binding cannot change")
            }
            Self::RetainedFsrBaselineRole => formatter.write_str(
                "retained FSR baseline requires an owned runtime replacement with exact removal",
            ),
            Self::RetainedFsrBaselinePath => {
                formatter.write_str("retained FSR baseline is not the deterministic target sibling")
            }
            Self::RetainedFsrBaselineOwnership => {
                formatter.write_str("retained FSR original receipt must be reused custody")
            }
        }
    }
}

impl Error for OptiScalerStateError {}

fn validate_runtime_binding_baseline(
    binding: &OptiScalerModuleRuntimeBinding,
) -> Result<(), OptiScalerStateError> {
    match binding.installed.ownership() {
        FileOwnership::Owned => {
            if matches!(binding.baseline, OptiScalerFileBaseline::Present { .. }) {
                return Err(
                    OptiScalerStateError::OwnedRuntimeBindingHasPresentBaseline {
                        module: binding.module.clone(),
                    },
                );
            }
        }
        FileOwnership::Reused => {
            let OptiScalerFileBaseline::Present { receipt } = &binding.baseline else {
                return Err(OptiScalerStateError::ReusedRuntimeBindingHasAbsentBaseline(
                    binding.module.clone(),
                ));
            };
            if receipt.ownership() != FileOwnership::Reused {
                return Err(OptiScalerStateError::ReusedRuntimeBindingOwnedBaseline(
                    binding.module.clone(),
                ));
            }
            if binding.installed.identity() != receipt.identity()
                || binding.installed.digest() != receipt.digest()
            {
                return Err(OptiScalerStateError::ReusedRuntimeBindingBaselineMismatch(
                    binding.module.clone(),
                ));
            }
        }
    }
    Ok(())
}

fn require_text(field: &'static str, value: &str) -> Result<(), OptiScalerStateError> {
    if value.trim().is_empty() {
        return Err(OptiScalerStateError::EmptyField(field));
    }
    Ok(())
}

fn validate_sorted_unique_text(
    field: &'static str,
    values: &[String],
) -> Result<(), OptiScalerStateError> {
    if values.windows(2).any(|window| window[0] >= window[1]) {
        return Err(OptiScalerStateError::InvalidField(field));
    }
    if values.iter().any(|value| value.trim().is_empty()) {
        return Err(OptiScalerStateError::EmptyField(field));
    }
    Ok(())
}

fn validate_timestamp(field: &'static str, value: Option<i64>) -> Result<(), OptiScalerStateError> {
    if value.is_some_and(|timestamp| timestamp < 0) {
        return Err(OptiScalerStateError::InvalidField(field));
    }
    Ok(())
}

fn validate_under_target(
    field: &'static str,
    path: &PathRef,
    target: &PathRef,
) -> Result<(), OptiScalerStateError> {
    let path_key = normalized_path_key(path.as_str());
    let target_key = normalized_path_key(target.as_str());
    if path_key == target_key
        || path_key
            .strip_prefix(&target_key)
            .is_some_and(|rest| rest.starts_with('/'))
    {
        Ok(())
    } else {
        Err(OptiScalerStateError::PathOutsideTarget {
            field,
            path: path.clone(),
        })
    }
}

fn validate_file_path(path: &PathRef, target: &PathRef) -> Result<(), OptiScalerStateError> {
    validate_under_target("release_files.path", path, target)?;
    if normalized_path_key(path.as_str()) == normalized_path_key(target.as_str()) {
        return Err(OptiScalerStateError::InvalidField(
            "release file path must not equal target directory",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod factory_tests;
