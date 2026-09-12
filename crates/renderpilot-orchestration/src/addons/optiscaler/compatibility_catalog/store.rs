//! Admission for revisable compatibility knowledge.
//!
//! This cache is deliberately independent from the immutable release cache.
//! A compatibility correction may be replaced, but never silently downgrades
//! the last admitted document or rewrites an equal revision with different
//! content.

use std::collections::HashSet;
use std::path::Path;
use std::sync::OnceLock;
use std::time::{Duration, SystemTime};

use serde::Deserialize;

use crate::addons::catalog_message::WireCatalogMessage;
use crate::cdn;
use crate::fs::{CacheObservation, CachePublication, MatchingCurrentPolicy};
use crate::{ServiceError, failed};

use super::model::{
    GuidanceKind, OptiScalerCompatibilityCatalog, WireCompatibilityCatalog, invalid_catalog,
};

const BUNDLED_CATALOG: &[u8] =
    include_bytes!("../../../../assets/optiscaler-compatibility-fallback.json");
const BUNDLED_MESSAGES: &[u8] =
    include_bytes!("../../../../assets/optiscaler-compatibility-messages.json");
const CACHE_FILE: &str = "optiscaler_compatibility_catalog_v1.json";
const MAX_BYTES: u64 = 2 * 1024 * 1024;
const TTL: Duration = Duration::from_hours(24);

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct WireMessageContract {
    schema_version: u32,
    revision: String,
    messages: Vec<WireMessageContractEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct WireMessageContractEntry {
    id: String,
    fallback_text: String,
    guidance_kind: GuidanceKind,
    context: String,
}

#[derive(Debug, Clone)]
struct MessageContract {
    revision: String,
    tuples: HashSet<(String, String, GuidanceKind, String)>,
}

#[derive(Debug)]
struct CachedCatalog {
    catalog: OptiScalerCompatibilityCatalog,
    fresh: bool,
}

fn catalog_lock() -> &'static tokio::sync::Mutex<()> {
    static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

/// Strictly parses one compatibility document against the bundled message
/// contract. The external document never carries arbitrary display text.
pub(crate) fn parse_catalog(bytes: &[u8]) -> Result<OptiScalerCompatibilityCatalog, ServiceError> {
    parse_catalog_with_contract(bytes, &bundled_message_contract()?)
}

fn parse_catalog_with_contract(
    bytes: &[u8],
    messages: &MessageContract,
) -> Result<OptiScalerCompatibilityCatalog, ServiceError> {
    let wire: WireCompatibilityCatalog =
        serde_json::from_slice(crate::fs::strip_utf8_bom(bytes))
            .map_err(|error| invalid_catalog(format!("invalid JSON: {error}")))?;
    validate_message_contract(&wire, messages)?;
    wire.try_into()
}

fn bundled_message_contract() -> Result<MessageContract, ServiceError> {
    parse_message_contract(BUNDLED_MESSAGES)
}

fn parse_message_contract(bytes: &[u8]) -> Result<MessageContract, ServiceError> {
    let wire: WireMessageContract = serde_json::from_slice(crate::fs::strip_utf8_bom(bytes))
        .map_err(|error| failed(format!("invalid OptiScaler message contract JSON: {error}")))?;
    if wire.schema_version != 1 || revision_key(&wire.revision).is_err() {
        return Err(failed("invalid OptiScaler message contract revision"));
    }
    let mut tuples = HashSet::new();
    for item in wire.messages {
        let message = WireCatalogMessage {
            id: item.id.clone(),
            fallback_text: item.fallback_text.clone(),
        };
        let message: crate::addons::catalog_message::CatalogMessage = message.into();
        message.validate("OptiScaler message contract entry")?;
        if item.context != item.guidance_kind.as_str()
            || !tuples.insert((
                item.id,
                item.fallback_text,
                item.guidance_kind,
                item.context,
            ))
        {
            return Err(failed(
                "invalid or duplicate OptiScaler message contract entry",
            ));
        }
    }
    Ok(MessageContract {
        revision: wire.revision,
        tuples,
    })
}

fn validate_message_contract(
    catalog: &WireCompatibilityCatalog,
    messages: &MessageContract,
) -> Result<(), ServiceError> {
    if catalog.revision != messages.revision {
        return Err(invalid_catalog(
            "catalog and bundled message contract revisions differ",
        ));
    }
    let mut used = HashSet::new();
    for guidance in catalog.entries.iter().flat_map(|entry| &entry.guidance) {
        let tuple = (
            guidance.message.id.clone(),
            guidance.message.fallback_text.clone(),
            guidance.kind,
            guidance.kind.as_str().to_owned(),
        );
        if !messages.tuples.contains(&tuple) || !used.insert(tuple) {
            return Err(invalid_catalog(
                "guidance must be a unique exact message-contract tuple",
            ));
        }
    }
    if used != messages.tuples {
        return Err(invalid_catalog(
            "message contract must contain exactly the published guidance tuples",
        ));
    }
    Ok(())
}

/// Returns the best independently admitted compatibility document.
pub(crate) async fn get_or_fetch_catalog() -> Result<OptiScalerCompatibilityCatalog, ServiceError> {
    let _single_flight = catalog_lock().lock().await;
    let bundled = parse_catalog(BUNDLED_CATALOG)?;
    let path = crate::app_dir::app_dir()?.join(CACHE_FILE);
    let observed = observe_cache(&path)?;
    let cached = match &observed {
        CacheObservation::Valid { value, .. } if accepts(&value.catalog, &bundled) => {
            Some(&value.catalog)
        }
        _ => None,
    };
    if let Some(cache) = cached
        && matches!(&observed, CacheObservation::Valid { value, .. } if value.fresh)
    {
        return Ok(cache.clone());
    }
    let admitted = cached.unwrap_or(&bundled);
    let (candidate_bytes, candidate) = match crate::net::download_limited_bytes(
        &cdn::cdn_url("addons/v1/optiscaler-compatibility.json"),
        MAX_BYTES,
        "OptiScaler compatibility catalog fetch",
    )
    .await
    {
        Ok(bytes) => match parse_catalog(&bytes) {
            Ok(candidate) => (bytes, candidate),
            Err(error) => {
                log::warn!("OptiScaler compatibility catalog rejected: {error}");
                return Ok(admitted.clone());
            }
        },
        Err(error) => {
            log::warn!("OptiScaler compatibility catalog refresh failed: {error}");
            return Ok(admitted.clone());
        }
    };
    match compare_admission(&candidate, admitted) {
        AdmissionOrder::Older | AdmissionOrder::Equal => Ok(admitted.clone()),
        AdmissionOrder::Divergent => {
            log::warn!("OptiScaler compatibility catalog rejected: equal revision changes content");
            Ok(admitted.clone())
        }
        AdmissionOrder::Newer => publish_candidate(
            &path,
            observed.generation(),
            &candidate_bytes,
            &candidate,
            &bundled,
            admitted,
        ),
    }
}

fn observe_cache(path: &Path) -> Result<CacheObservation<CachedCatalog>, ServiceError> {
    crate::fs::observe_cache_file(path, |bytes, metadata| {
        if metadata.len() > MAX_BYTES {
            return Err(failed(
                "OptiScaler compatibility cache exceeds its size limit",
            ));
        }
        let catalog = parse_catalog(bytes)?;
        let fresh = metadata
            .modified()
            .ok()
            .and_then(|modified| SystemTime::now().duration_since(modified).ok())
            .is_none_or(|age| age <= TTL);
        Ok(CachedCatalog { catalog, fresh })
    })
}

fn publish_candidate(
    path: &Path,
    observed: &crate::fs::CacheGeneration,
    bytes: &[u8],
    candidate: &OptiScalerCompatibilityCatalog,
    bundled: &OptiScalerCompatibilityCatalog,
    fallback: &OptiScalerCompatibilityCatalog,
) -> Result<OptiScalerCompatibilityCatalog, ServiceError> {
    let publication = match crate::fs::commit_cache_candidate(
        path,
        observed,
        bytes,
        MatchingCurrentPolicy::PreserveCurrent,
        parse_catalog,
    ) {
        Ok(publication) => publication,
        Err(error) => {
            log::warn!("OptiScaler compatibility catalog publication failed: {error}");
            return Ok(fallback.clone());
        }
    };
    match publication {
        CachePublication::Published => Ok(candidate.clone()),
        CachePublication::Current(_) | CachePublication::PreservedUnclassified => {
            latest_admitted(path, bundled, fallback)
        }
    }
}

fn latest_admitted(
    path: &Path,
    bundled: &OptiScalerCompatibilityCatalog,
    fallback: &OptiScalerCompatibilityCatalog,
) -> Result<OptiScalerCompatibilityCatalog, ServiceError> {
    match observe_cache(path) {
        Ok(CacheObservation::Valid { value, .. }) if accepts(&value.catalog, bundled) => {
            Ok(value.catalog)
        }
        Ok(_) => Ok(fallback.clone()),
        Err(error) => {
            log::warn!("OptiScaler compatibility catalog re-observation failed: {error}");
            Ok(fallback.clone())
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AdmissionOrder {
    Older,
    Equal,
    Newer,
    Divergent,
}

fn compare_admission(
    candidate: &OptiScalerCompatibilityCatalog,
    accepted: &OptiScalerCompatibilityCatalog,
) -> AdmissionOrder {
    let (Ok(candidate_key), Ok(accepted_key)) = (
        revision_key(&candidate.revision),
        revision_key(&accepted.revision),
    ) else {
        return AdmissionOrder::Divergent;
    };
    match candidate_key.cmp(&accepted_key) {
        std::cmp::Ordering::Less => AdmissionOrder::Older,
        std::cmp::Ordering::Greater => AdmissionOrder::Newer,
        std::cmp::Ordering::Equal if **candidate == **accepted => AdmissionOrder::Equal,
        std::cmp::Ordering::Equal => AdmissionOrder::Divergent,
    }
}

fn accepts(
    candidate: &OptiScalerCompatibilityCatalog,
    bundled: &OptiScalerCompatibilityCatalog,
) -> bool {
    !matches!(
        compare_admission(candidate, bundled),
        AdmissionOrder::Older | AdmissionOrder::Divergent
    )
}

fn revision_key(value: &str) -> Result<Vec<u64>, ServiceError> {
    let normalized = value.replace('-', ".");
    let parts = normalized
        .split('.')
        .map(|part| {
            part.parse::<u64>()
                .map_err(|_| failed("invalid OptiScaler compatibility revision component"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    if parts.is_empty() {
        Err(failed("empty OptiScaler compatibility revision"))
    } else {
        Ok(parts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_catalog_matches_message_contract() {
        let catalog = parse_catalog(BUNDLED_CATALOG).expect("bundled compatibility catalog");
        let messages = parse_message_contract(BUNDLED_MESSAGES).expect("bundled message contract");

        assert_eq!(catalog.revision, messages.revision);
        assert!(!catalog.entries().is_empty());
    }

    #[test]
    fn invalid_message_revision_is_rejected() {
        let mut value: serde_json::Value =
            serde_json::from_slice(BUNDLED_MESSAGES).expect("messages");
        value["revision"] = serde_json::Value::String("not-a-revision".to_owned());
        let bytes = serde_json::to_vec(&value).expect("serialize");
        assert!(parse_message_contract(&bytes).is_err());
    }

    #[test]
    fn conditional_variants_require_disjoint_exact_predicates() {
        let mut value: serde_json::Value =
            serde_json::from_slice(BUNDLED_CATALOG).expect("catalog");
        let default = value["entries"][0]["variants"][0].clone();
        let mut steam = default.clone();
        steam["when"] = serde_json::json!({ "launcher": "steam" });
        let mut executable = default;
        executable["when"] = serde_json::json!({ "executable": "Game.exe" });
        value["entries"][0]["variants"] = serde_json::json!([
            value["entries"][0]["variants"][0].clone(),
            steam,
            executable
        ]);
        assert!(parse_catalog(&serde_json::to_vec(&value).expect("serialize")).is_err());
    }

    #[test]
    fn producer_invalid_launcher_values_are_rejected() {
        for launcher in ["proton", "cross_over", "whisky"] {
            let mut value: serde_json::Value =
                serde_json::from_slice(BUNDLED_CATALOG).expect("catalog");
            let default = value["entries"][0]["variants"][0].clone();
            let mut conditional = default.clone();
            conditional["when"] = serde_json::json!({ "launcher": launcher });
            value["entries"][0]["variants"] = serde_json::json!([default, conditional]);

            assert!(
                parse_catalog(&serde_json::to_vec(&value).expect("serialize")).is_err(),
                "{launcher} must not be admitted as a compatibility launcher"
            );
        }
    }

    #[test]
    fn producer_nvngx_proxy_slot_is_admitted() {
        let mut value: serde_json::Value =
            serde_json::from_slice(BUNDLED_CATALOG).expect("catalog");
        value["entries"][0]["variants"][0]["proxy"] =
            serde_json::json!({ "kind": "exact", "slot": "nvngx.dll" });

        parse_catalog(&serde_json::to_vec(&value).expect("serialize"))
            .expect("nvngx.dll is an allowed exact proxy slot");
    }

    #[test]
    fn same_revision_content_change_is_not_admitted() {
        let accepted = parse_catalog(BUNDLED_CATALOG).expect("catalog");
        let mut value: serde_json::Value =
            serde_json::from_slice(BUNDLED_CATALOG).expect("catalog JSON");
        value["upstream"]["source"] =
            serde_json::Value::String("different-reviewed-source".to_owned());
        let candidate = parse_catalog(&serde_json::to_vec(&value).expect("serialize"))
            .expect("valid independently shaped candidate");
        assert_eq!(
            compare_admission(&candidate, &accepted),
            AdmissionOrder::Divergent
        );
    }
}
