use super::model::*;
use crate::addons::catalog_message::CatalogMessage;
use std::collections::HashSet;

pub(super) fn validate_catalog(
    wire: &WireCompatibilityCatalog,
) -> Result<Vec<CompatibilityEntry>, crate::ServiceError> {
    if wire.schema_version != 1 || !valid_revision(&wire.revision) {
        return Err(invalid_catalog("unsupported schema or revision"));
    }
    if wire.upstream.source.trim().is_empty()
        || wire.upstream.snapshot_revision.trim().is_empty()
        || !is_sha256(&wire.upstream.snapshot_sha256)
    {
        return Err(invalid_catalog("invalid upstream provenance"));
    }

    let mut ids = HashSet::new();
    let mut identities = HashSet::new();
    let mut entries = Vec::with_capacity(wire.entries.len());
    for entry in &wire.entries {
        if !stable_id(&entry.id) || !ids.insert(entry.id.to_ascii_lowercase()) {
            return Err(invalid_catalog(
                "entry ids must be unique stable identifiers",
            ));
        }
        if entry.identities.is_empty() || entry.variants.is_empty() {
            return Err(invalid_catalog(format!(
                "entry {} needs identities and variants",
                entry.id
            )));
        }
        for identity in &entry.identities {
            validate_identity(identity)?;
            if !identities.insert(identity_key(identity)) {
                return Err(invalid_catalog(format!(
                    "identity {} appears in multiple entries",
                    identity.value
                )));
            }
        }
        let mut declared = HashSet::new();
        if entry
            .declared_inputs
            .iter()
            .any(|input| !declared.insert(*input))
        {
            return Err(invalid_catalog(format!(
                "entry {} repeats declared input knowledge",
                entry.id
            )));
        }
        let guidance = validate_guidance(entry)?;
        let variants = validate_variants(entry)?;
        entries.push(CompatibilityEntry {
            id: entry.id.clone(),
            status: entry.status,
            identities: entry.identities.clone(),
            declared_inputs: entry.declared_inputs.clone(),
            guidance,
            variants,
        });
    }
    Ok(entries)
}

fn validate_guidance(
    entry: &WireCompatibilityEntry,
) -> Result<Vec<CatalogGuidance>, crate::ServiceError> {
    let mut ids = HashSet::new();
    let mut out = Vec::with_capacity(entry.guidance.len());
    for item in &entry.guidance {
        let message: CatalogMessage = item.message.clone().into();
        message.validate("compatibility guidance message")?;
        if !ids.insert((item.kind as u8, message.id.clone())) {
            return Err(invalid_catalog(format!(
                "entry {} repeats guidance message {}",
                entry.id, message.id
            )));
        }
        out.push(CatalogGuidance {
            kind: item.kind,
            message,
        });
    }
    Ok(out)
}

fn validate_variants(
    entry: &WireCompatibilityEntry,
) -> Result<Vec<ValidatedVariant>, crate::ServiceError> {
    let defaults = entry
        .variants
        .iter()
        .filter(|variant| variant.when.is_none())
        .count();
    if defaults != 1 {
        return Err(invalid_catalog(format!(
            "entry {} must declare exactly one default variant",
            entry.id
        )));
    }
    let mut conditional_variants = Vec::new();
    let mut variants = Vec::with_capacity(entry.variants.len());
    for source in &entry.variants {
        validate_variant(entry, source)?;
        if let Some(condition) = source.when.as_ref()
            && conditional_variants
                .iter()
                .any(|existing| conditions_overlap(existing, condition))
        {
            return Err(invalid_catalog(format!(
                "entry {} has overlapping conditional variants",
                entry.id
            )));
        }
        if let Some(condition) = source.when.as_ref() {
            conditional_variants.push(condition.clone());
        }
        variants.push(source.clone().try_into()?);
    }
    Ok(variants)
}

fn validate_variant(
    entry: &WireCompatibilityEntry,
    variant: &WireCompatibilityVariant,
) -> Result<(), crate::ServiceError> {
    if let Some(condition) = &variant.when {
        if condition.launcher.is_none() && condition.executable.is_none() {
            return Err(invalid_catalog(format!(
                "entry {} has an empty variant condition",
                entry.id
            )));
        }
        if condition
            .executable
            .as_deref()
            .is_some_and(|executable| !valid_executable_leaf(executable))
        {
            return Err(invalid_catalog(format!(
                "entry {} has invalid executable condition",
                entry.id
            )));
        }
    }
    if let ProxyPolicy::Exact { slot } = &variant.proxy
        && !valid_proxy_slot(slot)
    {
        return Err(invalid_catalog(format!(
            "entry {} has an unsupported proxy slot",
            entry.id
        )));
    }
    let mut ini = HashSet::new();
    for value in &variant.ini_overrides {
        if !safe_ini_atom(&value.section)
            || !safe_ini_atom(&value.key)
            || value.value.contains(['\r', '\n', '\0'])
            || !ini.insert((
                value.section.to_ascii_lowercase(),
                value.key.to_ascii_lowercase(),
            ))
        {
            return Err(invalid_catalog(format!(
                "entry {} has invalid or duplicate INI override",
                entry.id
            )));
        }
    }
    if let Some(launch) = &variant.launch {
        let mut arguments = HashSet::new();
        if launch.arguments.is_empty()
            || launch.arguments.iter().any(|argument| {
                !safe_launch_argument(argument) || !arguments.insert(argument.as_str())
            })
        {
            return Err(invalid_catalog(format!(
                "entry {} has invalid or duplicate launch arguments",
                entry.id
            )));
        }
    }
    let mut modules = HashSet::new();
    if variant
        .restricted_modules
        .iter()
        .any(|module| !stable_module_id(module) || !modules.insert(module.as_str()))
    {
        return Err(invalid_catalog(format!(
            "entry {} has invalid or duplicate module restrictions",
            entry.id
        )));
    }
    Ok(())
}

fn validate_identity(identity: &EntryIdentity) -> Result<(), crate::ServiceError> {
    let valid = match identity.kind {
        EntryIdentityKind::SteamAppid => positive_decimal(&identity.value),
        EntryIdentityKind::EpicId | EntryIdentityKind::GogId => stable_external_id(&identity.value),
        EntryIdentityKind::XboxStoreId => canonical_xbox_store_id(&identity.value),
        EntryIdentityKind::ExeName => valid_executable_leaf(&identity.value),
    };
    if valid {
        Ok(())
    } else {
        Err(invalid_catalog("invalid exact game identity"))
    }
}

fn identity_key(identity: &EntryIdentity) -> String {
    format!(
        "{:?}:{}",
        identity.kind,
        identity.value.to_ascii_lowercase()
    )
}

fn conditions_overlap(left: &WireVariantCondition, right: &WireVariantCondition) -> bool {
    left.launcher
        .zip(right.launcher)
        .is_none_or(|(a, b)| a == b)
        && left
            .executable
            .as_deref()
            .zip(right.executable.as_deref())
            .is_none_or(|(a, b)| a.eq_ignore_ascii_case(b))
}

fn valid_revision(value: &str) -> bool {
    let Some((date, sequence)) = value.rsplit_once('.') else {
        return false;
    };
    date.len() == 10
        && date.as_bytes()[4] == b'-'
        && date.as_bytes()[7] == b'-'
        && date[..4].bytes().all(|b| b.is_ascii_digit())
        && date[5..7].bytes().all(|b| b.is_ascii_digit())
        && date[8..].bytes().all(|b| b.is_ascii_digit())
        && !sequence.is_empty()
        && sequence.bytes().all(|b| b.is_ascii_digit())
}

fn stable_id(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
        })
}

fn stable_module_id(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

fn stable_external_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
}

fn positive_decimal(value: &str) -> bool {
    !value.is_empty() && value != "0" && value.bytes().all(|byte| byte.is_ascii_digit())
}

fn canonical_xbox_store_id(value: &str) -> bool {
    value.len() == 12
        && value.bytes().all(|byte| byte.is_ascii_alphanumeric())
        && value.bytes().all(|byte| !byte.is_ascii_lowercase())
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn valid_proxy_slot(value: &str) -> bool {
    const CLOSED_SLOTS: &[&str] = &[
        "nvngx.dll",
        "dxgi.dll",
        "winmm.dll",
        "d3d12.dll",
        "version.dll",
        "dbghelp.dll",
        "wininet.dll",
        "winhttp.dll",
    ];
    CLOSED_SLOTS
        .iter()
        .any(|slot| value.eq_ignore_ascii_case(slot))
}

fn valid_executable_leaf(value: &str) -> bool {
    let path = std::path::Path::new(value);
    path.file_name().and_then(|name| name.to_str()) == Some(value)
        && !value.eq_ignore_ascii_case(".")
        && !value.eq_ignore_ascii_case("..")
        && value.len() > 4
        && value[..].to_ascii_lowercase().ends_with(".exe")
        && !value.contains(['/', '\\', '\0'])
}

fn safe_ini_atom(value: &str) -> bool {
    !value.is_empty() && !value.contains(['\r', '\n', '\0', '[', ']', '='])
}

fn safe_launch_argument(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() >= 2
        && bytes[0] == b'-'
        && bytes[1].is_ascii_alphanumeric()
        && bytes[2..]
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'=' | b'-'))
}
