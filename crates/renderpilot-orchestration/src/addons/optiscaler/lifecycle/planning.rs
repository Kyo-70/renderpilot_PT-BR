use super::*;

/// Inputs for a first managed OptiScaler installation.
pub(crate) struct InstallOptiScalerRequest<'a> {
    /// Application services and repositories.
    pub context: &'a Context,
    /// Validated immutable release catalogue.
    pub manifest: &'a OptiScalerManifest,
    /// Validated compatibility catalogue snapshot.
    pub catalog: &'a OptiScalerCompatibilityCatalog,
    pub safety: crate::GameSafetyPermit,
    /// Optional explicit module selection; defaults are used when absent.
    pub modules: Option<&'a [String]>,
    /// Optional download progress observer.
    pub progress: Option<&'a ProgressObserver<'a>>,
}

/// Inputs for an update or repair.
pub(crate) struct UpdateOptiScalerRequest<'a> {
    /// Application services and repositories.
    pub context: &'a Context,
    /// Validated immutable release catalogue.
    pub manifest: &'a OptiScalerManifest,
    /// Validated compatibility catalogue snapshot.
    pub catalog: &'a OptiScalerCompatibilityCatalog,
    pub safety: crate::GameSafetyPermit,
    /// Optional download progress observer.
    pub progress: Option<&'a ProgressObserver<'a>>,
}

/// Inputs for changing the module selection of an existing installation.
pub(crate) struct SetOptiScalerModulesRequest<'a> {
    /// Application services and repositories.
    pub context: &'a Context,
    /// Validated immutable release catalogue.
    pub manifest: &'a OptiScalerManifest,
    /// Validated compatibility catalogue snapshot.
    pub catalog: &'a OptiScalerCompatibilityCatalog,
    pub safety: crate::GameSafetyPermit,
    /// Requested stable manifest module identifiers.
    pub modules: &'a [String],
    /// Optional download progress observer.
    pub progress: Option<&'a ProgressObserver<'a>>,
}

/// Inputs for explicitly moving an installation to another executable.
pub(crate) struct RelocateOptiScalerRequest<'a> {
    /// Application services and repositories.
    pub context: &'a Context,
    /// Validated immutable release catalogue.
    pub manifest: &'a OptiScalerManifest,
    /// Validated compatibility catalogue snapshot.
    pub catalog: &'a OptiScalerCompatibilityCatalog,
    pub safety: crate::GameSafetyPermit,
    /// New effective executable selected by the user.
    pub target_exe: &'a Path,
    /// Optional download progress observer.
    pub progress: Option<&'a ProgressObserver<'a>>,
}

#[derive(Debug, Clone)]
pub(in crate::addons::optiscaler) struct ApplyTarget {
    pub(in crate::addons::optiscaler) exe: PathBuf,
    pub(in crate::addons::optiscaler) dir: PathBuf,
    pub(in crate::addons::optiscaler) proxy: EvaluatedProxyPlan,
}

#[derive(Debug, Clone)]
pub(in crate::addons::optiscaler) enum ApplyIntent {
    Install { modules: Option<Vec<String>> },
    Update,
    Repair,
    SetModules { modules: Vec<String> },
    Relocate { target_exe: PathBuf },
}

/// Policy boundary between passive receipt reconciliation and an explicit
/// user-requested adoption performed by the install command.
#[derive(Clone, Copy)]
pub(in crate::addons::optiscaler) enum AdoptionPolicy {
    Reconcile,
    UserRequested,
}

impl AdoptionPolicy {
    pub(in crate::addons::optiscaler) fn ensure_allowed(
        self,
        availability: &EvaluatedAvailability,
    ) -> Result<(), ServiceError> {
        match self {
            Self::Reconcile => Ok(()),
            Self::UserRequested => ensure_adoption_allowed(availability),
        }
    }
}

impl ApplyIntent {
    pub(in crate::addons::optiscaler) const fn mutation_feature(&self) -> &'static str {
        match self {
            Self::Install { .. } => renderpilot_domain::mutation_features::OPTISCALER_INSTALL,
            Self::Update | Self::Repair | Self::SetModules { .. } => {
                renderpilot_domain::mutation_features::OPTISCALER_UPDATE
            }
            Self::Relocate { .. } => renderpilot_domain::mutation_features::OPTISCALER_RELOCATE,
        }
    }

    pub(in crate::addons::optiscaler) fn requested_modules(&self) -> Option<&[String]> {
        match self {
            Self::Install { modules } => modules.as_deref(),
            Self::SetModules { modules } => Some(modules),
            Self::Update | Self::Repair | Self::Relocate { .. } => None,
        }
    }

    pub(in crate::addons::optiscaler) fn relocation_target(&self) -> Option<&Path> {
        match self {
            Self::Relocate { target_exe } => Some(target_exe),
            _ => None,
        }
    }
}

pub(in crate::addons::optiscaler) struct ApplyPlan<'a> {
    pub(in crate::addons::optiscaler) intent: ApplyIntent,
    pub(in crate::addons::optiscaler) context: &'a Context,
    pub(in crate::addons::optiscaler) guard: crate::game_mutation_lock::GameMutationGuard,
    pub(in crate::addons::optiscaler) manifest: OptiScalerManifest,
    pub(in crate::addons::optiscaler) game_id: GameId,
    pub(in crate::addons::optiscaler) old_state: Option<OptiScalerInstallState>,
    pub(in crate::addons::optiscaler) old_managed_files: Vec<ManagedAddonFile>,
    pub(in crate::addons::optiscaler) old_config_base: Option<Vec<u8>>,
    pub(in crate::addons::optiscaler) release: OptiScalerRelease,
    pub(in crate::addons::optiscaler) modules: HashSet<String>,
    pub(in crate::addons::optiscaler) artifacts: ArtifactSet,
    pub(in crate::addons::optiscaler) target: ApplyTarget,
    pub(in crate::addons::optiscaler) safety: crate::GameSafetyPermit,
    pub(in crate::addons::optiscaler) compatibility_invariants: Vec<ManagedIniValue>,
    /// Exact prerequisite accepted under the final guarded compatibility
    /// evaluation. Existing states retain their persisted binding instead.
    pub(in crate::addons::optiscaler) accepted_prerequisite_binding:
        renderpilot_domain::OptiScalerPrerequisiteBinding,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::addons::optiscaler) struct OptiScalerLifecycleSnapshot {
    pub(in crate::addons::optiscaler) state: Option<OptiScalerInstallState>,
    pub(in crate::addons::optiscaler) topology: Option<GameProxyTopology>,
}

pub(in crate::addons::optiscaler) struct ArtifactSet {
    pub(in crate::addons::optiscaler) archive: PreparedArchive,
    pub(in crate::addons::optiscaler) module_artifacts: Vec<PreparedModuleArtifact>,
    pub(in crate::addons::optiscaler) native_artifacts: Vec<PreparedNativeModule>,
}
