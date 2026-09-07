//! Strict peer-program envelope and durable seal calculation.

use std::collections::BTreeSet;
use std::fmt::Write;

use renderpilot_application::{AppError, AppResult};
use renderpilot_domain::{
    PeerEndpointIntent, PeerEndpointOperation, PeerEndpointRole, PeerFileImage, Sha256Hash,
};
use serde_json::Value;
use sha2::{Digest, Sha256};

pub(crate) use renderpilot_domain::CapabilityToken;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParsedPeerProgram {
    intents: Vec<PeerEndpointIntent>,
    renodx_reshade_ini_ordinal: Option<usize>,
    before: Vec<Option<PeerFileImage>>,
    roots: Vec<String>,
    transaction_owner: String,
    execution_class: String,
    stage: Vec<CapabilityToken>,
    custody: Vec<CapabilityToken>,
    created_ancestors: Vec<CapabilityToken>,
    read_guards: Vec<Vec<CapabilityToken>>,
    subtree_publishes: Vec<Vec<CapabilityToken>>,
    seal: [u8; 32],
}

pub(crate) fn parse_manifest(manifest_json: &str, context: &str) -> AppResult<Value> {
    serde_json::from_str(manifest_json)
        .map_err(|error| AppError::invalid_input(format!("invalid {context}: {error}")))
}

impl ParsedPeerProgram {
    pub(crate) fn intents(&self) -> &[PeerEndpointIntent] {
        &self.intents
    }

    pub(crate) fn renodx_reshade_ini_intent(&self) -> Option<&PeerEndpointIntent> {
        self.renodx_reshade_ini_ordinal
            .map(|ordinal| &self.intents[ordinal])
    }

    pub(crate) fn before(&self) -> &[Option<PeerFileImage>] {
        &self.before
    }

    pub(crate) fn roots(&self) -> &[String] {
        &self.roots
    }

    pub(crate) fn transaction_owner(&self) -> &str {
        &self.transaction_owner
    }

    pub(crate) fn execution_class(&self) -> &str {
        &self.execution_class
    }

    pub(crate) fn stage(&self) -> &[CapabilityToken] {
        &self.stage
    }

    pub(crate) fn custody(&self) -> &[CapabilityToken] {
        &self.custody
    }

    pub(crate) fn created_ancestors(&self) -> &[CapabilityToken] {
        &self.created_ancestors
    }

    pub(crate) fn read_guards(&self) -> &[Vec<CapabilityToken>] {
        &self.read_guards
    }

    pub(crate) fn subtree_publishes(&self) -> &[Vec<CapabilityToken>] {
        &self.subtree_publishes
    }

    pub(crate) fn seal(&self) -> [u8; 32] {
        self.seal
    }
}

pub(crate) fn parse_peer_program(manifest: &Value, context: &str) -> AppResult<ParsedPeerProgram> {
    parse_peer_program_inner(manifest, context, false)
}

/// Parses the narrow claim-only RenoDX DLSS-Fix envelope.  The generic parser
/// deliberately remains strict; only this storage-owned branch may accept an
/// empty physical endpoint list, and only its caller can supply the required
/// typed projection.
pub(crate) fn parse_peer_program_with_renodx_dlss(
    manifest: &Value,
    context: &str,
) -> AppResult<ParsedPeerProgram> {
    parse_peer_program_inner(manifest, context, true)
}

/// Parses a file-peer program under its persisted feature authority.
///
/// The generic envelope remains non-empty.  Claim-only programs are admitted
/// solely for the three storage-owned RenoDX DLSS-Fix feature labels; callers
/// must never infer that exception from manifest content alone.
pub(crate) fn parse_file_peer_program_for_feature(
    manifest: &Value,
    persisted_feature: &str,
    context: &str,
) -> AppResult<ParsedPeerProgram> {
    if renderpilot_domain::mutation_features::is_renodx_dlss_fix_feature(persisted_feature) {
        parse_peer_program_with_renodx_dlss(manifest, context)
    } else {
        parse_peer_program(manifest, context)
    }
}

fn parse_peer_program_inner(
    manifest: &Value,
    context: &str,
    allow_empty_endpoints: bool,
) -> AppResult<ParsedPeerProgram> {
    let program = manifest
        .get("peer_program")
        .and_then(Value::as_object)
        .ok_or_else(|| invalid(context, "peer_program must be an object"))?;
    let format = program
        .get("format")
        .and_then(Value::as_u64)
        .ok_or_else(|| invalid(context, "peer_program format must be 1"))?;
    if format != 1 {
        return Err(invalid(context, "peer_program format must be 1"));
    }
    let owner = required_string(program, "transaction_owner", context)?;
    let execution_class = required_string(program, "execution_class", context)?;
    if !matches!(
        execution_class.as_str(),
        "ordinary" | "retryable" | "shared"
    ) {
        return Err(invalid(context, "peer_program execution_class is invalid"));
    }
    let roots = required_string_array(program, "roots", context)?
        .into_iter()
        .map(|value| {
            renderpilot_domain::PathRef::parse_exact(&value)
                .map(|path| path.as_str().to_owned())
                .map_err(|error| invalid(context, &format!("peer_program root path: {error}")))
        })
        .collect::<AppResult<Vec<_>>>()?;
    if roots.is_empty() {
        return Err(invalid(context, "peer_program roots must not be empty"));
    }
    CapabilityToken::validate_roots(&roots)
        .map_err(|error| invalid(context, &format!("peer_program roots: {error}")))?;
    reject_unknown_keys(
        program,
        [
            "format",
            "transaction_owner",
            "execution_class",
            "roots",
            "stage",
            "custody",
            "created_ancestors",
            "endpoints",
        ],
        context,
    )?;
    let stage = parse_capability_array(program, "stage", &roots, context)?;
    let custody = parse_capability_array(program, "custody", &roots, context)?;
    let created_ancestors = parse_capability_array(program, "created_ancestors", &roots, context)?;
    let endpoints = program
        .get("endpoints")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid(context, "peer_program endpoints must be an array"))?;
    if endpoints.is_empty() && !allow_empty_endpoints {
        return Err(invalid(context, "peer_program endpoints must not be empty"));
    }

    let mut intents = Vec::with_capacity(endpoints.len());
    let mut renodx_reshade_ini_ordinal = None;
    let mut singleton_roles = BTreeSet::new();
    let mut before = Vec::with_capacity(endpoints.len());
    let mut read_guards = Vec::with_capacity(endpoints.len());
    let mut subtree_publishes = Vec::with_capacity(endpoints.len());
    let mut seal = SealWriter::default();
    seal.field("format", &format.to_string());
    seal.field("transaction_owner", &owner);
    seal.field("execution_class", &execution_class);
    seal.field_list("roots", &roots);
    seal.field_tokens("stage", &stage);
    seal.field_tokens("custody", &custody);
    seal.field_tokens("created_ancestors", &created_ancestors);

    for (ordinal, endpoint) in endpoints.iter().enumerate() {
        let endpoint = endpoint
            .as_object()
            .ok_or_else(|| invalid(context, "peer_program endpoint must be an object"))?;
        reject_unknown_keys(
            endpoint,
            [
                "ordinal",
                "path",
                "role",
                "operation",
                "planned_sha256",
                "planned_length",
                "before",
                "read_guards",
                "subtree_publishes",
            ],
            context,
        )?;
        let actual_ordinal = endpoint
            .get("ordinal")
            .and_then(Value::as_u64)
            .ok_or_else(|| invalid(context, "peer_program endpoint ordinal is missing"))?;
        if actual_ordinal != ordinal as u64 {
            return Err(invalid(
                context,
                "peer_program endpoint ordinals are not contiguous",
            ));
        }
        let path =
            renderpilot_domain::PathRef::parse_exact(&required_string(endpoint, "path", context)?)
                .map_err(|error| invalid(context, &error.to_string()))?;
        let role_name = required_string(endpoint, "role", context)?;
        let operation_name = required_string(endpoint, "operation", context)?;
        let role = parse_role(&role_name, context)?;
        if role != PeerEndpointRole::Disjoint && !singleton_roles.insert(role.as_str()) {
            return Err(invalid(
                context,
                "peer_program contains duplicate singleton endpoint roles",
            ));
        }
        if role == PeerEndpointRole::RenoDxReshadeIni {
            if renodx_reshade_ini_ordinal.is_some() {
                return Err(invalid(
                    context,
                    "peer_program may contain at most one renodx_reshade_ini endpoint",
                ));
            }
            renodx_reshade_ini_ordinal = Some(ordinal);
        }
        let operation = parse_operation(&operation_name, context)?;
        let planned_sha256 = optional_digest(endpoint, "planned_sha256", context)?;
        let planned_length = optional_u64(endpoint, "planned_length", context)?;
        let before_image = parse_image(endpoint.get("before"), context)?;
        let after_image = endpoint.get("after");
        match operation {
            PeerEndpointOperation::Create if before_image.is_some() || after_image.is_some() => {
                return Err(invalid(context, "create endpoint has invalid O1 images"));
            }
            PeerEndpointOperation::Replace if before_image.is_none() || after_image.is_some() => {
                return Err(invalid(context, "replace endpoint has invalid O1 images"));
            }
            PeerEndpointOperation::Remove if before_image.is_none() || after_image.is_some() => {
                return Err(invalid(context, "remove endpoint has invalid O1 images"));
            }
            _ => {}
        }
        if matches!(
            operation,
            PeerEndpointOperation::Create | PeerEndpointOperation::Replace
        ) && (planned_sha256.is_none() || planned_length.is_none())
        {
            return Err(invalid(
                context,
                "create and replace endpoints require planned digest and length",
            ));
        }
        let intent = PeerEndpointIntent::new(path, role, operation, planned_sha256, planned_length)
            .map_err(|error| invalid(context, &error.to_string()))?;

        let read_guards_value = endpoint
            .get("read_guards")
            .ok_or_else(|| invalid(context, "peer_program read_guards is missing"))?;
        let endpoint_read_guards =
            parse_capability_value(read_guards_value, &roots, context, "read_guards")?;
        let subtree_publishes_value = endpoint
            .get("subtree_publishes")
            .ok_or_else(|| invalid(context, "peer_program subtree_publishes is missing"))?;
        let endpoint_subtree_publishes = parse_capability_value(
            subtree_publishes_value,
            &roots,
            context,
            "subtree_publishes",
        )?;

        seal.endpoint(
            ordinal,
            &intent,
            before_image.as_ref(),
            None,
            &endpoint_read_guards,
            &endpoint_subtree_publishes,
        );
        intents.push(intent);
        before.push(before_image);
        read_guards.push(endpoint_read_guards);
        subtree_publishes.push(endpoint_subtree_publishes);
    }

    Ok(ParsedPeerProgram {
        intents,
        renodx_reshade_ini_ordinal,
        before,
        roots,
        transaction_owner: owner,
        execution_class,
        stage,
        custody,
        created_ancestors,
        read_guards,
        subtree_publishes,
        seal: seal.finish(),
    })
}

fn parse_role(value: &str, context: &str) -> AppResult<PeerEndpointRole> {
    match value {
        "disjoint" => Ok(PeerEndpointRole::Disjoint),
        "topology_downstream" => Ok(PeerEndpointRole::TopologyDownstream),
        "renodx_reshade_ini" => Ok(PeerEndpointRole::RenoDxReshadeIni),
        "optiscaler_config" => Ok(PeerEndpointRole::OptiScalerConfig),
        "dlss_fix" => Ok(PeerEndpointRole::DlssFix),
        _ => Err(invalid(context, "peer_program endpoint role is invalid")),
    }
}

fn parse_operation(value: &str, context: &str) -> AppResult<PeerEndpointOperation> {
    match value {
        "create" => Ok(PeerEndpointOperation::Create),
        "replace" => Ok(PeerEndpointOperation::Replace),
        "remove" => Ok(PeerEndpointOperation::Remove),
        _ => Err(invalid(
            context,
            "peer_program endpoint operation is invalid",
        )),
    }
}

fn parse_image(value: Option<&Value>, context: &str) -> AppResult<Option<PeerFileImage>> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    let object = value
        .as_object()
        .ok_or_else(|| invalid(context, "peer_program image must be an object or null"))?;
    let identity = required_string(object, "identity", context)?;
    let digest = required_string(object, "sha256", context)?;
    let digest = Sha256Hash::new(digest).map_err(|error| invalid(context, &error.to_string()))?;
    let length = object
        .get("length")
        .and_then(Value::as_u64)
        .ok_or_else(|| invalid(context, "peer_program image length is missing"))?;
    PeerFileImage::new(identity, digest, length)
        .map(Some)
        .map_err(|error| invalid(context, &error.to_string()))
}

fn optional_digest(
    object: &serde_json::Map<String, Value>,
    key: &str,
    context: &str,
) -> AppResult<Option<Sha256Hash>> {
    let Some(value) = object.get(key) else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    let value = value
        .as_str()
        .ok_or_else(|| invalid(context, "peer_program planned digest must be a string"))?;
    Sha256Hash::new(value)
        .map(Some)
        .map_err(|error| invalid(context, &error.to_string()))
}

fn optional_u64(
    object: &serde_json::Map<String, Value>,
    key: &str,
    context: &str,
) -> AppResult<Option<u64>> {
    let Some(value) = object.get(key) else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    value
        .as_u64()
        .map(Some)
        .ok_or_else(|| invalid(context, "peer_program planned length must be an integer"))
}

fn required_string(
    object: &serde_json::Map<String, Value>,
    key: &str,
    context: &str,
) -> AppResult<String> {
    object
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty() && !value.contains('\0'))
        .map(str::to_owned)
        .ok_or_else(|| invalid(context, &format!("peer_program {key} is missing or empty")))
}

fn required_string_array(
    object: &serde_json::Map<String, Value>,
    key: &str,
    context: &str,
) -> AppResult<Vec<String>> {
    let values = object
        .get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| invalid(context, &format!("peer_program {key} must be an array")))?;
    values
        .iter()
        .map(|value| {
            value
                .as_str()
                .filter(|value| !value.trim().is_empty() && !value.contains('\0'))
                .map(str::to_owned)
                .ok_or_else(|| invalid(context, "peer_program root must be a non-empty string"))
        })
        .collect()
}

fn parse_capability_array(
    object: &serde_json::Map<String, Value>,
    key: &str,
    roots: &[String],
    context: &str,
) -> AppResult<Vec<CapabilityToken>> {
    let value = object
        .get(key)
        .ok_or_else(|| invalid(context, &format!("peer_program {key} is missing")))?;
    parse_capability_value(value, roots, context, key)
}

fn parse_capability_value(
    value: &Value,
    roots: &[String],
    context: &str,
    name: &str,
) -> AppResult<Vec<CapabilityToken>> {
    let values = value
        .as_array()
        .ok_or_else(|| invalid(context, &format!("peer_program {name} must be an array")))?;
    let mut parsed = Vec::with_capacity(values.len());
    let mut seen = BTreeSet::new();
    for value in values {
        let value = value
            .as_str()
            .filter(|value| !value.trim().is_empty() && !value.contains('\0'))
            .ok_or_else(|| invalid(context, &format!("peer_program {name} must contain paths")))?;
        let token = CapabilityToken::parse_exact(value, roots).map_err(|error| {
            invalid(context, &format!("peer_program {name} capability: {error}"))
        })?;
        if !seen.insert(token.clone()) {
            return Err(invalid(
                context,
                &format!("peer_program {name} contains duplicates"),
            ));
        }
        parsed.push(token);
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use renderpilot_domain::mutation_features::{
        LUMA_INSTALL, RENODX_DLSS_FIX_INSTALL, RENODX_DLSS_FIX_UNINSTALL, RENODX_DLSS_FIX_UPDATE,
        RENODX_INSTALL, RENODX_UPDATE,
    };
    use renderpilot_domain::{CapabilityToken, PathRef};
    use serde_json::json;

    use super::parse_file_peer_program_for_feature;

    fn claim_only_program() -> serde_json::Value {
        json!({
            "peer_program": {
                "format": 1,
                "transaction_owner": "claim-only-operation",
                "execution_class": "ordinary",
                "roots": ["C:/game"],
                "stage": [],
                "custody": [],
                "created_ancestors": [],
                "endpoints": []
            }
        })
    }

    #[test]
    fn claim_only_program_is_admitted_only_for_the_exact_dlss_feature_allowlist() {
        let manifest = claim_only_program();
        for feature in [
            RENODX_DLSS_FIX_INSTALL,
            RENODX_DLSS_FIX_UNINSTALL,
            RENODX_DLSS_FIX_UPDATE,
        ] {
            assert!(
                parse_file_peer_program_for_feature(&manifest, feature, "claim-only").is_ok(),
                "{feature} must retain its explicit claim-only exception"
            );
        }
        for feature in [
            RENODX_INSTALL,
            RENODX_UPDATE,
            LUMA_INSTALL,
            "unknown_feature",
        ] {
            assert!(
                parse_file_peer_program_for_feature(&manifest, feature, "claim-only").is_err(),
                "{feature} must not inherit the DLSS claim-only exception"
            );
        }
    }

    #[test]
    fn capability_token_matches_canonical_drive_root() {
        let roots = ["C:/Games/Example".to_owned()];
        let path = PathRef::parse_exact("C:/Games/Example/ReShade64.dll").expect("path");

        assert_eq!(
            CapabilityToken::from_path(&path, &roots)
                .expect("capability")
                .as_str(),
            "C:/Games/Example:reshade64.dll"
        );
    }

    #[test]
    fn capability_token_matches_canonical_unc_root() {
        let roots = ["//server/share/Game".to_owned()];
        let path = PathRef::parse_exact("//server/share/Game/ReShade64.dll").expect("path");

        assert_eq!(
            CapabilityToken::from_path(&path, &roots)
                .expect("capability")
                .as_str(),
            "//server/share/Game:reshade64.dll"
        );
    }

    #[test]
    fn capability_token_rejects_sibling_prefix() {
        let roots = ["C:/Games/Game".to_owned()];
        let path = PathRef::parse_exact("C:/Games/Game2/ReShade64.dll").expect("path");

        assert!(CapabilityToken::from_path(&path, &roots).is_err());
    }
}

fn reject_unknown_keys<'a>(
    object: &serde_json::Map<String, Value>,
    allowed: impl IntoIterator<Item = &'a str>,
    context: &str,
) -> AppResult<()> {
    let allowed = allowed.into_iter().collect::<BTreeSet<_>>();
    if let Some(key) = object.keys().find(|key| !allowed.contains(key.as_str())) {
        return Err(invalid(
            context,
            &format!("peer_program contains unknown field `{key}`"),
        ));
    }
    Ok(())
}

fn invalid(context: &str, message: &str) -> AppError {
    AppError::storage_failed(format!("{context}: {message}"))
}

#[derive(Default)]
struct SealWriter {
    bytes: String,
}

impl SealWriter {
    fn field(&mut self, key: &str, value: &str) {
        let _ = writeln!(self.bytes, "{key}={value}");
    }

    fn field_list(&mut self, key: &str, values: &[String]) {
        self.field(key, &values.join("\u{1f}"));
    }

    fn field_tokens(&mut self, key: &str, values: &[CapabilityToken]) {
        let rendered = values
            .iter()
            .map(CapabilityToken::as_str)
            .collect::<Vec<_>>()
            .join("\u{1f}");
        self.field(key, &rendered);
    }

    fn endpoint(
        &mut self,
        ordinal: usize,
        intent: &PeerEndpointIntent,
        before: Option<&PeerFileImage>,
        after: Option<&PeerFileImage>,
        read_guards: &[CapabilityToken],
        subtree_publishes: &[CapabilityToken],
    ) {
        let _ = writeln!(self.bytes, "ordinal={ordinal}");
        self.field("path", intent.path().as_str());
        self.field("role", role_name(intent.role()));
        self.field("operation", operation_name(intent.operation()));
        self.field("planned_sha256", &digest_name(intent.planned_sha256()));
        self.field(
            "planned_length",
            &intent
                .planned_length()
                .map_or_else(|| "-".to_owned(), |value| value.to_string()),
        );
        self.image("before", before);
        self.image("after", after);
        self.field_tokens("read_guards", read_guards);
        self.field_tokens("subtree_publishes", subtree_publishes);
    }

    fn image(&mut self, key: &str, image: Option<&PeerFileImage>) {
        match image {
            Some(image) => {
                self.field(
                    key,
                    &format!(
                        "{}:{}:{}",
                        image.identity(),
                        image.sha256().as_str(),
                        image.length()
                    ),
                );
            }
            None => self.field(key, "-"),
        }
    }

    fn finish(self) -> [u8; 32] {
        sha256_bytes(self.bytes.as_bytes())
    }
}

fn role_name(role: PeerEndpointRole) -> &'static str {
    match role {
        PeerEndpointRole::Disjoint => "disjoint",
        PeerEndpointRole::TopologyDownstream => "topology_downstream",
        PeerEndpointRole::RenoDxReshadeIni => "renodx_reshade_ini",
        PeerEndpointRole::OptiScalerConfig => "optiscaler_config",
        PeerEndpointRole::DlssFix => "dlss_fix",
    }
}

fn operation_name(operation: PeerEndpointOperation) -> &'static str {
    match operation {
        PeerEndpointOperation::Create => "create",
        PeerEndpointOperation::Replace => "replace",
        PeerEndpointOperation::Remove => "remove",
    }
}

fn digest_name(digest: Option<&Sha256Hash>) -> String {
    digest.map_or_else(|| "-".to_owned(), |digest| digest.as_str().to_owned())
}

pub(crate) fn sha256_bytes(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}
