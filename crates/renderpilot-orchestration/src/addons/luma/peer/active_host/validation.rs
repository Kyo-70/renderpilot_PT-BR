use std::path::{Component, Path};

use renderpilot_domain::{Architecture, PathRef, normalized_path_key};

use crate::{
    addons::{
        luma::peer::{effects::ensure_bytes_match_image, root_authority::LumaPeerRootAuthority},
        reshade::{
            host_policy::TopologyHostAssessment,
            scan::{ReshadeAddonSupport, ReshadeIdentity},
        },
    },
    peer_mutation_executor::PeerPathSnapshot,
};

use super::model::ActiveHostClassificationError;

pub(super) fn exact_host_path(
    authority: &LumaPeerRootAuthority,
) -> Result<PathRef, ActiveHostClassificationError> {
    let root = Path::new(authority.canonical_game_root_ref().as_str());
    let path = root.join("ReShade64.dll");
    let path = PathRef::new(path.to_string_lossy().into_owned()).map_err(|_| {
        ActiveHostClassificationError::Assessment("canonical game root cannot form ReShade64.dll")
    })?;
    if path
        .as_str()
        .split(['/', '\\'])
        .any(|component| component == "." || component == "..")
    {
        return Err(ActiveHostClassificationError::Assessment(
            "canonical game root produced a non-canonical host path",
        ));
    }
    Ok(path)
}

pub(super) fn require_exact_path(
    expected: &PathRef,
    observed: &PathRef,
) -> Result<(), ActiveHostClassificationError> {
    if normalized_path_key(expected.as_str()) != normalized_path_key(observed.as_str()) {
        return Err(ActiveHostClassificationError::Path {
            expected: expected.clone(),
            observed: observed.clone(),
        });
    }
    if Path::new(observed.as_str())
        .components()
        .any(|component| matches!(component, Component::CurDir | Component::ParentDir))
    {
        return Err(ActiveHostClassificationError::Assessment(
            "active host path is not structurally canonical",
        ));
    }
    Ok(())
}

pub(super) fn validate_assessment_path(
    assessment: &TopologyHostAssessment,
    expected: &PathRef,
) -> Result<(), ActiveHostClassificationError> {
    let assessed_present = assessment.assessment().host.as_present().is_some();
    if assessed_present != assessment.snapshot().present
        || assessment.snapshot().lifecycle != assessment.assessment().lifecycle
        || assessment.snapshot().action != assessment.assessment().action
        || assessment.snapshot().requires_host_download
            != assessment.assessment().initial_writes_host()
    {
        return Err(ActiveHostClassificationError::Assessment(
            "assessment snapshot is inconsistent with its host policy result",
        ));
    }
    if normalized_path_key(&assessment.snapshot().exact_path)
        != normalized_path_key(expected.as_str())
    {
        return Err(ActiveHostClassificationError::Assessment(
            "assessment exact path differs from the sealed game-root host",
        ));
    }
    let host = &assessment.assessment().host;
    if let Some(host) = host.as_present()
        && normalized_path_key(&host.path.to_string_lossy())
            != normalized_path_key(expected.as_str())
    {
        return Err(ActiveHostClassificationError::Assessment(
            "assessment host path differs from the sealed game-root host",
        ));
    }
    Ok(())
}

pub(super) fn correlate_live_evidence(
    assessment: &TopologyHostAssessment,
    path: &PathRef,
    snapshot: &PeerPathSnapshot,
) -> Result<(), ActiveHostClassificationError> {
    let expected = assessment.snapshot();
    match (expected.present, snapshot.file()) {
        (false, None) => {}
        (true, Some(file)) => {
            if expected.digest.as_ref() != Some(file.digest())
                || expected.length != Some(file.length())
            {
                return Err(ActiveHostClassificationError::Evidence {
                    path: path.clone(),
                    detail: "retained digest or length differs from the topology assessment",
                });
            }
            let bytes =
                snapshot
                    .bytes()
                    .ok_or_else(|| ActiveHostClassificationError::Evidence {
                        path: path.clone(),
                        detail: "present host snapshot has no retained bytes",
                    })?;
            ensure_bytes_match_image(path, bytes, file, false).map_err(|_| {
                ActiveHostClassificationError::Evidence {
                    path: path.clone(),
                    detail: "retained host bytes do not match their digest and length",
                }
            })?;
            let facts = host_facts(bytes);
            if expected.identity != Some(facts.identity)
                || expected.addon_support != Some(facts.addon_support)
                || expected.version != facts.version
            {
                return Err(ActiveHostClassificationError::Evidence {
                    path: path.clone(),
                    detail: "retained host PE facts differ from the topology assessment",
                });
            }
        }
        (false, Some(_)) | (true, None) => {
            return Err(ActiveHostClassificationError::Evidence {
                path: path.clone(),
                detail: "assessment presence differs from the retained host snapshot",
            });
        }
    }
    Ok(())
}

pub(crate) fn validate_prepared_host<'a>(
    prepared_bytes: Option<&'a [u8]>,
    minimum_version: &renderpilot_domain::Version,
) -> Result<&'a [u8], ActiveHostClassificationError> {
    let Some(bytes) = prepared_bytes else {
        return Err(ActiveHostClassificationError::Prepared(
            "the selected host transition requires prepared bytes",
        ));
    };
    let inspection = renderpilot_detection::inspect_pe_bytes(bytes);
    if inspection.architecture.is_none() || inspection.export_names.is_none() {
        return Err(ActiveHostClassificationError::Prepared(
            "prepared bytes are not a readable PE image",
        ));
    }
    if inspection.architecture != Some(Architecture::X64) {
        return Err(ActiveHostClassificationError::Prepared(
            "prepared bytes are not a 64-bit ReShade image for ReShade64.dll",
        ));
    }
    let has_reshade_export = inspection
        .export_names
        .as_deref()
        .unwrap_or(&[])
        .iter()
        .any(|name| name.eq_ignore_ascii_case("ReShadeVersion"));
    if !has_reshade_export
        && !crate::addons::reshade::scan::version_strings_point_to_reshade(&inspection.identity)
    {
        return Err(ActiveHostClassificationError::Prepared(
            "prepared PE has no ReShade identity evidence",
        ));
    }
    if crate::addons::reshade::scan::is_known_custom_identity(&inspection.identity) {
        return Err(ActiveHostClassificationError::Prepared(
            "prepared PE identifies a custom ReShade fork",
        ));
    }
    let addon_support = crate::addons::reshade::scan::addon_support_from_exports_for_topology(
        inspection.export_names.as_deref(),
        has_reshade_export,
    );
    if addon_support != ReshadeAddonSupport::Full {
        return Err(ActiveHostClassificationError::Prepared(
            "prepared PE does not expose the full ReShade add-on API",
        ));
    }
    let version = inspection
        .version
        .ok_or(ActiveHostClassificationError::Prepared(
            "prepared PE has no readable ReShade version",
        ))?;
    if version < *minimum_version {
        return Err(ActiveHostClassificationError::PreparedDetail(format!(
            "prepared ReShade version {version} is below required {minimum_version}"
        )));
    }
    Ok(bytes)
}

pub(super) fn host_facts(bytes: &[u8]) -> HostFacts {
    let inspection = renderpilot_detection::inspect_pe_bytes(bytes);
    let has_reshade_export = inspection
        .export_names
        .as_deref()
        .unwrap_or(&[])
        .iter()
        .any(|name| name.eq_ignore_ascii_case("ReShadeVersion"));
    let identity = if has_reshade_export {
        ReshadeIdentity::Confirmed
    } else if crate::addons::reshade::scan::version_strings_point_to_reshade(&inspection.identity) {
        ReshadeIdentity::Probable
    } else {
        ReshadeIdentity::Weak
    };
    HostFacts {
        identity,
        addon_support: crate::addons::reshade::scan::addon_support_from_exports_for_topology(
            inspection.export_names.as_deref(),
            has_reshade_export,
        ),
        version: inspection.version,
    }
}

pub(super) struct HostFacts {
    pub(super) identity: ReshadeIdentity,
    pub(super) addon_support: ReshadeAddonSupport,
    pub(super) version: Option<renderpilot_domain::Version>,
}

pub(crate) fn digest(
    bytes: &[u8],
) -> Result<renderpilot_domain::Sha256Hash, ActiveHostClassificationError> {
    renderpilot_detection::sha256_bytes(bytes)
        .map_err(|error| ActiveHostClassificationError::PreparedDetail(error.to_string()))
}
