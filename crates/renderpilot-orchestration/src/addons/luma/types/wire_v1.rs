//! Public v1 Luma document adapter → internal catalogue model.

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

/// Public v1 Luma document. The nested wire model keeps curation concepts
/// explicit without forcing install code to know about JSON presentation.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WireManifestV1 {
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

/// Guidance accepted by the released v1 wire contract.  Launch arguments are
/// normalized into `requirements.launch_arguments` before the runtime model is
/// built, so current UI guidance has no launch-argument variant.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct WireGuidance {
    pub id: String,
    pub kind: WireGuidanceKind,
    pub fallback_text: String,
    #[serde(default)]
    pub code: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum WireGuidanceKind {
    GameSetting,
    EngineIni,
    LaunchArgument,
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
    /// Converts a schema-v1 document to the internal installation model.
    /// The application accepts the released v1 format only; historical shapes
    /// within that format normalize at this ingress boundary.
    pub(crate) fn from_wire_v1(wire: WireManifestV1) -> Result<Self, crate::ServiceError> {
        Ok(Self {
            schema_version: wire.schema_version,
            generated_at: wire.generated_at,
            min_reshade_version: wire.minimum_reshade_version,
            titles: wire
                .games
                .into_iter()
                .map(title_from_wire_v1)
                .collect::<Result<Vec<_>, _>>()?,
        })
    }
}

fn title_from_wire_v1(game: WireGame) -> Result<LumaTitle, crate::ServiceError> {
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
    let (guidance, launch_args) = normalize_wire_guidance(guidance, launch_arguments)?;
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
                "Luma v1 profile `{id}` has incompatible {incompatible:?} payload `{}` for {architecture:?}",
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
        launch_args,
        external_requirement: managed_dependency,
        profile,
    })
}

fn normalize_wire_guidance(
    guidance: Vec<WireGuidance>,
    declared_launch_args: Vec<String>,
) -> Result<(Vec<LumaGuidance>, Vec<String>), crate::ServiceError> {
    let mut launch_args = Vec::with_capacity(declared_launch_args.len());
    for argument in declared_launch_args {
        append_launch_argument(&mut launch_args, argument);
    }

    let mut runtime_guidance = Vec::with_capacity(guidance.len());
    for WireGuidance {
        id,
        kind,
        fallback_text,
        code,
    } in guidance
    {
        let kind = match kind {
            WireGuidanceKind::LaunchArgument => {
                if id.trim().is_empty() {
                    return Err(crate::ServiceError::command_failed(
                        "Luma v1 launch-argument guidance must include a non-blank id",
                    ));
                }
                if fallback_text.trim().is_empty() {
                    return Err(crate::ServiceError::command_failed(format!(
                        "Luma v1 launch-argument guidance `{id}` must include non-blank fallback_text"
                    )));
                }
                let code = code.ok_or_else(|| {
                    crate::ServiceError::command_failed(format!(
                        "Luma v1 launch-argument guidance `{id}` must include code"
                    ))
                })?;
                if code.trim().is_empty() {
                    return Err(crate::ServiceError::command_failed(format!(
                        "Luma v1 launch-argument guidance `{id}` must include non-blank code"
                    )));
                }
                append_launch_argument(&mut launch_args, code);
                continue;
            }
            WireGuidanceKind::GameSetting => LumaGuidanceKind::GameSetting,
            WireGuidanceKind::EngineIni => LumaGuidanceKind::EngineIni,
            WireGuidanceKind::Warning => LumaGuidanceKind::Warning,
            WireGuidanceKind::Compatibility => LumaGuidanceKind::Compatibility,
            WireGuidanceKind::ExternalTool => LumaGuidanceKind::ExternalTool,
        };
        runtime_guidance.push(LumaGuidance {
            id,
            kind,
            fallback_text,
            code,
        });
    }

    Ok((runtime_guidance, launch_args))
}

fn append_launch_argument(arguments: &mut Vec<String>, argument: String) {
    if is_standalone_dx11_argument(&argument) {
        if arguments
            .iter()
            .any(|value| contains_dx11_argument_token(value))
        {
            return;
        }
        arguments.push("-dx11".to_owned());
        return;
    }

    if contains_dx11_argument_token(&argument) {
        arguments.retain(|value| !is_standalone_dx11_argument(value));
    }
    arguments.push(argument);
}

fn is_standalone_dx11_argument(argument: &str) -> bool {
    argument.trim().eq_ignore_ascii_case("-dx11")
}

fn contains_dx11_argument_token(argument: &str) -> bool {
    argument
        .split_ascii_whitespace()
        .any(|token| token.eq_ignore_ascii_case("-dx11"))
}
