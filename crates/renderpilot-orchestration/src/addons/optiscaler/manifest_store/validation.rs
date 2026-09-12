//! Strict semantic validation for decoded OptiScaler manifests.

use super::*;

pub(in crate::addons::optiscaler) fn validate(
    manifest: &WireOptiScalerManifest,
) -> Result<(), ServiceError> {
    if manifest.schema_version != 1 || !valid_revision(&manifest.revision) {
        return Err(failed("unsupported or unversioned OptiScaler manifest"));
    }
    let modules: HashMap<_, _> = manifest
        .modules
        .iter()
        .map(|module| (module.id.as_str(), module))
        .collect();
    if modules.len() != manifest.modules.len() || !modules.contains_key("core") {
        return Err(failed(
            "OptiScaler module ids must be unique and include core",
        ));
    }
    for module in &manifest.modules {
        if !stable_module_id(&module.id) || module.description.is_empty() {
            return Err(failed(format!(
                "invalid OptiScaler module id {}",
                module.id
            )));
        }
        let requires = module.requires.iter().collect::<HashSet<_>>();
        let conflicts = module.conflicts.iter().collect::<HashSet<_>>();
        if requires.len() != module.requires.len()
            || conflicts.len() != module.conflicts.len()
            || requires.contains(&module.id)
            || conflicts.contains(&module.id)
            || requires
                .iter()
                .any(|dependency| conflicts.contains(dependency))
        {
            return Err(failed(format!(
                "OptiScaler module {} has duplicate or contradictory relationships",
                module.id
            )));
        }
        for dependency in module.requires.iter().chain(&module.conflicts) {
            if !modules.contains_key(dependency.as_str()) {
                return Err(failed(format!(
                    "OptiScaler module {} references unknown module {dependency}",
                    module.id
                )));
            }
        }
        for conflict in &module.conflicts {
            if !modules[conflict.as_str()]
                .conflicts
                .iter()
                .any(|peer| peer == &module.id)
            {
                return Err(failed(format!(
                    "OptiScaler module conflict {} <-> {conflict} must be symmetric",
                    module.id
                )));
            }
        }
        ensure_acyclic(module.id.as_str(), &modules, &mut Vec::new())?;
        if let Some(artifact) = &module.artifact {
            if artifact.id.trim().is_empty()
                || artifact.id.contains(['\r', '\n', '\0'])
                || artifact.size == 0
                || artifact.size > MAX_MODULE_ARTIFACT_SIZE
                || !is_sha256(&artifact.sha256)
                || !safe_relative(&artifact.target)
                || artifact.target == "$proxy"
                || !artifact.pe_x64
            {
                return Err(failed(format!(
                    "invalid pinned artifact for OptiScaler module {}",
                    module.id
                )));
            }
            super::super::source::validate_module_artifact_source(&module.id, &artifact.source)?;
        }
    }

    let mut release_ids = HashSet::new();
    for release in &manifest.releases {
        if !stable_release_id(&release.id)
            || !release_ids.insert(release.id.to_ascii_lowercase())
            || release.archive_size == 0
            || release.archive_size > MAX_MANIFEST_ARCHIVE_SIZE
            || release.config_schema == 0
            || !is_sha256(&release.archive_sha256)
        {
            return Err(failed(format!("invalid OptiScaler release {}", release.id)));
        }
        super::super::source::validate_release_source(release)?;
        let mut members = HashSet::new();
        let mut targets = HashSet::new();
        let mut proxy_member = false;
        for member in &release.members {
            if !safe_relative(&member.archive_path)
                || member.size == 0
                || !is_sha256(&member.sha256)
                || !modules.contains_key(member.module.as_str())
                || !members.insert(normalized_relative_key(&member.archive_path))
            {
                return Err(failed(format!(
                    "invalid archive member {} in {}",
                    member.archive_path, release.id
                )));
            }
            let target = member.target.as_str();
            if target == "$proxy" {
                if proxy_member || member.module != "core" {
                    return Err(failed(format!("invalid proxy target in {}", release.id)));
                }
                proxy_member = true;
            } else if !safe_relative(target) || !targets.insert(normalized_relative_key(target)) {
                return Err(failed(format!("invalid or duplicate target {target}")));
            }
        }
        if !proxy_member
            || !release
                .members
                .iter()
                .any(|member| member.module == "core" && member.target == "OptiScaler.ini")
        {
            return Err(failed(format!(
                "release {} must contain one core proxy and OptiScaler.ini",
                release.id
            )));
        }
        for module in &manifest.modules {
            if let Some(artifact) = &module.artifact
                && !release
                    .members
                    .iter()
                    .any(|member| member.module == module.id)
                && targets.contains(&normalized_relative_key(&artifact.target))
            {
                return Err(failed(format!(
                    "module artifact target {} collides with release {} layout",
                    artifact.target, release.id
                )));
            }
        }
    }
    if manifest.current_release.trim().is_empty() || manifest.current_release().is_none() {
        return Err(failed(
            "OptiScaler current_release does not name a known stable release",
        ));
    }
    validate_config_migrations(manifest)?;
    Ok(())
}

fn validate_config_migrations(manifest: &WireOptiScalerManifest) -> Result<(), ServiceError> {
    let schemas = manifest
        .releases
        .iter()
        .map(|release| release.config_schema)
        .collect::<HashSet<_>>();
    let mut keys = HashSet::new();
    for migration in &manifest.config_migrations {
        if migration.from_schema == migration.to_schema
            || !schemas.contains(&migration.from_schema)
            || !schemas.contains(&migration.to_schema)
            || !safe_ini_atom(&migration.from_section)
            || !safe_ini_atom(&migration.from_key)
            || !safe_ini_atom(&migration.to_section)
            || !safe_ini_atom(&migration.to_key)
            || migration.value_map.iter().any(|(from, to)| {
                from.is_empty()
                    || to.is_empty()
                    || from.contains(['\r', '\n', '\0'])
                    || to.contains(['\r', '\n', '\0'])
            })
            || !keys.insert((
                migration.from_schema,
                migration.to_schema,
                migration.from_section.to_ascii_lowercase(),
                migration.from_key.to_ascii_lowercase(),
            ))
        {
            return Err(failed(
                "invalid or duplicate OptiScaler configuration migration",
            ));
        }
    }
    Ok(())
}

fn ensure_acyclic<'a>(
    id: &'a str,
    modules: &HashMap<&'a str, &'a super::super::types::OptiScalerModule>,
    stack: &mut Vec<&'a str>,
) -> Result<(), ServiceError> {
    if stack.contains(&id) {
        return Err(failed(format!(
            "OptiScaler module dependency cycle at {id}"
        )));
    }
    stack.push(id);
    for dependency in &modules[id].requires {
        ensure_acyclic(dependency, modules, stack)?;
    }
    stack.pop();
    Ok(())
}

fn safe_relative(value: &str) -> bool {
    let path = Path::new(value);
    !value.is_empty()
        && !value.contains('\0')
        && !value.contains(':')
        && !path.is_absolute()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

fn normalized_relative_key(value: &str) -> String {
    value.replace('\\', "/").to_ascii_lowercase()
}

fn stable_module_id(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

fn stable_release_id(value: &str) -> bool {
    let Some(version) = value.strip_prefix('v') else {
        return false;
    };
    let mut parts = version.split('.');
    matches!(
        (parts.next(), parts.next(), parts.next(), parts.next()),
        (Some(major), Some(minor), Some(patch), None)
            if canonical_decimal(major) && canonical_decimal(minor) && canonical_decimal(patch)
    )
}

fn canonical_decimal(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| byte.is_ascii_digit())
        && (value == "0" || !value.starts_with('0'))
}

fn valid_revision(value: &str) -> bool {
    let Some((date, serial)) = value.split_once('.') else {
        return false;
    };
    valid_date_shape(date) && !serial.is_empty() && serial.bytes().all(|byte| byte.is_ascii_digit())
}

fn valid_date_shape(value: &str) -> bool {
    value.len() == 10
        && value.as_bytes()[4] == b'-'
        && value.as_bytes()[7] == b'-'
        && value
            .bytes()
            .enumerate()
            .all(|(index, byte)| matches!(index, 4 | 7) || byte.is_ascii_digit())
}

fn safe_ini_atom(value: &str) -> bool {
    !value.trim().is_empty() && !value.contains(['\r', '\n', '\0', '[', ']', '='])
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value == value.to_ascii_lowercase()
        && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}
