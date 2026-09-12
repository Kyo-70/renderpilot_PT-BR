use std::ops::Deref;

use serde::Deserialize;

use crate::addons::catalog_message::{CatalogMessage, WireCatalogMessage};
use crate::{ServiceError, failed};

use super::validation;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WireCompatibilityCatalog {
    pub(crate) schema_version: u32,
    pub(crate) revision: String,
    pub(crate) upstream: WireUpstream,
    pub(crate) entries: Vec<WireCompatibilityEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WireUpstream {
    pub(crate) source: String,
    pub(crate) snapshot_revision: String,
    pub(crate) snapshot_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WireCompatibilityEntry {
    pub(crate) id: String,
    pub(crate) status: CompatibilityStatus,
    pub(crate) identities: Vec<EntryIdentity>,
    #[serde(default)]
    pub(crate) declared_inputs: Vec<DeclaredInput>,
    #[serde(default)]
    pub(crate) guidance: Vec<WireCatalogGuidance>,
    pub(crate) variants: Vec<WireCompatibilityVariant>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WireCatalogGuidance {
    pub(crate) kind: GuidanceKind,
    pub(crate) message: WireCatalogMessage,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CatalogGuidance {
    pub(crate) kind: GuidanceKind,
    pub(crate) message: CatalogMessage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum GuidanceKind {
    Warning,
    Compatibility,
    GameSetting,
}

impl GuidanceKind {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Warning => "warning",
            Self::Compatibility => "compatibility",
            Self::GameSetting => "game_setting",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CompatibilityStatus {
    Working,
    Conditional,
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EntryIdentity {
    pub(crate) kind: EntryIdentityKind,
    pub(crate) value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum EntryIdentityKind {
    SteamAppid,
    EpicId,
    GogId,
    /// Microsoft Store product id, resolved only from an exact registered Xbox
    /// package root's `MicrosoftGame.config` StoreId.
    XboxStoreId,
    /// Exact executable basename. This is intentionally not the shared glob
    /// matcher: catalogue identities are standalone and collision-checked.
    ExeName,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DeclaredInput {
    Dlss2Plus,
    Fsr2Plus,
    Xess,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WireCompatibilityVariant {
    #[serde(default)]
    pub(crate) when: Option<WireVariantCondition>,
    pub(crate) proxy: ProxyPolicy,
    #[serde(default)]
    pub(crate) ini_overrides: Vec<crate::addons::optiscaler::types::ManagedIniValue>,
    #[serde(default)]
    pub(crate) launch: Option<WireLaunchPolicy>,
    #[serde(default)]
    pub(crate) restricted_modules: Vec<String>,
    pub(crate) optipatcher: OptiPatcherPolicy,
    pub(crate) prerequisite: CompatibilityPrerequisite,
}

/// Reviewed launch configuration associated with one exact compatibility variant.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WireLaunchPolicy {
    pub(crate) arguments: Vec<String>,
    pub(crate) requirement: LaunchRequirement,
}

/// Whether the reviewed launch configuration is mandatory or advisory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LaunchRequirement {
    Required,
    Recommended,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WireVariantCondition {
    #[serde(default)]
    pub(crate) launcher: Option<WindowsLauncher>,
    #[serde(default)]
    pub(crate) executable: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedVariant {
    pub(crate) proxy: ProxyPolicy,
    pub(crate) ini_overrides: Vec<crate::addons::optiscaler::types::ManagedIniValue>,
    pub(crate) launch: Option<WireLaunchPolicy>,
    pub(crate) restricted_modules: Vec<String>,
    pub(crate) optipatcher: OptiPatcherPolicy,
    pub(crate) prerequisite: CompatibilityPrerequisite,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum ProxyPolicy {
    Automatic,
    Exact { slot: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum OptiPatcherPolicy {
    Unspecified,
    Unsupported,
    Supported,
    Recommended,
}

/// Lowercase compatibility-wire launcher names. These intentionally do not
/// deserialize the domain enum whose stable wire representation is title case.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WindowsLauncher {
    Steam,
    Epic,
    Gog,
    Ubisoft,
    Ea,
    BattleNet,
    Xbox,
    Manual,
}

impl WindowsLauncher {
    pub(crate) const fn matches(self, launcher: renderpilot_domain::Launcher) -> bool {
        matches!(
            (self, launcher),
            (Self::Steam, renderpilot_domain::Launcher::Steam)
                | (Self::Epic, renderpilot_domain::Launcher::Epic)
                | (Self::Gog, renderpilot_domain::Launcher::Gog)
                | (Self::Ubisoft, renderpilot_domain::Launcher::Ubisoft)
                | (Self::Ea, renderpilot_domain::Launcher::Ea)
                | (Self::BattleNet, renderpilot_domain::Launcher::BattleNet)
                | (Self::Xbox, renderpilot_domain::Launcher::Xbox)
                | (Self::Manual, renderpilot_domain::Launcher::Manual)
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CompatibilityPrerequisite {
    None,
    Luma,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CompatibilityEntry {
    pub(crate) id: String,
    pub(crate) status: CompatibilityStatus,
    pub(crate) identities: Vec<EntryIdentity>,
    pub(crate) declared_inputs: Vec<DeclaredInput>,
    pub(crate) guidance: Vec<CatalogGuidance>,
    pub(crate) variants: Vec<ValidatedVariant>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ValidatedVariant {
    pub(crate) condition: Option<ValidatedVariantCondition>,
    pub(crate) resolved: ResolvedVariant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ValidatedVariantCondition {
    pub(crate) launcher: Option<WindowsLauncher>,
    pub(crate) executable: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OptiScalerCompatibilityCatalog {
    wire: WireCompatibilityCatalog,
    entries: Vec<CompatibilityEntry>,
}

impl TryFrom<WireCompatibilityCatalog> for OptiScalerCompatibilityCatalog {
    type Error = ServiceError;

    fn try_from(wire: WireCompatibilityCatalog) -> Result<Self, Self::Error> {
        let entries = validation::validate_catalog(&wire)?;
        Ok(Self { wire, entries })
    }
}

impl Deref for OptiScalerCompatibilityCatalog {
    type Target = WireCompatibilityCatalog;

    fn deref(&self) -> &Self::Target {
        &self.wire
    }
}

impl OptiScalerCompatibilityCatalog {
    pub(crate) fn entries(&self) -> &[CompatibilityEntry] {
        &self.entries
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum ResolvedCompatibility<'a> {
    NoMatch,
    Conflict,
    Match {
        entry: &'a CompatibilityEntry,
        variant: &'a ResolvedVariant,
    },
}

impl From<WireCatalogGuidance> for CatalogGuidance {
    fn from(value: WireCatalogGuidance) -> Self {
        Self {
            kind: value.kind,
            message: value.message.into(),
        }
    }
}

impl TryFrom<WireCompatibilityVariant> for ValidatedVariant {
    type Error = ServiceError;

    fn try_from(value: WireCompatibilityVariant) -> Result<Self, Self::Error> {
        let condition = value.when.map(|condition| ValidatedVariantCondition {
            launcher: condition.launcher,
            executable: condition.executable,
        });
        Ok(Self {
            condition,
            resolved: ResolvedVariant {
                proxy: value.proxy,
                ini_overrides: value.ini_overrides,
                launch: value.launch,
                restricted_modules: value.restricted_modules,
                optipatcher: value.optipatcher,
                prerequisite: value.prerequisite,
            },
        })
    }
}

pub(crate) fn invalid_catalog(message: impl std::fmt::Display) -> ServiceError {
    failed(format!(
        "invalid OptiScaler compatibility catalog: {message}"
    ))
}
