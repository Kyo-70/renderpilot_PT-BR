use base64::{Engine as _, engine::general_purpose::STANDARD};
use renderpilot_application::AppResult;
use renderpilot_domain::OptiScalerConfigurationBaseline;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::error::{invalid_row, storage_context};

const FORMAT_TAG: &str = "renderpilot.optiscaler.configuration-baseline";
const REVISION: u8 = 1;

#[derive(Debug, Serialize)]
#[serde(deny_unknown_fields)]
struct BaselineWire<'a> {
    format_tag: &'static str,
    revision: u8,
    kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    receipt: Option<&'a renderpilot_domain::FileReceipt>,
    #[serde(skip_serializing_if = "Option::is_none")]
    length: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    bytes: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DecodedBaselineWire {
    format_tag: String,
    revision: u8,
    kind: String,
    receipt: Option<renderpilot_domain::FileReceipt>,
    length: Option<u64>,
    bytes: Option<String>,
}

pub(crate) fn encode(baseline: &OptiScalerConfigurationBaseline) -> AppResult<String> {
    let wire = match baseline {
        OptiScalerConfigurationBaseline::Absent => BaselineWire {
            format_tag: FORMAT_TAG,
            revision: REVISION,
            kind: "absent",
            receipt: None,
            length: None,
            bytes: None,
        },
        OptiScalerConfigurationBaseline::Present {
            receipt,
            length,
            bytes,
        } => BaselineWire {
            format_tag: FORMAT_TAG,
            revision: REVISION,
            kind: "present",
            receipt: Some(receipt),
            length: Some(*length),
            bytes: Some(STANDARD.encode(bytes)),
        },
    };
    serde_json::to_string(&wire)
        .map_err(|error| storage_context("could not serialize OptiScaler baseline", error))
}

pub(crate) fn decode(value: &str) -> AppResult<OptiScalerConfigurationBaseline> {
    let parsed = serde_json::from_str::<Value>(value)
        .map_err(|error| invalid_row(format!("invalid OptiScaler baseline JSON: {error}")))?;
    let object = parsed
        .as_object()
        .ok_or_else(|| invalid_row("OptiScaler baseline JSON must be an object"))?;
    let kind = object.get("kind").and_then(Value::as_str);
    let expected_keys = match kind {
        Some("absent") => ["format_tag", "revision", "kind"].as_slice(),
        Some("present") => [
            "format_tag",
            "revision",
            "kind",
            "receipt",
            "length",
            "bytes",
        ]
        .as_slice(),
        _ => return Err(invalid_row("invalid OptiScaler baseline kind")),
    };
    if !has_exact_keys(object, expected_keys) {
        return Err(invalid_row(
            "OptiScaler baseline has missing or extraneous fields",
        ));
    }
    let wire: DecodedBaselineWire = serde_json::from_value(parsed)
        .map_err(|error| invalid_row(format!("invalid OptiScaler baseline JSON: {error}")))?;
    if wire.format_tag != FORMAT_TAG || wire.revision != REVISION {
        return Err(invalid_row("unsupported OptiScaler baseline format"));
    }
    match wire.kind.as_str() {
        "absent" if wire.receipt.is_none() && wire.length.is_none() && wire.bytes.is_none() => {
            Ok(OptiScalerConfigurationBaseline::Absent)
        }
        "present" => {
            let receipt = wire
                .receipt
                .ok_or_else(|| invalid_row("present OptiScaler baseline has no receipt"))?;
            let length = wire
                .length
                .ok_or_else(|| invalid_row("present OptiScaler baseline has no length"))?;
            let encoded = wire
                .bytes
                .ok_or_else(|| invalid_row("present OptiScaler baseline has no bytes"))?;
            let bytes = STANDARD.decode(encoded.as_bytes()).map_err(|error| {
                invalid_row(format!("invalid OptiScaler baseline base64: {error}"))
            })?;
            if STANDARD.encode(&bytes) != encoded {
                return Err(invalid_row(
                    "OptiScaler baseline bytes are not canonical base64",
                ));
            }
            OptiScalerConfigurationBaseline::from_parts(receipt, length, bytes).map_err(invalid_row)
        }
        _ => Err(invalid_row("invalid OptiScaler baseline kind")),
    }
}

fn has_exact_keys(object: &Map<String, Value>, expected: &[&str]) -> bool {
    object.len() == expected.len() && expected.iter().all(|key| object.contains_key(*key))
}

#[cfg(test)]
mod tests {
    use super::*;
    use renderpilot_domain::FileReceipt;

    fn present() -> OptiScalerConfigurationBaseline {
        let bytes = b"[OptiScaler]\n".to_vec();
        let digest = renderpilot_detection::sha256_bytes(&bytes).expect("digest");
        OptiScalerConfigurationBaseline::present(
            FileReceipt::reused("config-id", digest).expect("receipt"),
            bytes,
        )
        .expect("baseline")
    }

    #[test]
    fn round_trip_uses_canonical_base64() {
        let encoded = encode(&present()).expect("encode");
        assert!(encoded.contains("\"bytes\":\"W09wdGlTY2FsZXJdCg==\""));
        assert_eq!(decode(&encoded).expect("decode"), present());
    }

    #[test]
    fn absent_round_trip_has_no_nullable_baseline_fields() {
        let encoded = encode(&OptiScalerConfigurationBaseline::Absent).expect("encode");
        assert_eq!(
            encoded,
            r#"{"format_tag":"renderpilot.optiscaler.configuration-baseline","revision":1,"kind":"absent"}"#
        );
        assert_eq!(
            decode(&encoded).expect("decode"),
            OptiScalerConfigurationBaseline::Absent
        );
    }

    #[test]
    fn malformed_and_noncanonical_values_fail_closed() {
        let encoded = encode(&present()).expect("encode");
        assert!(decode(&encoded.replace("==", "")).is_err());
        assert!(decode(&encoded.replace("kind", "foreign")).is_err());
        assert!(decode(&encoded.replace("format_tag", "unknown")).is_err());
        assert!(decode(&encoded.replace("\"bytes\":\"", "\"unknown\":true,\"bytes\":\"")).is_err());
        assert!(decode(
            r#"{"format_tag":"renderpilot.optiscaler.configuration-baseline","revision":1,"kind":"absent","receipt":null}"#
        )
        .is_err());
        assert!(
            decode(&encoded.replace("\"bytes\":\"W09wdGlTY2FsZXJdCg==\"", "\"bytes\":null"))
                .is_err()
        );
    }
}
