use super::{
    OptiScalerAdoptionState, OptiScalerDirectoryReceipt, OptiScalerFileReceipt,
    OptiScalerInstallState, OptiScalerModuleRuntimeBinding, OptiScalerPrerequisiteBinding,
};
use crate::{GameId, PathRef, Sha256Hash};

/// Public fields used to construct or rehydrate an OptiScaler state.
///
/// The configuration baseline is intentionally absent from this bag of fields;
/// callers must provide it through one of the state factories so its custody
/// rules cannot be bypassed by a struct literal.
#[derive(Debug, Clone)]
pub struct OptiScalerInstallStateParts {
    /// Owning game.
    pub game_id: GameId,
    /// Immutable release identifier.
    pub release_id: String,
    /// Manifest revision used for the operation.
    pub manifest_revision: String,
    /// Optional archive digest paired with `source`.
    pub archive_sha256: Option<Sha256Hash>,
    /// Optional canonical release source paired with `archive_sha256`.
    pub source: Option<String>,
    /// Selected executable.
    pub target_exe_path: PathRef,
    /// Directory receiving the release files.
    pub target_dir: PathRef,
    /// Sorted selected module identifiers.
    pub modules: Vec<String>,
    /// Release-owned file receipts.
    pub release_files: Vec<OptiScalerFileReceipt>,
    /// Module runtime bindings.
    pub runtime_bindings: Vec<OptiScalerModuleRuntimeBinding>,
    /// Directories created by the release operation.
    pub directory_receipts: Vec<OptiScalerDirectoryReceipt>,
    /// Associated proxy topology identifier.
    pub proxy_topology_id: Option<String>,
    /// Installed configuration schema.
    pub config_schema: u32,
    /// Release used as the configuration merge base.
    pub config_base_release: String,
    /// Lifecycle provenance classification.
    pub adoption_state: OptiScalerAdoptionState,
    /// Exact prerequisite accepted when the installation was created.
    pub prerequisite_binding: OptiScalerPrerequisiteBinding,
    /// Storage creation timestamp, when rehydrated.
    pub created_at: Option<i64>,
    /// Storage update timestamp, when rehydrated.
    pub updated_at: Option<i64>,
}

impl From<&OptiScalerInstallState> for OptiScalerInstallStateParts {
    fn from(state: &OptiScalerInstallState) -> Self {
        Self {
            game_id: state.game_id.clone(),
            release_id: state.release_id.clone(),
            manifest_revision: state.manifest_revision.clone(),
            archive_sha256: state.archive_sha256.clone(),
            source: state.source.clone(),
            target_exe_path: state.target_exe_path.clone(),
            target_dir: state.target_dir.clone(),
            modules: state.modules.clone(),
            release_files: state.release_files.clone(),
            runtime_bindings: state.runtime_bindings.clone(),
            directory_receipts: state.directory_receipts.clone(),
            proxy_topology_id: state.proxy_topology_id.clone(),
            config_schema: state.config_schema,
            config_base_release: state.config_base_release.clone(),
            adoption_state: state.adoption_state,
            prerequisite_binding: state.prerequisite_binding,
            created_at: state.created_at,
            updated_at: state.updated_at,
        }
    }
}

impl OptiScalerInstallStateParts {
    pub(super) fn build(
        self,
        configuration_baseline: super::OptiScalerConfigurationBaseline,
    ) -> OptiScalerInstallState {
        OptiScalerInstallState {
            game_id: self.game_id,
            release_id: self.release_id,
            manifest_revision: self.manifest_revision,
            archive_sha256: self.archive_sha256,
            source: self.source,
            target_exe_path: self.target_exe_path,
            target_dir: self.target_dir,
            modules: self.modules,
            release_files: self.release_files,
            runtime_bindings: self.runtime_bindings,
            directory_receipts: self.directory_receipts,
            proxy_topology_id: self.proxy_topology_id,
            config_schema: self.config_schema,
            config_base_release: self.config_base_release,
            adoption_state: self.adoption_state,
            prerequisite_binding: self.prerequisite_binding,
            created_at: self.created_at,
            updated_at: self.updated_at,
            configuration_baseline,
        }
    }
}
