//! Specialized storage binding for the typed RenoDX DLSS-Fix projection.

use renderpilot_application::{AppError, AppResult};
use renderpilot_domain::{
    PeerEndpointIntent, PeerEndpointOperation, PeerEndpointRole, PeerFileImage,
    PeerReadGuardEvidence, PeerReadGuardRequirement, PeerReadGuardSource, RenoDxDlssBeforeImage,
    RenoDxDlssProjection, normalized_path_key,
};

use super::{is_strict_descendant, strict_absolute_path};
use crate::peer_runtime::manifest::sha256_bytes;

/// Binds the typed companion preimage to the exact endpoint or read-guard
/// observation which carries it. The domain guard projection intentionally
/// checks identity and digest; storage also binds the declared byte length and
/// retained bytes so those three fields cannot be substituted independently.
pub(super) fn validate_projection_binding(
    projection: &RenoDxDlssProjection,
    intents: &[PeerEndpointIntent],
    program_before: &[Option<PeerFileImage>],
    requirements: &[PeerReadGuardRequirement],
    evidence: &[PeerReadGuardEvidence],
) -> AppResult<()> {
    validate_program_shape(intents, projection)?;
    validate_projection_bytes(projection)?;

    let companion = projection.companion_path();
    let companion_key = normalized_path_key(companion.as_str());
    if let Some(ordinal) = intents
        .iter()
        .position(|intent| normalized_path_key(intent.path().as_str()) == companion_key)
    {
        let observed = program_before.get(ordinal).ok_or_else(|| {
            AppError::storage_failed("RenoDX DLSS companion endpoint preimage is missing")
        })?;
        return validate_image(
            projection.before_image(),
            observed.as_ref(),
            "RenoDX DLSS companion endpoint preimage",
        );
    }

    let ordinal = requirements
        .iter()
        .position(|requirement| {
            normalized_path_key(requirement.path().as_str()) == companion_key
                && requirement
                    .sources()
                    .contains(&PeerReadGuardSource::RenoDxDlssCompanion)
        })
        .ok_or_else(|| {
            AppError::storage_failed("RenoDX DLSS companion preimage guard is missing")
        })?;
    let observed = evidence
        .get(ordinal)
        .and_then(PeerReadGuardEvidence::observed);
    validate_image(
        projection.before_image(),
        observed,
        "RenoDX DLSS companion read-guard preimage",
    )
}

/// Validates the typed companion path and any physical endpoint preimage
/// against the specialized manifest before peer records are available. The
/// peer claim slots and non-physical live guard are bound later during
/// preparation, when their exact records and evidence are present.
pub(super) fn validate_projection_manifest_binding(
    sealed_roots: &[String],
    intents: &[PeerEndpointIntent],
    program_before: &[Option<PeerFileImage>],
    projection: &RenoDxDlssProjection,
) -> AppResult<()> {
    validate_projection_bytes(projection)?;
    validate_program_shape(intents, projection)?;
    let companion = strict_absolute_path(
        projection.companion_path().as_str(),
        "RenoDX DLSS companion path",
    )?;
    let sealed_roots = sealed_roots
        .iter()
        .map(|root| strict_absolute_path(root, "sealed game root"))
        .collect::<AppResult<Vec<_>>>()?;
    if !sealed_roots
        .iter()
        .any(|root| is_strict_descendant(root.as_str(), companion.as_str()))
    {
        return Err(AppError::storage_failed(
            "RenoDX DLSS companion path is not below a sealed peer root",
        ));
    }

    let companion_key = normalized_path_key(companion.as_str());
    if let Some(ordinal) = intents
        .iter()
        .position(|intent| normalized_path_key(intent.path().as_str()) == companion_key)
    {
        let observed = program_before.get(ordinal).ok_or_else(|| {
            AppError::storage_failed("RenoDX DLSS companion endpoint preimage is missing")
        })?;
        validate_image(
            projection.before_image(),
            observed.as_ref(),
            "RenoDX DLSS companion endpoint preimage",
        )?;
    }
    Ok(())
}

fn validate_program_shape(
    intents: &[PeerEndpointIntent],
    projection: &RenoDxDlssProjection,
) -> AppResult<()> {
    if intents.len() > 2 {
        return Err(AppError::invalid_input(
            "RenoDX DLSS projection permits at most a companion and typed INI endpoint",
        ));
    }
    let companion_key = normalized_path_key(projection.companion_path().as_str());
    let companion_ordinal = intents
        .iter()
        .position(|intent| normalized_path_key(intent.path().as_str()) == companion_key);
    if let Some(ordinal) = companion_ordinal {
        if ordinal != 0 {
            return Err(AppError::invalid_input(
                "RenoDX DLSS companion endpoint must be ordinal zero",
            ));
        }
        let companion = &intents[ordinal];
        if companion.role() != PeerEndpointRole::Disjoint {
            return Err(AppError::invalid_input(
                "RenoDX DLSS companion endpoint must be disjoint",
            ));
        }
        let operation_matches_claims = match companion.operation() {
            PeerEndpointOperation::Create | PeerEndpointOperation::Replace => {
                projection.after_claim().created()
            }
            PeerEndpointOperation::Remove => {
                projection.before_claim().created() && !projection.after_claim().created()
            }
        };
        if !operation_matches_claims {
            return Err(AppError::invalid_input(
                "RenoDX DLSS companion operation differs from its claim transition",
            ));
        }
    }
    for (ordinal, intent) in intents.iter().enumerate() {
        let is_companion = companion_ordinal == Some(ordinal);
        if is_companion {
            continue;
        }
        let expected_ordinal = usize::from(companion_ordinal.is_some());
        if intent.role() != PeerEndpointRole::RenoDxReshadeIni || ordinal != expected_ordinal {
            return Err(AppError::invalid_input(
                "RenoDX DLSS projection permits only the companion followed by typed INI",
            ));
        }
    }
    Ok(())
}

fn validate_projection_bytes(projection: &RenoDxDlssProjection) -> AppResult<()> {
    if let RenoDxDlssBeforeImage::Present {
        sha256,
        length,
        bytes,
        ..
    } = projection.before_image()
        && (bytes.len() as u64 != *length || sha256_bytes(bytes) != sha256_bytes_from_hash(sha256))
    {
        return Err(AppError::invalid_input(
            "RenoDX DLSS companion preimage bytes do not match digest and length",
        ));
    }
    Ok(())
}

fn validate_image(
    expected: &RenoDxDlssBeforeImage,
    observed: Option<&PeerFileImage>,
    context: &str,
) -> AppResult<()> {
    match (expected, observed) {
        (RenoDxDlssBeforeImage::Absent, None) => Ok(()),
        (
            RenoDxDlssBeforeImage::Present {
                identity,
                sha256,
                length,
                ..
            },
            Some(observed),
        ) if observed.identity() == identity
            && observed.sha256() == sha256
            && observed.length() == *length =>
        {
            Ok(())
        }
        (RenoDxDlssBeforeImage::Absent, Some(_)) => {
            Err(AppError::invalid_input(format!("{context} must be absent")))
        }
        (RenoDxDlssBeforeImage::Present { .. }, None) => {
            Err(AppError::invalid_input(format!("{context} is missing")))
        }
        (RenoDxDlssBeforeImage::Present { .. }, Some(_)) => Err(AppError::invalid_input(format!(
            "{context} differs from the typed projection"
        ))),
    }
}

fn sha256_bytes_from_hash(hash: &renderpilot_domain::Sha256Hash) -> [u8; 32] {
    let mut bytes = [0; 32];
    let (pairs, remainder) = hash.as_str().as_bytes().as_chunks::<2>();
    debug_assert!(remainder.is_empty());
    for (index, pair) in pairs.iter().enumerate() {
        bytes[index] = (hex_nibble(pair[0]) << 4) | hex_nibble(pair[1]);
    }
    bytes
}

fn hex_nibble(value: u8) -> u8 {
    match value {
        b'0'..=b'9' => value - b'0',
        b'a'..=b'f' => value - b'a' + 10,
        b'A'..=b'F' => value - b'A' + 10,
        _ => 0,
    }
}
