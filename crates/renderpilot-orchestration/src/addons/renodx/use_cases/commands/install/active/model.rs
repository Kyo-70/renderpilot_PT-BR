use renderpilot_domain::{Architecture, GameProxyTopology};

use crate::Context;
use crate::addons::renodx::matcher::ResolvedInstall;
use crate::addons::renodx::peer::{InstallActiveSnapshot, InstallCommandVariant};
use crate::addons::reshade::types::ReshadeChannel;
use renderpilot_domain::RenoDxReshadeIniFeature;

/// The source variant selected by the public RenoDX install command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActiveInstallSource {
    /// Resolve the add-on from the RenoDX catalogue.
    Catalog,
    /// Resolve a compatible plan for a caller-provided local add-on.
    InstallFromFile { architecture: Architecture },
}

impl ActiveInstallSource {
    /// Stable command identity used by both phase snapshots and fingerprints.
    pub(crate) const fn command_variant(self) -> InstallCommandVariant {
        match self {
            Self::Catalog => InstallCommandVariant::Catalog,
            Self::InstallFromFile { .. } => InstallCommandVariant::InstallFromFile,
        }
    }

    /// Typed ReShade.ini endpoint feature for this source.
    pub(crate) const fn reshade_ini_feature(self) -> RenoDxReshadeIniFeature {
        match self {
            Self::Catalog => RenoDxReshadeIniFeature::Install,
            Self::InstallFromFile { .. } => RenoDxReshadeIniFeature::InstallFromFile,
        }
    }

    /// Stable wire label used in the request fingerprint.
    pub(crate) const fn stable_label(self) -> &'static str {
        match self {
            Self::Catalog => "catalog",
            Self::InstallFromFile { .. } => "install_from_file",
        }
    }

    /// Architecture supplied by the local-file source, if any.
    pub(crate) const fn file_architecture(self) -> Option<Architecture> {
        match self {
            Self::Catalog => None,
            Self::InstallFromFile { architecture } => Some(architecture),
        }
    }
}

/// Immutable inputs for one active-install phase.
pub(crate) struct ResolveActiveInstallRequest<'a> {
    pub(crate) context: &'a Context,
    pub(crate) manifest: &'a crate::addons::renodx::types::RenoDxManifest,
    pub(crate) game_id: &'a renderpilot_domain::GameId,
    pub(crate) requested_channel: ReshadeChannel,
    pub(crate) selected_topology: GameProxyTopology,
    pub(crate) source: ActiveInstallSource,
}

/// The resolved plan plus every read-side fact sealed for the active route.
pub(crate) struct ActiveInstallResolution {
    plan: ResolvedInstall,
    snapshot: InstallActiveSnapshot,
}

impl ActiveInstallResolution {
    pub(crate) fn new(plan: ResolvedInstall, snapshot: InstallActiveSnapshot) -> Self {
        Self { plan, snapshot }
    }

    pub(crate) fn plan(&self) -> &ResolvedInstall {
        &self.plan
    }

    pub(crate) fn snapshot(&self) -> &InstallActiveSnapshot {
        &self.snapshot
    }

    pub(crate) fn into_parts(self) -> (ResolvedInstall, InstallActiveSnapshot) {
        (self.plan, self.snapshot)
    }
}
