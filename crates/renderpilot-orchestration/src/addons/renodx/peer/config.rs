//! Pure typed `ReShade.ini` planning for an active RenoDX install.

use std::path::Path;

use renderpilot_domain::{RenoDxReshadeIniAuthority, Sha256Hash};

use crate::addons::renodx::install::PreparedInstall;
use crate::addons::renodx::peer::{InstallActiveSnapshot, RenoDxConfigSourceSeal};
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
        }
    }
}

impl std::error::Error for RenoDxConfigError {}

impl From<RenoDxPeerEffectError> for RenoDxConfigError {
    fn from(error: RenoDxPeerEffectError) -> Self {
        Self::Effects(error)
    }
}

/// Result of config planning. A typed authority exists only when the config
/// endpoint actually changes; existing changed files never become ownership
/// claims and absent changed files are represented solely in `created`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RenoDxConfigProjection {
    authority: Option<RenoDxReshadeIniAuthority>,
    created: bool,
}

impl RenoDxConfigProjection {
    pub(crate) fn authority(&self) -> Option<&RenoDxReshadeIniAuthority> {
        self.authority.as_ref()
    }

    pub(crate) const fn created(&self) -> bool {
        self.created
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
    let (before, before_bytes, base) = match source {
        RenoDxConfigSourceSeal::Absent { .. } => (None, None, std::borrow::Cow::Borrowed("")),
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
                String::from_utf8_lossy(owned_bytes),
            )
        }
    };

    let mut tweaks = prepared.ini_tweaks.clone();
    if !snapshot.content().is_empty() {
        // Preserve the established RenoDX policy: a non-empty ReShade tree
        // must not have its bundled add-ons disabled as part of this install.
        tweaks.disabled_addons.clear();
    }
    if !has_write_keys(&tweaks) {
        return Ok(RenoDxConfigProjection {
            authority: None,
            created: false,
        });
    }

    let merged = ini_merge_strategy(&tweaks).apply(&base);
    match (before, before_bytes) {
        (None, _) => {
            if merged.is_empty() {
                return Ok(RenoDxConfigProjection {
                    authority: None,
                    created: false,
                });
            }
            accumulator.create(RenoDxPeerEffectGroup::Config, ini_path, merged.into_bytes())?;
            Ok(RenoDxConfigProjection {
                authority: Some(authority),
                created: true,
            })
        }
        (Some(before), Some(before_bytes)) => {
            if merged.as_bytes() == before_bytes.as_slice() {
                Ok(RenoDxConfigProjection {
                    authority: None,
                    created: false,
                })
            } else {
                accumulator.replace(
                    RenoDxPeerEffectGroup::Config,
                    ini_path,
                    &before,
                    before_bytes,
                    merged.into_bytes(),
                )?;
                Ok(RenoDxConfigProjection {
                    authority: Some(authority),
                    created: false,
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
