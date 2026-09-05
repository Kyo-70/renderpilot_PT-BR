use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer, ser::SerializeStruct};
use sha2::{Digest, Sha256};

use super::{FileOwnership, FileReceipt, OptiScalerStateError};
use crate::Sha256Hash;

pub(super) const MAX_CONFIGURATION_BASELINE_BYTES: usize = 16 * 1024 * 1024;

/// Immutable bytes captured before the first OptiScaler configuration write.
///
/// The receipt is always reused custody: the bytes belonged to the user before
/// OptiScaler touched the path and can only be restored, never removed as an
/// OptiScaler-owned file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OptiScalerConfigurationBaseline {
    /// `target_dir/OptiScaler.ini` did not exist before the operation.
    Absent,
    /// Exact user-owned preimage retained for a configuration replacement.
    Present {
        /// Immutable evidence for the pre-operation file.
        receipt: FileReceipt,
        /// Number of bytes retained in `bytes`.
        length: u64,
        /// Exact bytes observed before the first configuration write.
        bytes: Vec<u8>,
    },
}

impl OptiScalerConfigurationBaseline {
    /// Creates an absent configuration baseline.
    #[must_use]
    pub const fn absent() -> Self {
        Self::Absent
    }

    /// Creates and validates a present configuration baseline.
    pub fn present(receipt: FileReceipt, bytes: Vec<u8>) -> Result<Self, OptiScalerStateError> {
        let value = Self::Present {
            length: bytes.len() as u64,
            receipt,
            bytes,
        };
        value.validate().map(|()| value)
    }

    /// Reconstructs and validates a present baseline with an explicit length.
    pub fn from_parts(
        receipt: FileReceipt,
        length: u64,
        bytes: Vec<u8>,
    ) -> Result<Self, OptiScalerStateError> {
        let value = Self::Present {
            receipt,
            length,
            bytes,
        };
        value.validate().map(|()| value)
    }

    /// Returns the exact retained preimage bytes, when the path was present.
    #[must_use]
    pub fn bytes(&self) -> Option<&[u8]> {
        match self {
            Self::Absent => None,
            Self::Present { bytes, .. } => Some(bytes),
        }
    }

    /// Returns the declared retained preimage length, when the path was present.
    #[must_use]
    pub const fn length(&self) -> Option<u64> {
        match self {
            Self::Absent => None,
            Self::Present { length, .. } => Some(*length),
        }
    }

    /// Returns the exact retained preimage receipt, when the path was present.
    #[must_use]
    pub fn receipt(&self) -> Option<&FileReceipt> {
        match self {
            Self::Absent => None,
            Self::Present { receipt, .. } => Some(receipt),
        }
    }

    pub(super) fn validate(&self) -> Result<(), OptiScalerStateError> {
        let Self::Present {
            receipt,
            length,
            bytes,
        } = self
        else {
            return Ok(());
        };
        receipt.validate()?;
        if receipt.ownership() != FileOwnership::Reused {
            return Err(OptiScalerStateError::ConfigurationBaselineNotReused);
        }
        if *length != bytes.len() as u64 {
            return Err(OptiScalerStateError::ConfigurationBaselineLengthMismatch);
        }
        if bytes.len() > MAX_CONFIGURATION_BASELINE_BYTES {
            return Err(OptiScalerStateError::ConfigurationBaselineOversize);
        }
        let digest = Sha256Hash::new(hex_digest(bytes))
            .map_err(|_| OptiScalerStateError::ConfigurationBaselineDigestMismatch)?;
        if &digest != receipt.digest() {
            return Err(OptiScalerStateError::ConfigurationBaselineDigestMismatch);
        }
        Ok(())
    }
}

impl Serialize for OptiScalerConfigurationBaseline {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut value = serializer.serialize_struct("OptiScalerConfigurationBaseline", 4)?;
        match self {
            Self::Absent => {
                value.serialize_field("kind", "absent")?;
                value.serialize_field("receipt", &Option::<FileReceipt>::None)?;
                value.serialize_field("length", &Option::<u64>::None)?;
                value.serialize_field("bytes", &Option::<Vec<u8>>::None)?;
            }
            Self::Present {
                receipt,
                length,
                bytes,
            } => {
                value.serialize_field("kind", "present")?;
                value.serialize_field("receipt", receipt)?;
                value.serialize_field("length", length)?;
                value.serialize_field("bytes", bytes)?;
            }
        }
        value.end()
    }
}

impl<'de> Deserialize<'de> for OptiScalerConfigurationBaseline {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            kind: String,
            receipt: Option<FileReceipt>,
            length: Option<u64>,
            bytes: Option<Vec<u8>>,
        }
        let wire = Wire::deserialize(deserializer)?;
        let value = match wire.kind.as_str() {
            "absent" if wire.receipt.is_none() && wire.length.is_none() && wire.bytes.is_none() => {
                Self::Absent
            }
            "present" => Self::Present {
                receipt: wire.receipt.ok_or_else(|| {
                    serde::de::Error::custom("configuration baseline receipt is missing")
                })?,
                length: wire.length.ok_or_else(|| {
                    serde::de::Error::custom("configuration baseline length is missing")
                })?,
                bytes: wire.bytes.ok_or_else(|| {
                    serde::de::Error::custom("configuration baseline bytes are missing")
                })?,
            },
            _ => {
                return Err(serde::de::Error::custom(
                    "configuration baseline kind is invalid",
                ));
            }
        };
        value.validate().map_err(serde::de::Error::custom)?;
        Ok(value)
    }
}

fn hex_digest(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(Sha256Hash::HEX_LENGTH);
    for byte in Sha256::digest(bytes) {
        use fmt::Write;
        let _ = write!(output, "{byte:02x}");
    }
    output
}
