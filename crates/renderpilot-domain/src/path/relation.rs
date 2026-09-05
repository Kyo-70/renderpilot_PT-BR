use super::normalized_path_key;

/// Lexical relationship of two paths after target-platform normalization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NormalizedPathRelation {
    /// Both spellings identify the same lexical path.
    Equal,
    /// `left` is a strict lexical ancestor of `right`.
    LeftAncestor,
    /// `right` is a strict lexical ancestor of `left`.
    RightAncestor,
    /// The paths are unrelated lexically.
    Disjoint,
}

impl NormalizedPathRelation {
    /// Returns whether the paths are equal or one is an ancestor of the other.
    #[must_use]
    pub const fn overlaps(self) -> bool {
        !matches!(self, Self::Disjoint)
    }
}

/// Classifies two path strings using the domain's existing lexical key.
///
/// Anchor classes (drive roots, UNC shares, Unix roots, and relative paths)
/// must match before ancestry is reported.  Verbatim DOS/UNC aliases are
/// handled by `normalized_path_key`; no additional normalization is applied.
#[must_use]
pub fn normalized_path_relation(left: &str, right: &str) -> NormalizedPathRelation {
    let left = normalized_path_key(left);
    let right = normalized_path_key(right);

    if left == right {
        return NormalizedPathRelation::Equal;
    }
    if !same_anchor(&left, &right) {
        return NormalizedPathRelation::Disjoint;
    }
    if is_component_prefix(&left, &right) {
        NormalizedPathRelation::LeftAncestor
    } else if is_component_prefix(&right, &left) {
        NormalizedPathRelation::RightAncestor
    } else {
        NormalizedPathRelation::Disjoint
    }
}

fn same_anchor(left: &str, right: &str) -> bool {
    match (anchor(left), anchor(right)) {
        (Anchor::Drive(left), Anchor::Drive(right)) => left == right,
        (Anchor::Unc(left), Anchor::Unc(right)) => left == right,
        (Anchor::Unix, Anchor::Unix) | (Anchor::Relative, Anchor::Relative) => true,
        _ => false,
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Anchor<'a> {
    Drive(u8),
    Unc(&'a str),
    Unix,
    Relative,
}

fn anchor(path: &str) -> Anchor<'_> {
    let bytes = path.as_bytes();
    if bytes.len() >= 3 && bytes[1] == b':' && bytes[2] == b'/' {
        return Anchor::Drive(bytes[0]);
    }
    if let Some(rest) = path.strip_prefix("//") {
        let mut components = rest.split('/');
        let server = components.next().unwrap_or_default();
        let share = components.next().unwrap_or_default();
        let end = if share.is_empty() {
            server.len()
        } else {
            server.len() + 1 + share.len()
        };
        return Anchor::Unc(&rest[..end]);
    }
    if path.starts_with('/') {
        Anchor::Unix
    } else {
        Anchor::Relative
    }
}

fn is_component_prefix(prefix: &str, path: &str) -> bool {
    if prefix.ends_with('/') {
        return path.starts_with(prefix);
    }
    path.strip_prefix(prefix)
        .is_some_and(|suffix| suffix.starts_with('/'))
}

#[cfg(test)]
mod tests {
    use super::{NormalizedPathRelation as R, normalized_path_relation as rel};

    #[test]
    fn classifies_equal_case_and_separator_aliases() {
        assert_eq!(rel(r"C:\Games\Bin", "c:/games/bin"), R::Equal);
        assert_eq!(rel(r"\\?\C:\Games\Bin", "c:/games/bin"), R::Equal);
        assert_eq!(rel(r"\\?\UNC\srv\share\Game", "//srv/share/game"), R::Equal);
    }

    #[test]
    fn classifies_ancestors_with_root_boundaries() {
        assert_eq!(rel("/", "/games/bin/dxgi.dll"), R::LeftAncestor);
        assert_eq!(rel("c:/", "C:/Games/Bin"), R::LeftAncestor);
        assert_eq!(rel("/games", "/games/bin/dxgi.dll"), R::LeftAncestor);
        assert_eq!(rel("c:/games", "c:/games/bin"), R::LeftAncestor);
        assert_eq!(rel("//srv/share", "//srv/share/bin"), R::LeftAncestor);
        assert_eq!(rel("games/bin", "games"), R::RightAncestor);
    }

    #[test]
    fn rejects_siblings_prefix_collisions_and_cross_anchors() {
        assert_eq!(rel("/games", "/games2"), R::Disjoint);
        assert_eq!(rel("c:/games", "d:/games/bin"), R::Disjoint);
        assert_eq!(rel("//srv/share", "//other/share/bin"), R::Disjoint);
        assert_eq!(rel("//srv/share", "//srv/shareware/bin"), R::Disjoint);
        assert_eq!(rel("/games", "c:/games/bin"), R::Disjoint);
    }

    #[test]
    fn preserves_dot_and_non_ascii_segments() {
        assert_eq!(rel("/Игры", "/игры/bin"), R::Disjoint);
        assert_eq!(
            rel("/games/../other", "/games/../other/bin"),
            R::LeftAncestor
        );
        assert_eq!(rel("/games/../other", "/games/other/bin"), R::Disjoint);
    }
}
