//! Public v2 Luma document adapter → internal catalogue model.

use renderpilot_domain::Architecture;
use serde::Deserialize;

use crate::addons::catalog_message::WireCatalogMessage;
use crate::addons::matching::{MatchRule, Status};

use super::catalog::{
    GENERIC_UNITY_ASSET, GENERIC_UNITY_ASSET_X32, GENERIC_UNREAL_ASSET, LumaCategory, LumaEngine,
    LumaFeatures, LumaGuidance, LumaGuidanceKind, LumaManifest, LumaProfile, LumaTitle,
    is_generic_unity_asset, is_generic_unreal_asset,
};
use super::managed::LumaExternalRequirement;
use crate::addons::engine_config::{EngineIniEntry, EngineIniRecipe};

/// Public v2 Luma document. The nested wire model keeps curation concepts
/// explicit without forcing install code to know about JSON presentation.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WireManifestV2 {
    schema_version: u32,
    generated_at: String,
    minimum_reshade_version: String,
    games: Vec<WireGame>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct WireGame {
    pub id: String,
    pub name: String,
    pub architecture: Architecture,
    pub status: Status,
    pub r#match: Vec<MatchRule>,
    pub package: WirePackage,
    pub profile: WireProfile,
    #[serde(default)]
    pub features: Option<LumaFeatures>,
    #[serde(default)]
    pub requirements: WireRequirements,
    #[serde(default)]
    pub guidance: Vec<WireGuidance>,
    #[serde(default)]
    pub availability: Option<WireAvailability>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct WirePackage {
    pub release_asset: String,
    pub addon_file: String,
}

/// Public profile identity. It is deliberately narrower than the internal
/// tagged model and binds directly to one exact shared release asset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum WireProfile {
    Game,
    Unreal,
    Unity,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct WireRequirements {
    #[serde(default)]
    pub launch_arguments: Vec<String>,
    #[serde(default)]
    pub managed_dependency: Option<LumaExternalRequirement>,
}

/// Guidance accepted by the released v2 wire contract. Launch arguments use
/// the structured `requirements.launch_arguments` field and are not guidance.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct WireGuidance {
    pub id: String,
    pub kind: WireGuidanceKind,
    pub fallback_text: String,
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub engine_ini: Option<WireEngineIniRecipe>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct WireEngineIniRecipe {
    pub schema_version: u32,
    pub revision: u32,
    pub id: String,
    pub sections: Vec<WireEngineIniSection>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct WireEngineIniSection {
    pub name: String,
    pub entries: Vec<WireEngineIniEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct WireEngineIniEntry {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum WireGuidanceKind {
    GameSetting,
    EngineIni,
    Warning,
    Compatibility,
    ExternalTool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, tag = "kind", rename_all = "snake_case")]
pub(super) enum WireAvailability {
    Blocked { message: WireCatalogMessage },
}

impl LumaManifest {
    /// Converts a schema-v2 document to the internal installation model.
    pub(crate) fn from_wire_v2(wire: WireManifestV2) -> Result<Self, crate::ServiceError> {
        Ok(Self {
            schema_version: wire.schema_version,
            generated_at: wire.generated_at,
            min_reshade_version: wire.minimum_reshade_version,
            titles: wire
                .games
                .into_iter()
                .map(title_from_wire_v2)
                .collect::<Result<Vec<_>, _>>()?,
        })
    }
}

fn title_from_wire_v2(game: WireGame) -> Result<LumaTitle, crate::ServiceError> {
    let WireGame {
        id,
        name,
        architecture,
        status,
        r#match,
        package,
        profile,
        features,
        requirements:
            WireRequirements {
                launch_arguments,
                managed_dependency,
            },
        guidance,
        availability,
    } = game;
    let guidance = guidance_from_wire_v2(guidance)?;
    let profile = match profile {
        WireProfile::Game
            if !is_generic_unreal_asset(&package.release_asset)
                && !is_generic_unity_asset(&package.release_asset) =>
        {
            LumaProfile::Game
        }
        WireProfile::Unreal if package.release_asset == GENERIC_UNREAL_ASSET => {
            LumaProfile::Engine {
                engine: LumaEngine::Unreal,
            }
        }
        WireProfile::Unity
            if matches!(
                (architecture, package.release_asset.as_str()),
                (Architecture::X64, GENERIC_UNITY_ASSET)
                    | (Architecture::X86, GENERIC_UNITY_ASSET_X32)
            ) =>
        {
            LumaProfile::Engine {
                engine: LumaEngine::Unity,
            }
        }
        incompatible => {
            return Err(crate::ServiceError::command_failed(format!(
                "Luma v2 profile `{id}` has incompatible {incompatible:?} payload `{}` for {architecture:?}",
                package.release_asset,
            )));
        }
    };

    Ok(LumaTitle {
        id,
        name,
        asset: package.release_asset,
        addon_file: package.addon_file,
        arch: architecture,
        status,
        category: availability.map_or(LumaCategory::Installable, |value| match value {
            WireAvailability::Blocked { message } => LumaCategory::Blacklist {
                message: message.into(),
            },
        }),
        match_rules: r#match,
        features,
        guidance,
        launch_args: launch_arguments,
        external_requirement: managed_dependency,
        profile,
    })
}

fn guidance_from_wire_v2(
    guidance: Vec<WireGuidance>,
) -> Result<Vec<LumaGuidance>, crate::ServiceError> {
    guidance
        .into_iter()
        .map(
            |WireGuidance {
                 id,
                 kind,
                 fallback_text,
                 code,
                 engine_ini,
             }| {
                if matches!(kind, WireGuidanceKind::EngineIni) {
                    let code = code.as_deref().ok_or_else(|| {
                        crate::ServiceError::command_failed(format!(
                            "Luma v2 guidance `{id}` engine_ini kind must include code"
                        ))
                    })?;
                    let recipe = engine_ini.as_ref().ok_or_else(|| {
                        crate::ServiceError::command_failed(format!(
                            "Luma v2 guidance `{id}` engine_ini kind must include a typed recipe"
                        ))
                    })?;
                    let canonical_code = render_wire_engine_ini_recipe(recipe);
                    if code != canonical_code {
                        return Err(crate::ServiceError::command_failed(format!(
                            "Luma v2 guidance `{id}` code does not match canonical Engine.ini rendering"
                        )));
                    }
                } else if code.is_some() || engine_ini.is_some() {
                    return Err(crate::ServiceError::command_failed(format!(
                        "Luma v2 guidance `{id}` non-engine guidance must not include code or engine_ini"
                    )));
                }
                let kind = match kind {
                    WireGuidanceKind::GameSetting => LumaGuidanceKind::GameSetting,
                    WireGuidanceKind::EngineIni => LumaGuidanceKind::EngineIni,
                    WireGuidanceKind::Warning => LumaGuidanceKind::Warning,
                    WireGuidanceKind::Compatibility => LumaGuidanceKind::Compatibility,
                    WireGuidanceKind::ExternalTool => LumaGuidanceKind::ExternalTool,
                };
                let engine_ini = engine_ini
                    .map(|recipe| {
                        if recipe.id != id {
                            return Err(crate::ServiceError::command_failed(format!(
                                "Luma v2 guidance `{id}` engine_ini id `{}` does not match",
                                recipe.id
                            )));
                        }
                        let entries = recipe
                            .sections
                            .into_iter()
                            .flat_map(|section| {
                                section
                                    .entries
                                    .into_iter()
                                    .map(move |entry| EngineIniEntry {
                                        section: section.name.clone(),
                                        key: entry.key,
                                        value: entry.value,
                                    })
                            })
                            .collect();
                        Ok(EngineIniRecipe {
                            schema_version: recipe.schema_version,
                            revision: recipe.revision,
                            id: recipe.id,
                            entries,
                        })
                    })
                    .transpose()?;
                Ok(LumaGuidance {
                    id,
                    kind,
                    fallback_text,
                    code,
                    engine_ini,
                })
            },
        )
        .collect()
}

/// Renders the wire recipe exactly like the catalog producer. This adapter
/// check keeps display text from becoming a second mutation authority.
fn render_wire_engine_ini_recipe(recipe: &WireEngineIniRecipe) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    for (s_idx, section) in recipe.sections.iter().enumerate() {
        if s_idx > 0 {
            out.push_str("\n\n");
        }
        let _ = write!(out, "[{}]", section.name);
        for entry in &section.entries {
            let _ = write!(out, "\n{}={}", entry.key, entry.value);
        }
    }
    out
}
