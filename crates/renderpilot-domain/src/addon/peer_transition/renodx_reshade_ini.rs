//! Typed RenoDX mutation authority for the shared ReShade configuration endpoint.

use crate::{PathRef, PeerTransitionError, normalized_path_key};

/// RenoDX feature operations that may mutate through the typed authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenoDxReshadeIniFeature {
    /// Mutate the shared configuration for a catalog-provided RenoDX add-on.
    Install,
    /// Mutate the shared configuration for a local-file RenoDX add-on.
    InstallFromFile,
    /// Remove the RenoDX add-on through the typed mutation authority.
    Uninstall,
    /// Reconcile the RenoDX add-on's narrow Set_Path policy during update.
    Update,
    /// Mutate the shared configuration for a RenoDX DLSS fix install.
    DlssFixInstall,
    /// Mutate the shared configuration for a RenoDX DLSS fix update.
    DlssFixUpdate,
    /// Remove the RenoDX DLSS fix through the typed mutation authority.
    DlssFixUninstall,
}

/// Exact feature identifier for the main RenoDX install.
pub const RENODX_INSTALL: &str = crate::mutation_features::RENODX_INSTALL;
/// Exact feature identifier for a local-file RenoDX install.
pub const RENODX_INSTALL_FROM_FILE: &str = crate::mutation_features::RENODX_INSTALL_FROM_FILE;
/// Exact feature identifier for the main RenoDX uninstall.
pub const RENODX_UNINSTALL: &str = crate::mutation_features::RENODX_UNINSTALL;
/// Exact feature identifier for a RenoDX DLSS-fix install.
pub const RENODX_DLSS_FIX_INSTALL: &str = crate::mutation_features::RENODX_DLSS_FIX_INSTALL;
/// Exact feature identifier for a RenoDX DLSS-fix update.
pub const RENODX_DLSS_FIX_UPDATE: &str = crate::mutation_features::RENODX_DLSS_FIX_UPDATE;
/// Exact feature identifier for a RenoDX DLSS-fix uninstall.
pub const RENODX_DLSS_FIX_UNINSTALL: &str = crate::mutation_features::RENODX_DLSS_FIX_UNINSTALL;

impl RenoDxReshadeIniFeature {
    /// Parses only the feature identifiers admitted by the typed endpoint.
    pub fn try_from_feature(feature: &str) -> Result<Self, PeerTransitionError> {
        match feature {
            RENODX_INSTALL => Ok(Self::Install),
            RENODX_INSTALL_FROM_FILE => Ok(Self::InstallFromFile),
            RENODX_UNINSTALL => Ok(Self::Uninstall),
            crate::mutation_features::RENODX_UPDATE => Ok(Self::Update),
            RENODX_DLSS_FIX_INSTALL => Ok(Self::DlssFixInstall),
            RENODX_DLSS_FIX_UPDATE => Ok(Self::DlssFixUpdate),
            RENODX_DLSS_FIX_UNINSTALL => Ok(Self::DlssFixUninstall),
            _ => Err(PeerTransitionError::UnsupportedRenoDxReshadeIniFeature),
        }
    }

    /// Returns the exact persisted feature identifier.
    #[must_use]
    pub const fn as_feature(self) -> &'static str {
        match self {
            Self::Install => RENODX_INSTALL,
            Self::InstallFromFile => RENODX_INSTALL_FROM_FILE,
            Self::Uninstall => RENODX_UNINSTALL,
            Self::Update => crate::mutation_features::RENODX_UPDATE,
            Self::DlssFixInstall => RENODX_DLSS_FIX_INSTALL,
            Self::DlssFixUpdate => RENODX_DLSS_FIX_UPDATE,
            Self::DlssFixUninstall => RENODX_DLSS_FIX_UNINSTALL,
        }
    }

    /// Returns whether this feature is a main add-on install.
    #[must_use]
    pub const fn is_main_install(self) -> bool {
        matches!(self, Self::Install | Self::InstallFromFile)
    }

    /// Returns whether this operation is the main add-on update.
    #[must_use]
    pub const fn is_main_update(self) -> bool {
        matches!(self, Self::Update)
    }

    /// Returns whether this feature is a main add-on uninstall.
    #[must_use]
    pub const fn is_main_uninstall(self) -> bool {
        matches!(self, Self::Uninstall)
    }

    /// Returns whether this feature is a DLSS-fix operation.
    #[must_use]
    pub const fn is_dlss_fix(self) -> bool {
        matches!(
            self,
            Self::DlssFixInstall | Self::DlssFixUpdate | Self::DlssFixUninstall
        )
    }
}

/// Sealed root and exact RenoDX mutation authority for ReShade.ini.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenoDxReshadeIniAuthority {
    feature: RenoDxReshadeIniFeature,
    canonical_game_root: PathRef,
    ini_path: PathRef,
}

impl RenoDxReshadeIniAuthority {
    /// Builds mutation authority for the exact ReShade.ini below `canonical_game_root`.
    pub fn new(
        feature: RenoDxReshadeIniFeature,
        canonical_game_root: PathRef,
    ) -> Result<Self, PeerTransitionError> {
        let root = canonical_game_root.as_str();
        let ini = if root.ends_with('/') {
            format!("{root}ReShade.ini")
        } else {
            format!("{root}/ReShade.ini")
        };
        let ini_path = PathRef::new(ini).map_err(|_| {
            PeerTransitionError::InvalidRenoDxReshadeIniPath(canonical_game_root.clone())
        })?;
        Ok(Self {
            feature,
            canonical_game_root,
            ini_path,
        })
    }

    /// Parses a feature and builds its exact endpoint authority.
    pub fn try_from_feature(
        feature: &str,
        canonical_game_root: PathRef,
    ) -> Result<Self, PeerTransitionError> {
        Self::new(
            RenoDxReshadeIniFeature::try_from_feature(feature)?,
            canonical_game_root,
        )
    }

    /// Returns the admitted RenoDX feature.
    #[must_use]
    pub const fn feature(&self) -> RenoDxReshadeIniFeature {
        self.feature
    }

    /// Returns the sealed canonical game root.
    #[must_use]
    pub fn canonical_game_root(&self) -> &PathRef {
        &self.canonical_game_root
    }

    /// Returns the exact derived ReShade.ini path.
    #[must_use]
    pub fn ini_path(&self) -> &PathRef {
        &self.ini_path
    }

    /// Compares a path using the domain's Windows/Unix lexical identity.
    #[must_use]
    pub fn matches_ini_path(&self, path: &PathRef) -> bool {
        normalized_path_key(self.ini_path.as_str()) == normalized_path_key(path.as_str())
    }
}
