//! Strict codec and validation for durable (persisted) path references used in
//! peer records and storage.

use super::PathRef;
use serde::{Deserialize, Deserializer, Serializer};
use std::{error::Error, fmt, path::Path};

/// Failure returned by the strict durable path codec.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DurablePathWireError {
    /// Input is empty.
    Empty,
    /// Input has surrounding whitespace.
    Whitespace,
    /// Input contains a NUL character.
    ContainsNul,
    /// Input is not absolute.
    Relative,
    /// Input contains a traversal component.
    Traversal,
    /// Input contains an empty interior component.
    EmptyComponent,
    /// Input has an invalid drive prefix.
    InvalidDrive,
    /// Input has fewer than server and share UNC components.
    IncompleteUnc,
    /// Input uses a device namespace.
    DeviceNamespace,
    /// Input is not valid UTF-8.
    NonUnicode,
    /// Input is valid but not the canonical wire spelling.
    NonCanonical,
}

impl fmt::Display for DurablePathWireError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::Empty => "durable path is empty",
            Self::Whitespace => "durable path has surrounding whitespace",
            Self::ContainsNul => "durable path contains NUL",
            Self::Relative => "durable path is not absolute",
            Self::Traversal => "durable path contains traversal",
            Self::EmptyComponent => "durable path contains an empty component",
            Self::InvalidDrive => "durable path has an invalid drive",
            Self::IncompleteUnc => "durable path has an incomplete UNC prefix",
            Self::DeviceNamespace => "durable path uses a device namespace",
            Self::NonUnicode => "durable path is not valid Unicode",
            Self::NonCanonical => "durable path is not in canonical wire spelling",
        };
        formatter.write_str(message)
    }
}

impl Error for DurablePathWireError {}

/// Renders a native absolute path into canonical durable text.
pub fn from_canonical_native_absolute(path: &Path) -> Result<PathRef, DurablePathWireError> {
    let value = path.to_str().ok_or(DurablePathWireError::NonUnicode)?;
    let rendered = render(value)?;
    PathRef::new(rendered).map_err(|_| DurablePathWireError::Empty)
}

/// Parses a durable path and rejects every non-canonical spelling.
pub fn parse_exact(value: &str) -> Result<PathRef, DurablePathWireError> {
    let rendered = render(value)?;
    if rendered != value {
        return Err(DurablePathWireError::NonCanonical);
    }
    PathRef::new(rendered).map_err(|_| DurablePathWireError::Empty)
}

/// Produces a target-platform lookup key. This value is not wire data.
#[must_use]
pub fn comparison_key(path: &PathRef) -> String {
    if cfg!(windows) {
        path.as_str().to_ascii_lowercase()
    } else {
        path.as_str().to_owned()
    }
}

/// Returns true if the path uses Windows drive or UNC root syntax.
#[must_use]
pub fn is_windows_path(path: &str) -> bool {
    let replaced = path.replace('\\', "/");
    matches!(
        parse_prefix(&replaced),
        Ok((Prefix::Drive | Prefix::Unc, _))
    )
}

/// Serde adapter for strict durable path fields.
pub fn serialize<S>(path: &PathRef, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let path = parse_exact(path.as_str()).map_err(serde::ser::Error::custom)?;
    serializer.serialize_str(path.as_str())
}

/// Serde adapter for strict durable path fields.
pub fn deserialize<'de, D>(deserializer: D) -> Result<PathRef, D::Error>
where
    D: Deserializer<'de>,
{
    let value = String::deserialize(deserializer)?;
    parse_exact(&value).map_err(serde::de::Error::custom)
}

/// Serde adapter for a vector of strict durable path fields.
pub mod vec {
    use super::{PathRef, parse_exact};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    /// Serializes a vector of canonical path references.
    pub fn serialize<S>(paths: &[PathRef], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let values = paths
            .iter()
            .map(|path| {
                parse_exact(path.as_str())
                    .map(|path| path.as_str().to_owned())
                    .map_err(serde::ser::Error::custom)
            })
            .collect::<Result<Vec<_>, S::Error>>()?;
        values.serialize(serializer)
    }

    /// Deserializes and strictly validates a vector of path references.
    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<PathRef>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let values = Vec::<String>::deserialize(deserializer)?;
        values
            .iter()
            .map(|value| parse_exact(value).map_err(serde::de::Error::custom))
            .collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Prefix {
    Posix,
    Drive,
    Unc,
}

fn parse_prefix(normalized: &str) -> Result<(Prefix, &str), DurablePathWireError> {
    if let Some(rest) = normalized.strip_prefix("//?/UNC/") {
        Ok((Prefix::Unc, rest))
    } else if let Some(rest) = normalized.strip_prefix("//?/") {
        if rest.len() >= 2 && rest.as_bytes()[0].is_ascii_alphabetic() && rest.as_bytes()[1] == b':'
        {
            Ok((Prefix::Drive, rest))
        } else {
            Err(DurablePathWireError::DeviceNamespace)
        }
    } else if normalized.starts_with("//./") || normalized == "//." {
        Err(DurablePathWireError::DeviceNamespace)
    } else if let Some(rest) = normalized.strip_prefix("//") {
        Ok((Prefix::Unc, rest))
    } else if let Some(rest) = normalized.strip_prefix('/') {
        Ok((Prefix::Posix, rest))
    } else if normalized.len() >= 2
        && normalized.as_bytes()[0].is_ascii_alphabetic()
        && normalized.as_bytes()[1] == b':'
    {
        Ok((Prefix::Drive, normalized))
    } else {
        Err(DurablePathWireError::Relative)
    }
}

fn render(value: &str) -> Result<String, DurablePathWireError> {
    if value.is_empty() {
        return Err(DurablePathWireError::Empty);
    }
    if value != value.trim() {
        return Err(DurablePathWireError::Whitespace);
    }
    if value.contains('\0') {
        return Err(DurablePathWireError::ContainsNul);
    }

    let replaced = value.replace('\\', "/");
    let (kind, body) = parse_prefix(&replaced)?;

    match kind {
        Prefix::Posix => {
            let body = trim_trailing_separators(body);
            if body.is_empty() {
                return Ok("/".to_owned());
            }
            validate_components(body)?;
            Ok(format!("/{body}"))
        }
        Prefix::Drive => {
            if body.len() < 3
                || !body.as_bytes()[0].is_ascii_alphabetic()
                || body.as_bytes()[1] != b':'
                || body.as_bytes()[2] != b'/'
            {
                return Err(DurablePathWireError::InvalidDrive);
            }
            let drive = body.as_bytes()[0].to_ascii_uppercase() as char;
            let components = trim_trailing_separators(&body[3..]);
            validate_components(components)?;
            if components.is_empty() {
                Ok(format!("{drive}:/"))
            } else {
                Ok(format!("{drive}:/{components}"))
            }
        }
        Prefix::Unc => {
            let body = trim_trailing_separators(body);
            let mut components = body.split('/');
            let server = components.next().filter(|part| !part.is_empty());
            let share = components.next().filter(|part| !part.is_empty());
            let (Some(server), Some(share)) = (server, share) else {
                return Err(DurablePathWireError::IncompleteUnc);
            };
            if server == "." || server == ".." || share == "." || share == ".." {
                return Err(DurablePathWireError::Traversal);
            }
            let tail = components.collect::<Vec<_>>();
            if tail.iter().any(|part| part.is_empty()) {
                return Err(DurablePathWireError::EmptyComponent);
            }
            if tail.iter().any(|part| *part == "." || *part == "..") {
                return Err(DurablePathWireError::Traversal);
            }
            if tail.is_empty() {
                Ok(format!("//{server}/{share}"))
            } else {
                Ok(format!("//{server}/{share}/{}", tail.join("/")))
            }
        }
    }
}

fn validate_components(value: &str) -> Result<(), DurablePathWireError> {
    if value.is_empty() {
        return Ok(());
    }
    for part in value.split('/') {
        if part.is_empty() {
            return Err(DurablePathWireError::EmptyComponent);
        }
        if part == "." || part == ".." {
            return Err(DurablePathWireError::Traversal);
        }
    }
    Ok(())
}

fn trim_trailing_separators(value: &str) -> &str {
    value.trim_end_matches('/')
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
        struct StrictPath(#[serde(with = "crate::path::durable_wire")] PathRef);

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

    #[test]
    fn posix_trailing_slashes_are_trimmed_in_render_and_rejected_in_exact_wire() {
        assert_eq!(
            PathRef::from_canonical_native_absolute(std::path::Path::new("/var/lib/renderpilot/"))
                .expect("posix rendered")
                .as_str(),
            "/var/lib/renderpilot"
        );
        assert_eq!(
            PathRef::from_canonical_native_absolute(std::path::Path::new("/"))
                .expect("posix root")
                .as_str(),
            "/"
        );
        assert!(PathRef::parse_exact("/var/lib/renderpilot/").is_err());
        assert!(PathRef::parse_exact("/var/lib/renderpilot").is_ok());
        assert!(PathRef::parse_exact("/").is_ok());
    }

    #[test]
    fn is_windows_path_recognizes_drives_and_unc_strictly() {
        assert!(is_windows_path("C:/Games"));
        assert!(is_windows_path("d:/Games"));
        assert!(is_windows_path(r"C:\Games"));
        assert!(is_windows_path("//server/share"));
        assert!(is_windows_path(r"\\server\share"));
        assert!(!is_windows_path("/var/games"));
        assert!(!is_windows_path("1:/games"));
        assert!(!is_windows_path("?:/games"));
    }
}
