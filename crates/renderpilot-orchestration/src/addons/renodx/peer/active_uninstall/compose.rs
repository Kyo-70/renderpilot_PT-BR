use renderpilot_domain::{
    NormalizedPathRelation, PeerEndpointRole, managed_sidecar_path, normalized_path_key,
    normalized_path_relation,
};

use crate::peer_mutation_executor::ExactEndpointProgram;

use super::effects::{
    ActiveUninstallEffects, emit_remove, emit_replace, find_endpoint, present_bytes, present_image,
};
use super::error::RenoDxActiveUninstallError;
use super::host::compose_host;
use super::ini::compose_ini;
use super::model::{ActiveUninstallComposition, ActiveUninstallInput};
use super::validation::validate_input;

pub(crate) fn compose_active_uninstall(
    input: ActiveUninstallInput<'_>,
) -> Result<ActiveUninstallComposition, RenoDxActiveUninstallError> {
    validate_input(&input)?;
    let ini_path = renderpilot_domain::PathRef::new(
        input
            .root
            .config_source()
            .exact_ini_path()
            .to_string_lossy()
            .into_owned(),
    )
    .map_err(|_| RenoDxActiveUninstallError::Invalid("cannot form exact ReShade.ini path"))?;
    let mut effects = ActiveUninstallEffects::new();

    let mut created = input
        .record
        .created_files()
        .iter()
        .filter(|path| {
            !matches!(
                normalized_path_relation(path.as_str(), ini_path.as_str()),
                NormalizedPathRelation::Equal
            )
        })
        .filter(|path| {
            !input.topology.downstream.as_ref().is_some_and(|link| {
                matches!(
                    normalized_path_relation(path.as_str(), link.path.as_str()),
                    NormalizedPathRelation::Equal
                )
            })
        })
        .filter(|path| {
            !input.record.backed_up_files().iter().any(|backed| {
                matches!(
                    normalized_path_relation(path.as_str(), backed.as_str()),
                    NormalizedPathRelation::Equal
                )
            })
        })
        .cloned()
        .collect::<Vec<_>>();
    created.sort_by_key(|path| normalized_path_key(path.as_str()));
    created.dedup_by(|left, right| {
        matches!(
            normalized_path_relation(left.as_str(), right.as_str()),
            NormalizedPathRelation::Equal
        )
    });
    for path in created {
        let endpoint = find_endpoint(input.endpoints, &path)?;
        let before_bytes = present_bytes(endpoint.path(), endpoint.snapshot())?;
        let before = present_image(endpoint.path(), endpoint.snapshot())?;
        emit_remove(
            endpoint.path(),
            PeerEndpointRole::Disjoint,
            before,
            before_bytes,
            &mut effects,
        )?;
    }

    let mut backed = input.record.backed_up_files().to_vec();
    backed.sort_by_key(|path| normalized_path_key(path.as_str()));
    for path in backed {
        if matches!(
            normalized_path_relation(path.as_str(), ini_path.as_str()),
            NormalizedPathRelation::Equal
        ) {
            continue;
        }
        if input.topology.downstream.as_ref().is_some_and(|link| {
            matches!(
                normalized_path_relation(path.as_str(), link.path.as_str()),
                NormalizedPathRelation::Equal
            )
        }) {
            return Err(RenoDxActiveUninstallError::Invalid(
                "managed host cannot also be a generic backed claim",
            ));
        }
        let endpoint = find_endpoint(input.endpoints, &path)?;
        let before_bytes = present_bytes(endpoint.path(), endpoint.snapshot())?;
        let before = present_image(endpoint.path(), endpoint.snapshot())?;
        let backup = endpoint
            .backup()
            .ok_or(RenoDxActiveUninstallError::Invalid(
                "backed claim is missing its sealed sidecar snapshot",
            ))?;
        let restored = present_bytes(backup.path(), backup.snapshot())?;
        let baseline = present_image(backup.path(), backup.snapshot())?;
        if input.record.managed_files().iter().any(|file| {
            matches!(
                normalized_path_relation(file.path().as_str(), path.as_str()),
                NormalizedPathRelation::Equal
            )
        }) {
            return Err(RenoDxActiveUninstallError::Invalid(
                "generic backed claim overlaps a managed claim",
            ));
        }
        emit_replace(
            endpoint.path(),
            PeerEndpointRole::Disjoint,
            before,
            before_bytes,
            &baseline,
            restored,
            &mut effects,
        )?;
        let sidecar = managed_sidecar_path(endpoint.path()).map_err(|_| {
            RenoDxActiveUninstallError::Path(std::path::PathBuf::from(endpoint.path().as_str()))
        })?;
        emit_remove(
            &sidecar,
            PeerEndpointRole::Disjoint,
            present_image(backup.path(), backup.snapshot())?,
            present_bytes(backup.path(), backup.snapshot())?,
            &mut effects,
        )?;
    }

    let planned_topology = compose_host(&input, &mut effects)?;
    let ini_authority = compose_ini(&input, &mut effects)?;
    for (index, left) in effects.endpoints().iter().enumerate() {
        if effects.endpoints().iter().skip(index + 1).any(|right| {
            normalized_path_relation(left.path().as_str(), right.path().as_str()).overlaps()
        }) {
            return Err(RenoDxActiveUninstallError::Invalid(
                "active uninstall endpoint paths overlap",
            ));
        }
    }

    let (endpoints, payloads, intents) = effects.into_parts();
    let program = ExactEndpointProgram::new(endpoints).map_err(|_| {
        RenoDxActiveUninstallError::Invalid("active uninstall produced an invalid endpoint program")
    })?;
    if payloads.len() != program.endpoints().len() || intents.len() != program.endpoints().len() {
        return Err(RenoDxActiveUninstallError::Invalid(
            "active uninstall endpoint payload alignment changed",
        ));
    }
    Ok(ActiveUninstallComposition {
        program,
        payloads,
        game_intents: intents,
        planned_topology,
        reshade_ini_authority: ini_authority,
    })
}
