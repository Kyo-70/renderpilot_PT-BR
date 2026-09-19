use std::path::Path;
use std::{error::Error, fmt};

use serde::{Deserialize, Deserializer, Serialize};

use crate::text::{RequiredTextError, normalize_required_text};

const PATH_SEPARATOR: char = '/';
const WINDOWS_SEPARATOR: char = '\\';
const NUL: char = '\0';
const WINDOWS_DRIVE_ROOT_LEN: usize = 3;

/// Platform-neutral path reference stored as normalized UTF-8 text.
///
/// `PathRef` does not touch the filesystem and does not canonicalize paths.
/// Normalization is lexical only:
///
/// - surrounding whitespace is trimmed;
/// - backslashes are converted to `/`;
/// - redundant trailing separators are removed;
/// - root separators are preserved:
///   - `/` remains `/`;
///   - `D:\` becomes `D:/`, not `D:`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct PathRef(
    /// Stored normalized path text.
    String,
);

/// Lowercased, forward-slash comparison key for a path string.
///
/// Purely lexical — no filesystem access. Strips a Windows verbatim prefix
/// (`\\?\`) so canonicalized and raw forms compare equal. Used by domain
/// invariants and orchestration path maps; orchestration's `normalized_key`
/// for `std::path::Path` is a thin wrapper over this.
#[must_use]
pub fn normalized_path_key(path: &str) -> String {
    let normalized = path.replace('\\', "/").to_ascii_lowercase();
    if let Some(rest) = normalized.strip_prefix("//?/unc/") {
        format!("//{rest}")
    } else if let Some(rest) = normalized.strip_prefix("//?/") {
        rest.to_owned()
    } else {
        normalized
    }
}

pub mod capability;
pub mod durable_wire;
/// Purely lexical relationship between two normalized path spellings.
///
/// This deliberately does not touch the filesystem or perform dot-segment,
/// Unicode, or junction resolution.  The existing [`normalized_path_key`]
/// remains the sole normalization operation; this type only classifies the
/// resulting keys while respecting anchor boundaries.
pub mod relation;

impl PathRef {
    /// Creates a normalized path reference.
    pub fn new(value: impl Into<String>) -> Result<Self, PathRefError> {
        let value = normalize_required_text("path", value).map_err(PathRefError::from)?;

        validate_path_text(&value)?;

        Ok(Self(normalize_path_text(&value)))
    }

    /// Converts an already resolved native absolute path to the canonical
    /// durable wire spelling.
    ///
    /// This is lexical only. Filesystem/junction resolution belongs to the
    /// platform adapter; this method only turns that result into the one
    /// representation allowed in durable peer records.
    pub fn from_canonical_native_absolute(
        path: &Path,
    ) -> Result<Self, durable_wire::DurablePathWireError> {
        durable_wire::from_canonical_native_absolute(path)
    }

    /// Parses a path read from a durable wire record.
    ///
    /// Unlike PathRef::new, this rejects values which would need
    /// normalization.
    pub fn parse_exact(value: &str) -> Result<Self, durable_wire::DurablePathWireError> {
        durable_wire::parse_exact(value)
    }

    /// Returns the target-platform comparison key. The key is for lookup
    /// only and must never be serialized or included in a fingerprint.
    #[must_use]
    pub fn comparison_key(&self) -> String {
        durable_wire::comparison_key(self)
    }

    /// Returns normalized path text.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns the owned normalized path string.
    #[must_use]
    pub fn into_inner(self) -> String {
        self.0
    }

    /// Returns the final path component (the file name), when present.
    ///
    /// The path is already forward-slash normalized, so this is a pure lexical
    /// lookup that never touches the filesystem.
    pub fn file_name(&self) -> Option<&str> {
        Path::new(&self.0)
            .file_name()
            .and_then(|name| name.to_str())
    }

    /// Returns the parent directory as normalized text, when present.
    ///
    /// A bare file name yields `Some("")`; a root (`/`, `C:/`) yields `None`.
    pub fn parent(&self) -> Option<&str> {
        Path::new(&self.0)
            .parent()
            .and_then(|parent| parent.to_str())
    }
}

impl fmt::Display for PathRef {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl AsRef<str> for PathRef {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl From<PathRef> for String {
    fn from(path: PathRef) -> Self {
        path.into_inner()
    }
}

impl TryFrom<&str> for PathRef {
    type Error = PathRefError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl TryFrom<String> for PathRef {
    type Error = PathRefError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl<'de> Deserialize<'de> for PathRef {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;

        Self::new(value).map_err(serde::de::Error::custom)
    }
}

/// Error returned when a path reference cannot be normalized.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathRefError {
    /// Path text is empty after trimming whitespace.
    Empty,
    /// Path text contains a NUL byte.
    ContainsNul,
}

impl fmt::Display for PathRefError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("path cannot be empty"),
            Self::ContainsNul => formatter.write_str("path cannot contain NUL bytes"),
        }
    }
}

impl Error for PathRefError {}

impl From<RequiredTextError> for PathRefError {
    fn from(_: RequiredTextError) -> Self {
        Self::Empty
    }
}

fn validate_path_text(value: &str) -> Result<(), PathRefError> {
    if contains_nul(value) {
        return Err(PathRefError::ContainsNul);
    }

    Ok(())
}

fn contains_nul(value: &str) -> bool {
    value.contains(NUL)
}

fn normalize_path_text(value: &str) -> String {
    let mut normalized = normalize_path_separators(value);

    trim_redundant_trailing_separators(&mut normalized);

    normalized
}

fn normalize_path_separators(value: &str) -> String {
    value.replace(WINDOWS_SEPARATOR, PATH_SEPARATOR.encode_utf8(&mut [0; 4]))
}

fn trim_redundant_trailing_separators(path: &mut String) {
    while has_redundant_trailing_separator(path) {
        path.pop();
    }
}

fn has_redundant_trailing_separator(path: &str) -> bool {
    path.len() > 1 && path.ends_with(PATH_SEPARATOR) && !is_windows_drive_root(path)
}

fn is_windows_drive_root(path: &str) -> bool {
    let bytes = path.as_bytes();

    bytes.len() == WINDOWS_DRIVE_ROOT_LEN
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && bytes[2] == b'/'
}

#[cfg(test)]
mod tests {
    use super::{PathRef, PathRefError, normalized_path_key};
    use serde_json::json;

    #[test]
    fn path_ref_normalizes_windows_separators_and_trailing_slash() {
        let path = PathRef::new(r"  C:\Games\Cyberpunk 2077\ ").expect("valid path");

        assert_eq!(path.as_str(), "C:/Games/Cyberpunk 2077");
    }

    #[test]
    fn path_ref_preserves_windows_drive_root_slash() {
        let path = PathRef::new("D:\\").expect("valid Windows root");

        assert_eq!(path.as_str(), "D:/");
    }

    #[test]
    fn path_ref_preserves_windows_drive_root_with_forward_slash() {
        let path = PathRef::new("D:/").expect("valid Windows root");

        assert_eq!(path.as_str(), "D:/");
    }

    #[test]
    fn path_ref_trims_duplicate_trailing_separators_after_windows_drive_root() {
        let path = PathRef::new("D://").expect("valid Windows root");

        assert_eq!(path.as_str(), "D:/");
    }

    #[test]
    fn path_ref_preserves_unix_root() {
        let path = PathRef::new("/").expect("valid root");

        assert_eq!(path.as_str(), "/");
    }

    #[test]
    fn path_ref_trims_duplicate_unix_root_separators_to_single_root() {
        let path = PathRef::new("///").expect("valid root-like path");

        assert_eq!(path.as_str(), "/");
    }

    #[test]
    fn path_ref_removes_multiple_trailing_separators() {
        let path = PathRef::new("C:/Games/Game///").expect("valid path");

        assert_eq!(path.as_str(), "C:/Games/Game");
    }

    #[test]
    fn path_ref_preserves_internal_duplicate_separators() {
        let path = PathRef::new("C:/Games//Game").expect("valid path");

        assert_eq!(path.as_str(), "C:/Games//Game");
    }

    #[test]
    fn path_ref_does_not_canonicalize_relative_segments() {
        let path = PathRef::new("C:/Games/../Game").expect("valid lexical path");

        assert_eq!(path.as_str(), "C:/Games/../Game");
    }

    #[test]
    fn path_ref_rejects_blank_text() {
        let error = PathRef::new("  ").expect_err("blank path should fail");

        assert_eq!(error, PathRefError::Empty);
    }

    #[test]
    fn normalized_key_unifies_verbatim_drive_and_unc_paths() {
        assert_eq!(
            normalized_path_key(r"\\?\C:\Games\Example"),
            normalized_path_key("c:/games/example"),
        );
        assert_eq!(
            normalized_path_key(r"\\?\UNC\server\share\Game"),
            normalized_path_key(r"\\server\share\game"),
        );
    }

    #[test]
    fn path_ref_rejects_nul_bytes() {
        let error = PathRef::new("C:/Games/\0Game").expect_err("NUL should fail");

        assert_eq!(error, PathRefError::ContainsNul);
    }

    #[test]
    fn path_ref_deserialization_normalizes_input() {
        let path: PathRef =
            serde_json::from_str(r#""C:\\Games\\Cyberpunk 2077\\""#).expect("valid path json");

        assert_eq!(path.as_str(), "C:/Games/Cyberpunk 2077");
    }

    #[test]
    fn path_ref_serializes_as_plain_string() {
        let path = PathRef::new("C:/Games/Game").expect("valid path");

        let json = serde_json::to_string(&path).expect("path should serialize");

        assert_eq!(json, r#""C:/Games/Game""#);
    }

    #[test]
    fn path_ref_as_ref_returns_normalized_text() {
        let path = PathRef::new(r"C:\Games\Game\").expect("valid path");

        assert_eq!(path.as_ref(), "C:/Games/Game");
    }

    #[test]
    fn path_ref_file_name_returns_final_component() {
        assert_eq!(
            PathRef::new("/games/game/nvngx_dlss.dll")
                .unwrap()
                .file_name(),
            Some("nvngx_dlss.dll")
        );
        assert_eq!(
            PathRef::new("nvngx_dlss.dll").unwrap().file_name(),
            Some("nvngx_dlss.dll")
        );
    }

    #[test]
    fn path_ref_parent_returns_directory() {
        assert_eq!(
            PathRef::new("/games/game/nvngx_dlss.dll").unwrap().parent(),
            Some("/games/game")
        );
        // A bare file name has an empty parent; a root has none.
        assert_eq!(PathRef::new("nvngx_dlss.dll").unwrap().parent(), Some(""));
        assert_eq!(PathRef::new("/").unwrap().parent(), None);
    }

    #[cfg(windows)]
    #[test]
    fn path_ref_parent_preserves_windows_drive_root() {
        // Drive semantics only apply on Windows targets, where the bundle engine runs.
        assert_eq!(
            PathRef::new("C:/nvngx_dlss.dll").unwrap().parent(),
            Some("C:/")
        );
        assert_eq!(
            PathRef::new("C:/Games/Game/nvngx_dlss.dll")
                .unwrap()
                .parent(),
            Some("C:/Games/Game")
        );
    }

    #[test]
    fn normalized_path_key_strips_verbatim_prefix_and_normalizes() {
        assert_eq!(
            super::normalized_path_key(r"\\?\C:\Games\DLSS.dll"),
            "c:/games/dlss.dll"
        );
        assert_eq!(
            super::normalized_path_key(r"C:\Games\DLSS.dll"),
            super::normalized_path_key("C:/Games/DLSS.dll")
        );
    }

    #[test]
    fn durable_wire_renders_drive_and_unc_aliases() {
        let cases = [
            (r"c:\Games\Example", "C:/Games/Example"),
            ("C:/Games/Example", "C:/Games/Example"),
            (r"\\?\C:\Games\Example", "C:/Games/Example"),
            (r"\\server\share\Game", "//server/share/Game"),
            (r"\\?\UNC\server\share\Game", "//server/share/Game"),
            ("//server/share/Game", "//server/share/Game"),
        ];
        for (input, expected) in cases {
            assert_eq!(
                PathRef::from_canonical_native_absolute(std::path::Path::new(input))
                    .expect("native path")
                    .as_str(),
                expected
            );
        }
    }

    #[test]
    fn durable_wire_rejects_noncanonical_persisted_spellings() {
        for input in [
            r"C:\Games\Example",
            "//?/C:/Games/Example",
            "c:/Games/Example",
            "C:/Games/Example/",
            "C:/Games//Example",
            "C:/Games/../Example",
            "game/Example",
            "C:Example",
            "//server",
            "//?/GLOBALROOT/device",
        ] {
            assert!(PathRef::parse_exact(input).is_err(), "{input}");
        }
        for input in [
            "C:/Games/Example",
            "//server/share/Game",
            "/var/lib/renderpilot",
            "/",
            "C:/",
        ] {
            assert!(PathRef::parse_exact(input).is_ok(), "{input}");
        }
    }

    #[test]
    fn durable_wire_serde_adapter_is_strict_without_changing_general_pathref() {
        #[derive(serde::Deserialize, serde::Serialize)]
        struct StrictPath(#[serde(with = "super::durable_wire")] PathRef);

        assert!(serde_json::from_value::<StrictPath>(json!(r"C:\Games\Example")).is_err());
        assert_eq!(
            serde_json::from_value::<StrictPath>(json!("C:/Games/Example"))
                .expect("strict path")
                .0
                .as_str(),
            "C:/Games/Example"
        );
        assert!(serde_json::from_value::<PathRef>(json!(r"C:\Games\Example")).is_ok());
        let noncanonical = PathRef::new("c:/Games/Example").expect("permissive path");
        assert!(serde_json::to_value(StrictPath(noncanonical)).is_err());
    }

    #[test]
    fn durable_wire_comparison_key_is_platform_specific_and_not_wire_text() {
        let path = PathRef::parse_exact("C:/Games/Example").expect("path");
        let key = path.comparison_key();
        if cfg!(windows) {
            assert_eq!(key, "c:/games/example");
        } else {
            assert_eq!(key, "C:/Games/Example");
        }
        assert_eq!(path.as_str(), "C:/Games/Example");
    }
}
