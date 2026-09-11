use renderpilot_domain::{
    NormalizedPathRelation, PeerEndpointRole, RenoDxReshadeIniAuthority, RenoDxReshadeIniFeature,
    normalized_path_relation,
};

use super::effects::{
    ActiveUninstallEffects, emit_remove, emit_replace, image_for_bytes, present_bytes,
    present_image,
};
use super::error::RenoDxActiveUninstallError;
use super::model::ActiveUninstallInput;
use crate::addons::renodx::reshade_ini::ini_remove_renodx_strategy;

pub(super) fn compose_ini(
    input: &ActiveUninstallInput<'_>,
    effects: &mut ActiveUninstallEffects,
) -> Result<Option<RenoDxReshadeIniAuthority>, RenoDxActiveUninstallError> {
    let ini_path = input.root.config_source().exact_ini_path();
    let ini_ref = renderpilot_domain::PathRef::new(ini_path.to_string_lossy().into_owned())
        .map_err(|_| RenoDxActiveUninstallError::Path(ini_path.to_path_buf()))?;
    let created = input.record.created_files().iter().any(|path| {
        matches!(
            normalized_path_relation(path.as_str(), ini_ref.as_str()),
            NormalizedPathRelation::Equal
        )
    });
    let backed = input.record.backed_up_files().iter().any(|path| {
        matches!(
            normalized_path_relation(path.as_str(), ini_ref.as_str()),
            NormalizedPathRelation::Equal
        )
    });
    let named_claim = input
        .record
        .created_files()
        .iter()
        .chain(input.record.backed_up_files())
        .any(|path| {
            path.file_name()
                .is_some_and(|name| name.eq_ignore_ascii_case("ReShade.ini"))
                && !matches!(
                    normalized_path_relation(path.as_str(), ini_ref.as_str()),
                    NormalizedPathRelation::Equal
                )
        });
    if named_claim || backed && !created {
        return Err(RenoDxActiveUninstallError::Invalid(
            "ReShade.ini claim is not exact and coherent",
        ));
    }
    let current_bytes = input.ini_snapshot.bytes();
    let transformed = current_bytes.map(|bytes| {
        ini_remove_renodx_strategy()
            .apply(&String::from_utf8_lossy(bytes))
            .into_bytes()
    });
    let should_remove = created && !backed;
    let should_replace = (created && backed)
        || (!created
            && !backed
            && transformed
                .as_ref()
                .is_some_and(|after| current_bytes != Some(after.as_slice())));
    if (created || backed) && current_bytes.is_none() {
        return Err(RenoDxActiveUninstallError::Invalid(
            "claimed ReShade.ini is missing",
        ));
    }
    if !should_remove && !should_replace {
        return Ok(None);
    }
    let authority = RenoDxReshadeIniAuthority::new(
        RenoDxReshadeIniFeature::Uninstall,
        input.root.canonical_game_root_ref().clone(),
    )
    .map_err(|_| RenoDxActiveUninstallError::Invalid("cannot build exact ReShade.ini authority"))?;
    let before_bytes = present_bytes(&ini_ref, input.ini_snapshot)?;
    let before = present_image(&ini_ref, input.ini_snapshot)?;
    if should_remove {
        emit_remove(
            &ini_ref,
            PeerEndpointRole::RenoDxReshadeIni,
            before,
            before_bytes,
            effects,
        )?;
    } else {
        let after = transformed.ok_or(RenoDxActiveUninstallError::Invalid(
            "ReShade.ini transform has no bytes",
        ))?;
        let after_image = image_for_bytes(&after)?;
        emit_replace(
            &ini_ref,
            PeerEndpointRole::RenoDxReshadeIni,
            before,
            before_bytes,
            &after_image,
            after,
            effects,
        )?;
    }
    Ok(Some(authority))
}
