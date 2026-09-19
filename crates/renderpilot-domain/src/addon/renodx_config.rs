//! Durable, RenoDX-specific provenance for the optional ReShade.ini Set_Path key.

use serde::{Deserialize, Serialize};

use crate::PathRef;

/// Current serialized shape of a RenoDX Set_Path receipt.
pub const RENODX_CONFIG_RECEIPT_SCHEMA_VERSION: u8 = 1;

/// The exact logical value that existed before RenoDX managed Set_Path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum RenoDxSetPathBaseline {
    /// Set_Path was absent from the unique `[renodx]` section.
    Absent,
    /// Set_Path existed; the value is retained exactly as a logical RHS.
    Present {
        /// The exact trimmed right-hand-side value from before the install.
        value: String,
    },
}

/// The typed value last written by RenderPilot. Only 0 and 1 are valid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RenoDxSetPathValue {
    /// The native RenoDX path (`Set_Path=0`).
    Zero,
    /// The upgrade RenoDX path (`Set_Path=1`).
    One,
}

impl RenoDxSetPathValue {
    /// Returns the only wire representation accepted for this typed value.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Zero => "0",
            Self::One => "1",
        }
    }
}

/// Narrow receipt for one exact ReShade.ini path and one managed RenoDX key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenoDxConfigReceipt {
    /// Receipt schema version.
    pub schema_version: u8,
    /// Exact path of the ReShade.ini that was planned.
    pub ini_path: PathRef,
    /// Value captured before RenderPilot took ownership.
    pub baseline: RenoDxSetPathBaseline,
    /// Whether the target `[renodx]` section existed before the write.
    pub section_preexisted: bool,
    /// Last typed value written by RenderPilot.
    pub last_written: RenoDxSetPathValue,
    /// Exact body of the anchor line that was given a trailing newline by RenderPilot.
    #[serde(default)]
    pub newline_anchor: Option<String>,
}

impl RenoDxConfigReceipt {
    /// Builds a receipt for one exact ReShade.ini path.
    #[must_use]
    pub fn new(
        ini_path: PathRef,
        baseline: RenoDxSetPathBaseline,
        section_preexisted: bool,
        last_written: RenoDxSetPathValue,
    ) -> Self {
        Self {
            schema_version: RENODX_CONFIG_RECEIPT_SCHEMA_VERSION,
            ini_path,
            baseline,
            section_preexisted,
            last_written,
            newline_anchor: None,
        }
    }

    /// Sets the anchor line that was given a trailing newline by RenderPilot.
    #[must_use]
    pub fn with_newline_anchor(mut self, anchor: Option<String>) -> Self {
        self.newline_anchor = anchor;
        self
    }

    /// Returns whether this receipt can be acted on by the current planner.
    #[must_use]
    pub fn is_supported(&self) -> bool {
        self.schema_version == RENODX_CONFIG_RECEIPT_SCHEMA_VERSION
            && self
                .ini_path
                .file_name()
                .is_some_and(|name| name.eq_ignore_ascii_case("ReShade.ini"))
            && match &self.baseline {
                RenoDxSetPathBaseline::Absent => self
                    .newline_anchor
                    .as_deref()
                    .is_none_or(|anchor| !anchor.contains(['\0', '\n'])),
                // The baseline is an opaque logical RHS.  An empty RHS and
                // embedded `=` are both valid INI values; only NUL is unsafe
                // for the byte-preserving planner/receipt boundary.
                // In addition, if a key was present, its section must have pre-existed,
                // and no newline anchor could have been added.
                RenoDxSetPathBaseline::Present { value } => {
                    self.section_preexisted
                        && self.newline_anchor.is_none()
                        && !value.contains('\0')
                }
            }
    }
}
