use super::super::runtime;
use super::*;

pub(in crate::addons::optiscaler::lifecycle::apply) struct FilesystemApplyPlan<'a> {
    pub(in crate::addons::optiscaler::lifecycle::apply) layout: PathLayout<'a>,
    pub(in crate::addons::optiscaler::lifecycle::apply) cleanup: CleanupPlan,
    pub(in crate::addons::optiscaler::lifecycle::apply) preservations: Vec<ConfigPreservationPlan>,
    pub(in crate::addons::optiscaler::lifecycle::apply) journal: MutationJournal,
    pub(in crate::addons::optiscaler::lifecycle::apply) peer_host_transition:
        Option<adoption::PeerHostTransitionPlan>,
    pub(in crate::addons::optiscaler::lifecycle::apply) config: ConfigInputs,
    pub(in crate::addons::optiscaler::lifecycle::apply) release_source_url: String,
    pub(in crate::addons::optiscaler::lifecycle::apply) retained_claims:
        Vec<renderpilot_storage_sqlite::OptiScalerRetainedClaim>,
    /// Game-owned AMD FSR entry points kept in a deterministic original backup
    /// while the selected OptiScaler release occupies their active filename.
    pub(in crate::addons::optiscaler::lifecycle::apply) retained_fsr:
        Vec<RetainedFsrEntryPointPlan>,
}

pub(in crate::addons::optiscaler::lifecycle::apply) struct PathLayout<'a> {
    pub(in crate::addons::optiscaler::lifecycle::apply) new_paths: HashMap<String, PathBuf>,
    pub(in crate::addons::optiscaler::lifecycle::apply) release_write_paths: HashSet<String>,
    pub(in crate::addons::optiscaler::lifecycle::apply) native_targets: Vec<NativeTarget>,
    pub(in crate::addons::optiscaler::lifecycle::apply) native_paths: NativeModulePaths,
    pub(in crate::addons::optiscaler::lifecycle::apply) artifact_targets:
        Vec<(&'a PreparedModuleArtifact, PathBuf)>,
    pub(in crate::addons::optiscaler::lifecycle::apply) artifact_write_paths: HashSet<String>,
    pub(in crate::addons::optiscaler::lifecycle::apply) runtime_plans:
        HashMap<String, runtime::RuntimeApplyPlan>,
    pub(in crate::addons::optiscaler::lifecycle::apply) native_expected_sha256:
        HashMap<String, Sha256Hash>,
    pub(in crate::addons::optiscaler::lifecycle::apply) native_copy_paths: HashSet<String>,
    pub(in crate::addons::optiscaler::lifecycle::apply) removed_old_paths: Vec<PathBuf>,
    pub(in crate::addons::optiscaler::lifecycle::apply) old_proxy_path: PathBuf,
}

pub(in crate::addons::optiscaler::lifecycle::apply) struct CleanupPlan {
    pub(in crate::addons::optiscaler::lifecycle::apply) other_claim_paths: HashSet<String>,
    pub(in crate::addons::optiscaler::lifecycle::apply) preplanned_nonmanaged_keys: HashSet<String>,
    pub(in crate::addons::optiscaler::lifecycle::apply) exact_removed_paths: Vec<PathBuf>,
    pub(in crate::addons::optiscaler::lifecycle::apply) preplanned_preserved: Vec<String>,
}

pub(in crate::addons::optiscaler::lifecycle::apply) struct MutationJournal {
    pub(in crate::addons::optiscaler::lifecycle::apply) scope: MutationScope,
    pub(in crate::addons::optiscaler::lifecycle::apply) targets: Vec<MutationTarget>,
}

pub(in crate::addons::optiscaler::lifecycle::apply) struct ConfigInputs {
    pub(in crate::addons::optiscaler::lifecycle::apply) config_member_archive_path: String,
    pub(in crate::addons::optiscaler::lifecycle::apply) new_config: Vec<u8>,
    pub(in crate::addons::optiscaler::lifecycle::apply) current_config: Vec<u8>,
    /// Whether the user configuration is merged in place or retained at its
    /// old location while a fresh canonical target is created.
    pub(in crate::addons::optiscaler::lifecycle::apply) target_mode: ConfigTargetMode,
    /// Existing user configuration path used as the merge source.
    pub(in crate::addons::optiscaler::lifecycle::apply) source_path: PathBuf,
    /// Canonical configuration path for the successor release.
    pub(in crate::addons::optiscaler::lifecycle::apply) destination_path: PathBuf,
    pub(in crate::addons::optiscaler::lifecycle::apply) current_sha256: Option<Sha256Hash>,
    pub(in crate::addons::optiscaler::lifecycle::apply) current_receipt: Option<FileReceipt>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::addons::optiscaler::lifecycle::apply) enum ConfigTargetMode {
    InPlace,
    RetargetToAbsent,
}

pub(in crate::addons::optiscaler::lifecycle::apply) struct RemovedPathRequest<'a> {
    pub(in crate::addons::optiscaler::lifecycle::apply) game_id: &'a GameId,
    pub(in crate::addons::optiscaler::lifecycle::apply) old_state:
        Option<&'a OptiScalerInstallState>,
    pub(in crate::addons::optiscaler::lifecycle::apply) old_managed_files: &'a [ManagedAddonFile],
    pub(in crate::addons::optiscaler::lifecycle::apply) old_proxy_path: &'a Path,
    pub(in crate::addons::optiscaler::lifecycle::apply) new_proxy_path: &'a Path,
    pub(in crate::addons::optiscaler::lifecycle::apply) target_dir: &'a Path,
    pub(in crate::addons::optiscaler::lifecycle::apply) removed_paths: &'a [PathBuf],
    pub(in crate::addons::optiscaler::lifecycle::apply) other_claim_paths: HashSet<String>,
    pub(in crate::addons::optiscaler::lifecycle::apply) expected_hashes:
        &'a HashMap<String, Sha256Hash>,
}

pub(in crate::addons::optiscaler::lifecycle::apply) struct RemovedPathPlan {
    pub(in crate::addons::optiscaler::lifecycle::apply) cleanup: CleanupPlan,
    pub(in crate::addons::optiscaler::lifecycle::apply) preservations: Vec<ConfigPreservationPlan>,
    pub(in crate::addons::optiscaler::lifecycle::apply) quarantine: Vec<MutationTarget>,
}
