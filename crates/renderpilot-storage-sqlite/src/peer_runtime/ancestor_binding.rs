use std::collections::BTreeSet;

use renderpilot_application::{AppError, AppResult};
use renderpilot_domain::{CapabilityToken, PathRef, PeerEndpointOperation};
use serde_json::Value;

use super::ParsedPeerProgram;
use super::recovery::PeerRecoveryAncestor;

/// Bind the durable directory projection to the already sealed endpoint
/// program.  The file manifest carries native paths and endpoint ordinals;
/// the peer program carries the lowered capability tokens.  Both views must
/// describe exactly the same ordered projection before a permit is minted.
pub(super) fn bind(
    manifest: &serde_json::Map<String, Value>,
    program: &ParsedPeerProgram,
    roots: &[String],
) -> AppResult<Vec<PeerRecoveryAncestor>> {
    let Some(value) = manifest.get("peer_ancestors") else {
        if program.created_ancestors().is_empty()
            && program.subtree_publishes().iter().all(Vec::is_empty)
        {
            return Ok(Vec::new());
        }
        return Err(AppError::storage_failed(
            "file peer manifest is missing its ancestor projection",
        ));
    };

    let entries = value
        .as_array()
        .ok_or_else(|| AppError::storage_failed("file peer peer_ancestors must be an array"))?;
    let mut canonical = Vec::with_capacity(entries.len());
    let mut seen = BTreeSet::new();

    for entry in entries {
        let entry = entry.as_object().ok_or_else(|| {
            AppError::storage_failed("file peer ancestor entry must be an object")
        })?;
        reject_unknown_keys(entry)?;
        let path = entry
            .get("path")
            .and_then(Value::as_str)
            .filter(|path| !path.trim().is_empty() && !path.contains('\0'))
            .ok_or_else(|| {
                AppError::storage_failed("file peer ancestor path must be a non-empty string")
            })?;
        let path = PathRef::parse_exact(path)
            .map_err(|error| AppError::storage_failed(format!("file peer ancestor: {error}")))?;
        let token = CapabilityToken::from_path(&path, roots).map_err(|error| {
            AppError::storage_failed(format!("file peer ancestor capability: {error}"))
        })?;
        if token.as_str().ends_with(":.") {
            return Err(AppError::storage_failed(
                "file peer ancestor path cannot be a declared root",
            ));
        }
        if !seen.insert(token.clone()) {
            return Err(AppError::storage_failed(
                "file peer ancestor paths contain duplicate capabilities",
            ));
        }

        let consumers = parse_consumers(entry.get("consumer_ordinals"), program)?;
        canonical.push((path.as_str().to_owned(), token, consumers));
    }

    let lowered = canonical.iter().map(|(_, token, _)| token);
    let expected = program.created_ancestors().iter();
    if !lowered.eq(expected) {
        return Err(AppError::storage_failed(
            "file peer ancestor capability order differs from peer program",
        ));
    }

    for (ordinal, intent) in program.intents().iter().enumerate() {
        let listed = canonical
            .iter()
            .filter(|(_, _, consumers)| consumers.contains(&ordinal))
            .map(|(_, token, _)| token);
        let expected = program.subtree_publishes()[ordinal].iter();
        if !listed.eq(expected) {
            return Err(AppError::storage_failed(
                "file peer subtree publish order differs from peer program",
            ));
        }
        if !matches!(intent.operation(), PeerEndpointOperation::Create)
            && !program.subtree_publishes()[ordinal].is_empty()
        {
            return Err(AppError::storage_failed(
                "file peer replace or remove endpoint cannot consume ancestors",
            ));
        }
    }

    Ok(canonical
        .into_iter()
        .map(|(path, _, consumers)| PeerRecoveryAncestor::from_validated(path, consumers))
        .collect())
}

fn parse_consumers(value: Option<&Value>, program: &ParsedPeerProgram) -> AppResult<Vec<usize>> {
    let values = value
        .and_then(Value::as_array)
        .ok_or_else(|| AppError::storage_failed("file peer ancestor consumers must be an array"))?;
    if values.is_empty() {
        return Err(AppError::storage_failed(
            "file peer ancestor consumers must not be empty",
        ));
    }
    let mut consumers = Vec::with_capacity(values.len());
    let mut previous = None;
    for value in values {
        let ordinal = value
            .as_u64()
            .and_then(|ordinal| usize::try_from(ordinal).ok())
            .ok_or_else(|| {
                AppError::storage_failed(
                    "file peer ancestor consumer ordinal must be a usize integer",
                )
            })?;
        if ordinal >= program.intents().len()
            || previous.is_some_and(|previous| ordinal <= previous)
        {
            return Err(AppError::storage_failed(
                "file peer ancestor consumer ordinals must be strictly increasing endpoints",
            ));
        }
        if !matches!(
            program.intents()[ordinal].operation(),
            PeerEndpointOperation::Create
        ) {
            return Err(AppError::storage_failed(
                "file peer ancestor consumer must be a create endpoint",
            ));
        }
        previous = Some(ordinal);
        consumers.push(ordinal);
    }
    Ok(consumers)
}

fn reject_unknown_keys(object: &serde_json::Map<String, Value>) -> AppResult<()> {
    if let Some(key) = object
        .keys()
        .find(|key| !matches!(key.as_str(), "path" | "consumer_ordinals"))
    {
        return Err(AppError::storage_failed(format!(
            "file peer ancestor entry contains unknown field `{key}`"
        )));
    }
    Ok(())
}
