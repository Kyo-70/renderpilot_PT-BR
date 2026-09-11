use std::collections::BTreeSet;

use renderpilot_domain::{
    AddonKind, NormalizedPathRelation, ProxyImplementation, normalized_path_key,
    normalized_path_relation,
};

use super::error::RenoDxActiveUninstallError;
use super::model::ActiveUninstallInput;

pub(super) fn validate_input(
    input: &ActiveUninstallInput<'_>,
) -> Result<(), RenoDxActiveUninstallError> {
    if input.record.kind() != AddonKind::RenoDx {
        return Err(RenoDxActiveUninstallError::Invalid("record is not RenoDX"));
    }
    if input.record.game_id() != &input.topology.game_id {
        return Err(RenoDxActiveUninstallError::Invalid(
            "record and topology belong to different games",
        ));
    }
    input
        .topology
        .validate()
        .map_err(|_| RenoDxActiveUninstallError::Invalid("active proxy topology is invalid"))?;
    if input.topology.outer.implementation != ProxyImplementation::OptiScaler {
        return Err(RenoDxActiveUninstallError::Invalid(
            "active topology outer is not OptiScaler",
        ));
    }
    if input
        .root
        .roots()
        .require_game_path(&input.topology.root_slot)
        .is_err()
    {
        return Err(RenoDxActiveUninstallError::Invalid(
            "topology root slot is outside the sealed game root",
        ));
    }
    if input.record.addon_file().file_name().is_none() {
        return Err(RenoDxActiveUninstallError::Invalid(
            "record add-on path has no file name",
        ));
    }

    let addon_key = normalized_path_key(input.record.addon_file().as_str());
    let mut created_keys = BTreeSet::new();
    for path in input.record.created_files() {
        if !created_keys.insert(normalized_path_key(path.as_str()))
            && normalized_path_key(path.as_str()) != addon_key
        {
            return Err(RenoDxActiveUninstallError::Invalid(
                "record contains a duplicate created path",
            ));
        }
    }
    if !created_keys.contains(&addon_key) {
        return Err(RenoDxActiveUninstallError::Invalid(
            "record add-on claim is not an exact created file",
        ));
    }

    let mut claimed_paths = BTreeSet::new();
    for path in input
        .record
        .created_files()
        .iter()
        .chain(input.record.backed_up_files())
    {
        if claimed_paths.insert(normalized_path_key(path.as_str()))
            && input.root.roots().require_sealed_path(path).is_err()
        {
            return Err(RenoDxActiveUninstallError::Path(std::path::PathBuf::from(
                path.as_str(),
            )));
        }
    }

    let created = input
        .record
        .created_files()
        .iter()
        .map(|path| normalized_path_key(path.as_str()))
        .collect::<BTreeSet<_>>();
    let mut backed = BTreeSet::new();
    for path in input.record.backed_up_files() {
        if !backed.insert(normalized_path_key(path.as_str())) {
            return Err(RenoDxActiveUninstallError::Invalid(
                "record contains a duplicate backed path",
            ));
        }
        if !created.contains(&normalized_path_key(path.as_str())) {
            return Err(RenoDxActiveUninstallError::Invalid(
                "backed claim has no live created claim",
            ));
        }
    }

    let mut endpoint_keys = BTreeSet::new();
    for endpoint in input.endpoints {
        if !endpoint_keys.insert(normalized_path_key(endpoint.path().as_str())) {
            return Err(RenoDxActiveUninstallError::Invalid(
                "active uninstall endpoint snapshots contain a duplicate path",
            ));
        }
    }
    ensure_no_overlaps(input.endpoints)?;

    let ini_path = renderpilot_domain::PathRef::new(
        input
            .root
            .config_source()
            .exact_ini_path()
            .to_string_lossy()
            .into_owned(),
    )
    .map_err(|_| RenoDxActiveUninstallError::Invalid("cannot form exact ReShade.ini path"))?;
    let mut expected_endpoint_keys = input
        .record
        .created_files()
        .iter()
        .filter(|path| {
            !matches!(
                normalized_path_relation(path.as_str(), ini_path.as_str()),
                NormalizedPathRelation::Equal
            )
        })
        .map(|path| normalized_path_key(path.as_str()))
        .collect::<BTreeSet<_>>();
    for managed in input.record.managed_files() {
        if input.record.created_files().iter().any(|path| {
            matches!(
                normalized_path_relation(path.as_str(), managed.path().as_str()),
                NormalizedPathRelation::Equal
            )
        }) || input.record.backed_up_files().iter().any(|path| {
            matches!(
                normalized_path_relation(path.as_str(), managed.path().as_str()),
                NormalizedPathRelation::Equal
            )
        }) {
            return Err(RenoDxActiveUninstallError::Invalid(
                "managed claim overlaps a generic record claim",
            ));
        }
        if input.topology.downstream.as_ref().is_some_and(|link| {
            matches!(
                normalized_path_relation(link.path.as_str(), managed.path().as_str()),
                NormalizedPathRelation::Equal
            )
        }) {
            expected_endpoint_keys.insert(normalized_path_key(managed.path().as_str()));
        } else {
            return Err(RenoDxActiveUninstallError::Invalid(
                "RenoDX managed claim does not match the active downstream",
            ));
        }
    }
    if endpoint_keys != expected_endpoint_keys {
        return Err(RenoDxActiveUninstallError::Invalid(
            "active uninstall endpoint snapshots do not exactly match record claims",
        ));
    }
    for path in input.record.created_files() {
        if matches!(
            normalized_path_relation(path.as_str(), ini_path.as_str()),
            NormalizedPathRelation::Equal
        ) {
            continue;
        }
        if !input.endpoints.iter().any(|endpoint| {
            matches!(
                normalized_path_relation(endpoint.path().as_str(), path.as_str()),
                NormalizedPathRelation::Equal
            )
        }) {
            return Err(RenoDxActiveUninstallError::Invalid(
                "created claim has no sealed endpoint snapshot",
            ));
        }
    }
    Ok(())
}

fn ensure_no_overlaps(
    endpoints: &[super::model::ActiveUninstallEndpointOwned],
) -> Result<(), RenoDxActiveUninstallError> {
    for (index, left) in endpoints.iter().enumerate() {
        for right in endpoints.iter().skip(index + 1) {
            if normalized_path_relation(left.path().as_str(), right.path().as_str()).overlaps() {
                return Err(RenoDxActiveUninstallError::Invalid(
                    "active uninstall endpoint snapshots contain overlapping paths",
                ));
            }
        }
    }
    Ok(())
}
