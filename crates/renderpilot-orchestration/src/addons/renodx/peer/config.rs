//! Pure typed `ReShade.ini` planning for an active RenoDX install.

use std::path::Path;

use renderpilot_domain::{RenoDxReshadeIniAuthority, Sha256Hash};

use crate::addons::renodx::install::PreparedInstall;
use crate::addons::renodx::peer::{InstallActiveSnapshot, RenoDxConfigSourceSeal};
use crate::addons::renodx::reshade_ini::{RenoDxConfigError as RenoDxIniError, plan_config};
use crate::addons::reshade::ini_schema::ini_merge_strategy;
use crate::peer_mutation_executor::VerifiedPeerFile;

use super::effects::{RenoDxPeerEffectAccumulator, RenoDxPeerEffectError, RenoDxPeerEffectGroup};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RenoDxConfigError {
    UnsupportedFeature,
    InvalidPath,
    InvalidSource(&'static str),
    InvalidDigest,
    Effects(RenoDxPeerEffectError),
    Ini(RenoDxIniError),
}

impl std::fmt::Display for RenoDxConfigError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedFeature => {
                formatter.write_str("active RenoDX config feature is not a main install")
            }
            Self::InvalidPath => formatter.write_str("active RenoDX config path is invalid"),
            Self::InvalidSource(reason) => {
                write!(formatter, "invalid retained RenoDX config: {reason}")
            }
            Self::InvalidDigest => formatter.write_str("invalid retained RenoDX config digest"),
            Self::Effects(error) => error.fmt(formatter),
            Self::Ini(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for RenoDxConfigError {}

impl From<RenoDxPeerEffectError> for RenoDxConfigError {
    fn from(error: RenoDxPeerEffectError) -> Self {
        Self::Effects(error)
    }
}

impl From<RenoDxIniError> for RenoDxConfigError {
    fn from(error: RenoDxIniError) -> Self {
        Self::Ini(error)
    }
}

/// Result of config planning. A typed authority exists only when the config
/// endpoint actually changes; existing changed files never become ownership
/// claims and absent changed files are represented solely in `created`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RenoDxConfigProjection {
    authority: Option<RenoDxReshadeIniAuthority>,
    created: bool,
    receipt: Option<renderpilot_domain::RenoDxConfigReceipt>,
}

impl RenoDxConfigProjection {
    pub(crate) fn authority(&self) -> Option<&RenoDxReshadeIniAuthority> {
        self.authority.as_ref()
    }

    pub(crate) const fn created(&self) -> bool {
        self.created
    }

    pub(crate) fn receipt(&self) -> Option<&renderpilot_domain::RenoDxConfigReceipt> {
        self.receipt.as_ref()
    }
}

/// Emits at most one typed `ReShade.ini` endpoint. No filesystem state is
/// consulted: all source bytes and metadata come from the phase-one seal.
pub(crate) fn lower_config(
    snapshot: &InstallActiveSnapshot,
    prepared: &PreparedInstall,
    accumulator: &mut RenoDxPeerEffectAccumulator,
) -> Result<RenoDxConfigProjection, RenoDxConfigError> {
    if !snapshot.feature().is_main_install() {
        return Err(RenoDxConfigError::UnsupportedFeature);
    }

    let root = snapshot.root_seal().canonical_game_root_ref().clone();
    let authority = RenoDxReshadeIniAuthority::new(snapshot.feature(), root)
        .map_err(|_| RenoDxConfigError::InvalidPath)?;
    let ini_path = path_ref(snapshot.root_seal().config_source().exact_ini_path())?;
    if !authority.matches_ini_path(&ini_path) {
        return Err(RenoDxConfigError::InvalidPath);
    }

    let source = snapshot.root_seal().config_source();
    let (before, before_bytes, base_bytes) = match source {
        RenoDxConfigSourceSeal::Absent { .. } => (None, None, &[][..]),
        RenoDxConfigSourceSeal::File {
            owned_bytes,
            identity,
            digest,
            length,
            ..
        } => {
            if identity.trim().is_empty() {
                return Err(RenoDxConfigError::InvalidSource("empty file identity"));
            }
            if *length != owned_bytes.len() as u64 {
                return Err(RenoDxConfigError::InvalidSource(
                    "retained length differs from bytes",
                ));
            }
            if digest_bytes(owned_bytes)? != *digest {
                return Err(RenoDxConfigError::InvalidSource(
                    "retained digest differs from bytes",
                ));
            }
            let file = VerifiedPeerFile::new_with_length(identity.clone(), digest.clone(), *length)
                .map_err(|_| RenoDxConfigError::InvalidSource("invalid file image"))?;
            (
                Some(file),
                Some(owned_bytes.clone()),
                owned_bytes.as_slice(),
            )
        }
    };

    let mut tweaks = prepared.ini_tweaks.clone();
    if !snapshot.content().is_empty() {
        // Preserve the established RenoDX policy: a non-empty ReShade tree
        // must not have its bundled add-ons disabled as part of this install.
        tweaks.disabled_addons.clear();
    }
    let desired_set_path = prepared.processing_path.desired_set_path();
    let config = prepared.renodx_config.as_ref();
    if desired_set_path.is_none()
        && !config.is_some_and(|config| !config.settings.is_empty())
        && !has_write_keys(&tweaks)
    {
        return Ok(RenoDxConfigProjection {
            authority: None,
            created: false,
            receipt: None,
        });
    }

    let typed_config =
        if desired_set_path.is_some() || config.is_some_and(|config| !config.settings.is_empty()) {
            Some(
                plan_config(ini_path.clone(), base_bytes, desired_set_path, config)
                    .map_err(RenoDxConfigError::Ini)?,
            )
        } else {
            None
        };
    let strategy = ini_merge_strategy(&tweaks);
    let (merged, receipt) = match typed_config {
        Some(config) => {
            let text = std::str::from_utf8(&config.after)
                .map_err(|_| RenoDxConfigError::Ini(RenoDxIniError::NonUtf8))?;
            let merged = if strategy.has_writes() {
                strategy.apply(text).into_bytes()
            } else {
                config.after
            };
            (merged, Some(config.receipt))
        }
        None => {
            let base = String::from_utf8_lossy(base_bytes);
            let merged = if strategy.has_writes() {
                strategy.apply(&base)
            } else {
                base.into_owned()
            };
            (merged.into_bytes(), None)
        }
    };
    match (before, before_bytes) {
        (None, _) => {
            if merged.is_empty() {
                return Ok(RenoDxConfigProjection {
                    authority: None,
                    created: false,
                    receipt,
                });
            }
            accumulator.create(RenoDxPeerEffectGroup::Config, ini_path, merged)?;
            Ok(RenoDxConfigProjection {
                authority: Some(authority),
                created: true,
                receipt,
            })
        }
        (Some(before), Some(before_bytes)) => {
            if merged == before_bytes {
                Ok(RenoDxConfigProjection {
                    authority: None,
                    created: false,
                    receipt,
                })
            } else {
                accumulator.replace(
                    RenoDxPeerEffectGroup::Config,
                    ini_path,
                    &before,
                    before_bytes,
                    merged,
                )?;
                Ok(RenoDxConfigProjection {
                    authority: Some(authority),
                    created: false,
                    receipt,
                })
            }
        }
        (Some(_), None) => Err(RenoDxConfigError::InvalidSource(
            "present config source has no retained bytes",
        )),
    }
}

fn has_write_keys(tweaks: &crate::addons::reshade::types::ReshadeIniTweaks) -> bool {
    !tweaks.disabled_addons.is_empty() || tweaks.addon_path.is_some() || tweaks.dlss_fix.is_some()
}

fn path_ref(path: &Path) -> Result<renderpilot_domain::PathRef, RenoDxConfigError> {
    let value = path.to_str().ok_or(RenoDxConfigError::InvalidPath)?;
    renderpilot_domain::PathRef::new(value.to_owned()).map_err(|_| RenoDxConfigError::InvalidPath)
}

fn digest_bytes(bytes: &[u8]) -> Result<Sha256Hash, RenoDxConfigError> {
    renderpilot_detection::sha256_bytes(bytes).map_err(|_| RenoDxConfigError::InvalidDigest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::addons::engine::MergeStrategy;
    use crate::addons::renodx::types::renodx_ini_defaults;

    #[test]
    fn default_config_strategy_has_only_renodx_keys() {
        let strategy = ini_merge_strategy(&renodx_ini_defaults());
        assert!(matches!(strategy, MergeStrategy::IniSetKeys { .. }));
        assert!(strategy.apply("").contains("DisabledAddons"));
    }
}
