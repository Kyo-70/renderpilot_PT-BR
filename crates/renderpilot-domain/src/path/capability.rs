//! Opaque root-relative capability tokens used by strict peer programs.

use super::{PathRef, durable_wire};
use serde::Serialize;
use std::{error::Error, fmt};

/// A validated root-relative peer capability.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct CapabilityToken(String);

impl CapabilityToken {
    /// Lowers one canonical physical path against the exact declared
    /// roots.  Filesystem resolution must already have happened before
    /// this value-level operation.
    pub fn from_path(path: &PathRef, roots: &[String]) -> Result<Self, CapabilityTokenError> {
        let path =
            durable_wire::parse_exact(path.as_str()).map_err(CapabilityTokenError::PhysicalPath)?;
        let roots = validated_roots(roots)?;
        let path_key = path.comparison_key();
        let Some((root, root_key)) = roots
            .iter()
            .find(|(_, root_key)| same_or_descendant(&path_key, root_key))
        else {
            return Err(CapabilityTokenError::OutsideDeclaredRoots);
        };
        let relative = relative_component(path.as_str(), root.as_str())
            .or_else(|| relative_component(&path_key, root_key))
            .ok_or(CapabilityTokenError::OutsideDeclaredRoots)?;
        validate_relative(relative)?;
        let canonical_relative = canonical_relative(root.as_str(), relative);
        Ok(Self(format!("{}:{canonical_relative}", root.as_str())))
    }

    /// Parses one persisted token against the exact declared roots.
    /// Matching starts with an exact root prefix; the token is never
    /// split on an arbitrary colon.
    pub fn parse_exact(value: &str, roots: &[String]) -> Result<Self, CapabilityTokenError> {
        let roots = validated_roots(roots)?;
        let (root, relative) = roots
            .iter()
            .find_map(|(root, _)| {
                value
                    .strip_prefix(root.as_str())
                    .and_then(|suffix| suffix.strip_prefix(':'))
                    .map(|relative| (root, relative))
            })
            .ok_or(CapabilityTokenError::UnknownRootPrefix)?;

        validate_relative(relative)?;
        if !is_canonical_relative(root.as_str(), relative) {
            return Err(CapabilityTokenError::NonCanonical);
        }
        Ok(Self(value.to_owned()))
    }

    /// Checks that roots are strict canonical paths and pairwise
    /// disjoint in the target-platform comparison space.
    pub fn validate_roots(roots: &[String]) -> Result<(), CapabilityTokenError> {
        validated_roots(roots).map(|_| ())
    }

    /// Returns the canonical wire token.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for CapabilityToken {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for CapabilityToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Failure returned by the capability grammar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CapabilityTokenError {
    /// No roots were supplied.
    EmptyRoots,
    /// A declared root is not a strict canonical durable path.
    InvalidRoot(durable_wire::DurablePathWireError),
    /// Two roots have the same target-platform identity.
    DuplicateRoots,
    /// One root contains another in the target-platform comparison space.
    OverlappingRoots,
    /// The physical path used for lowering is not strict canonical text.
    PhysicalPath(durable_wire::DurablePathWireError),
    /// The token does not begin with one exact declared root and colon.
    UnknownRootPrefix,
    /// The token has no relative component.
    EmptyRelative,
    /// The relative component contains an empty path component.
    InvalidRelativeComponent,
    /// The relative component contains `.` or `..`.
    TraversalComponent,
    /// Backslashes are not valid capability separators.
    ContainsBackslash,
    /// Colons are reserved for the root/token boundary.
    ContainsColon,
    /// NUL is never valid in a capability.
    ContainsNul,
    /// The token spelling is not canonical for this target platform.
    NonCanonical,
    /// The physical path is not below one declared root.
    OutsideDeclaredRoots,
}

impl fmt::Display for CapabilityTokenError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::EmptyRoots => "capability roots are empty",
            Self::InvalidRoot(error) => {
                return write!(formatter, "invalid capability root: {error}");
            }
            Self::DuplicateRoots => "capability roots contain duplicates",
            Self::OverlappingRoots => "capability roots overlap",
            Self::PhysicalPath(error) => {
                return write!(formatter, "invalid capability path: {error}");
            }
            Self::UnknownRootPrefix => "capability does not use an exact declared root",
            Self::EmptyRelative => "capability relative component is empty",
            Self::InvalidRelativeComponent => "capability relative component is invalid",
            Self::TraversalComponent => "capability relative component contains traversal",
            Self::ContainsBackslash => "capability contains a backslash",
            Self::ContainsColon => "capability relative component contains a colon",
            Self::ContainsNul => "capability contains NUL",
            Self::NonCanonical => "capability is not in canonical wire spelling",
            Self::OutsideDeclaredRoots => "capability path is outside declared roots",
        };
        formatter.write_str(message)
    }
}

impl Error for CapabilityTokenError {}

type ValidatedRoot = (PathRef, String);

fn validated_roots(roots: &[String]) -> Result<Vec<ValidatedRoot>, CapabilityTokenError> {
    if roots.is_empty() {
        return Err(CapabilityTokenError::EmptyRoots);
    }
    let mut validated = Vec::with_capacity(roots.len());
    for root in roots {
        let path = durable_wire::parse_exact(root).map_err(CapabilityTokenError::InvalidRoot)?;
        let key = path.comparison_key();
        if validated.iter().any(|(_, existing)| existing == &key) {
            return Err(CapabilityTokenError::DuplicateRoots);
        }
        validated.push((path, key));
    }
    for (index, (_, left)) in validated.iter().enumerate() {
        if validated
            .iter()
            .skip(index + 1)
            .any(|(_, right)| same_or_descendant(left, right) || same_or_descendant(right, left))
        {
            return Err(CapabilityTokenError::OverlappingRoots);
        }
    }
    Ok(validated)
}

fn same_or_descendant(path: &str, root: &str) -> bool {
    path == root
        || if root.ends_with('/') {
            path.starts_with(root)
        } else {
            path.strip_prefix(root)
                .is_some_and(|rest| rest.starts_with('/'))
        }
}

fn relative_component<'a>(path: &'a str, root: &str) -> Option<&'a str> {
    if path == root {
        return Some(".");
    }
    if root.ends_with('/') {
        path.strip_prefix(root)
    } else {
        path.strip_prefix(root)
            .and_then(|rest| rest.strip_prefix('/'))
    }
}

fn canonical_relative(root: &str, value: &str) -> String {
    if durable_wire::is_windows_path(root) {
        value.to_ascii_lowercase()
    } else {
        value.to_owned()
    }
}

fn is_canonical_relative(root: &str, value: &str) -> bool {
    if durable_wire::is_windows_path(root) {
        !value.bytes().any(|byte| byte.is_ascii_uppercase())
    } else {
        true
    }
}

fn validate_relative(value: &str) -> Result<(), CapabilityTokenError> {
    if value.is_empty() {
        return Err(CapabilityTokenError::EmptyRelative);
    }
    if value == "." {
        return Ok(());
    }
    if value.contains('\\') {
        return Err(CapabilityTokenError::ContainsBackslash);
    }
    if value.contains(':') {
        return Err(CapabilityTokenError::ContainsColon);
    }
    if value.contains('\0') {
        return Err(CapabilityTokenError::ContainsNul);
    }
    for component in value.split('/') {
        if component.is_empty() {
            return Err(CapabilityTokenError::InvalidRelativeComponent);
        }
        if component == "." || component == ".." {
            return Err(CapabilityTokenError::TraversalComponent);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{CapabilityToken, CapabilityTokenError};
    use crate::PathRef;

    fn roots(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn exact_root_uses_dot_and_child_uses_declared_root_prefix() {
        let roots = roots(&["C:/Games/Example"]);
        let root = PathRef::parse_exact("C:/Games/Example").expect("root");
        let child = PathRef::parse_exact("C:/Games/Example/ReShade64.dll").expect("child");

        assert_eq!(
            CapabilityToken::from_path(&root, &roots).unwrap().as_str(),
            "C:/Games/Example:."
        );
        assert_eq!(
            CapabilityToken::from_path(&child, &roots).unwrap().as_str(),
            "C:/Games/Example:reshade64.dll"
        );
        assert_eq!(
            CapabilityToken::parse_exact("C:/Games/Example:reshade64.dll", &roots)
                .unwrap()
                .as_str(),
            "C:/Games/Example:reshade64.dll"
        );
    }

    #[test]
    fn posix_relative_component_preserves_case() {
        let roots = roots(&["/Games/Example"]);
        let path = PathRef::parse_exact("/Games/Example/ReShade64.dll").expect("path");

        assert_eq!(
            CapabilityToken::from_path(&path, &roots)
                .expect("capability")
                .as_str(),
            "/Games/Example:ReShade64.dll"
        );
    }

    #[test]
    fn windows_relative_component_is_lowercase_but_root_is_exact() {
        let roots = roots(&["C:/Games/Example"]);
        assert_eq!(
            CapabilityToken::parse_exact("C:/Games/Example:reshade64.dll", &roots)
                .unwrap()
                .as_str(),
            "C:/Games/Example:reshade64.dll"
        );
        assert_eq!(
            CapabilityToken::parse_exact("C:/Games/Example:ReShade64.dll", &roots).unwrap_err(),
            CapabilityTokenError::NonCanonical
        );
        assert_eq!(
            CapabilityToken::parse_exact("c:/Games/Example:reshade64.dll", &roots).unwrap_err(),
            CapabilityTokenError::UnknownRootPrefix
        );
    }

    #[test]
    fn parser_is_prefix_based_and_rejects_ambiguous_or_malformed_values() {
        let declared_roots = roots(&["C:/Games/Example"]);
        for value in [
            "C:/Games/Example2:file.dll",
            "C:/Games/Example:file/../dll",
            "C:/Games/Example:file\\dll",
            "C:/Games/Example:file::dll",
            "C:/Games/Example:file//dll",
            "C:/Games/Example:file/./dll",
            "C:/Games/Example:",
        ] {
            assert!(
                CapabilityToken::parse_exact(value, &declared_roots).is_err(),
                "{value}"
            );
        }
        assert_eq!(
            CapabilityToken::validate_roots(&roots(&["C:/Games", "C:/Games/Example"])),
            Err(CapabilityTokenError::OverlappingRoots)
        );
    }
}
