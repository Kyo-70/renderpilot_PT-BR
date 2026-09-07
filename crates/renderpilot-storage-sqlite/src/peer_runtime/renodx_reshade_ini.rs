//! Storage-bound binding for the typed RenoDX ReShade.ini endpoint.

use renderpilot_application::{AppError, AppResult};
use renderpilot_domain::{PathRef, RenoDxReshadeIniAuthority};

use super::manifest::ParsedPeerProgram;

pub(super) fn bind_preparation(
    feature: &str,
    canonical_game_root: &PathRef,
    program: &ParsedPeerProgram,
    supplied: Option<&RenoDxReshadeIniAuthority>,
) -> AppResult<Option<RenoDxReshadeIniAuthority>> {
    let Some(intent) = program.renodx_reshade_ini_intent() else {
        if supplied.is_some() {
            return Err(invalid("authority was supplied without a typed endpoint"));
        }
        return Ok(None);
    };
    let Some(supplied) = supplied else {
        return Err(invalid(
            "typed endpoint requires RenoDX ReShade.ini authority",
        ));
    };
    let expected = reconstruct(feature, canonical_game_root)?;
    if supplied != &expected {
        return Err(invalid(
            "supplied RenoDX ReShade.ini authority differs from feature/root",
        ));
    }
    if !expected.matches_ini_path(intent.path()) {
        return Err(invalid(
            "typed endpoint path differs from the feature/root-derived ReShade.ini path",
        ));
    }
    Ok(Some(expected))
}

pub(super) fn bind_recovery(
    feature: &str,
    canonical_game_root: &PathRef,
    program: &ParsedPeerProgram,
) -> AppResult<Option<RenoDxReshadeIniAuthority>> {
    let Some(intent) = program.renodx_reshade_ini_intent() else {
        return Ok(None);
    };
    let expected = reconstruct(feature, canonical_game_root)?;
    if !expected.matches_ini_path(intent.path()) {
        return Err(invalid(
            "durable typed endpoint path differs from the feature/root-derived ReShade.ini path",
        ));
    }
    Ok(Some(expected))
}

fn reconstruct(
    feature: &str,
    canonical_game_root: &PathRef,
) -> AppResult<RenoDxReshadeIniAuthority> {
    RenoDxReshadeIniAuthority::try_from_feature(feature, canonical_game_root.clone()).map_err(
        |error| {
            invalid(format!(
                "cannot reconstruct RenoDX ReShade.ini authority: {error}"
            ))
        },
    )
}

fn invalid(message: impl Into<String>) -> AppError {
    AppError::invalid_input(format!("RenoDX ReShade.ini authority: {}", message.into()))
}
