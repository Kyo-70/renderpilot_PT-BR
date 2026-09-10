use std::path::PathBuf;

use renderpilot_domain::PathRef;

use crate::ServiceError;
use crate::file_mutation::MutationScope;

/// Explicit filesystem authority for an active peer mutation.
///
/// The game root is always first.  The optional payload root is second and is
/// retained only when it is a distinct canonical directory.  This type is
/// deliberately closed so an adapter cannot smuggle in a parent-derived or
/// record-derived root after planning.
#[derive(Debug, Clone)]
pub(crate) struct PeerRoots {
    scope: MutationScope,
}

impl PeerRoots {
    pub(crate) fn new(
        game_root: impl Into<PathBuf>,
        payload_root: Option<PathBuf>,
    ) -> Result<Self, ServiceError> {
        let game_root = game_root.into();
        let game_root = crate::paths::canonical_candidate(&game_root)
            .map_err(|error| crate::failed(format!("peer game root is not reachable: {error}")))?;
        let payload_root = payload_root
            .map(|root| {
                crate::paths::canonical_candidate(&root).map_err(|error| {
                    crate::failed(format!("peer payload root is not reachable: {error}"))
                })
            })
            .transpose()?;
        if payload_root.as_ref().is_some_and(|payload| {
            crate::paths::normalized_key(payload) == crate::paths::normalized_key(&game_root)
        }) {
            return Err(crate::failed(
                "peer payload root must be distinct from the explicit game root",
            ));
        }
        let mut roots = vec![game_root];
        if let Some(payload_root) = payload_root {
            roots.push(payload_root);
        }
        let scope = MutationScope::new(roots)?;
        for root in scope.roots() {
            crate::fs::VerifiedDir::open_absolute_components(root, None).map_err(|error| {
                crate::failed(format!(
                    "peer mutation root is not a reachable directory {}: {error}",
                    root.display()
                ))
            })?;
        }
        Ok(Self { scope })
    }

    pub(crate) fn scope(&self) -> &MutationScope {
        &self.scope
    }

    pub(crate) fn roots(&self) -> &[PathBuf] {
        self.scope.roots()
    }

    pub(crate) fn require_game_path(&self, path: &PathRef) -> Result<(), ServiceError> {
        let game_root = self
            .scope
            .roots()
            .first()
            .ok_or_else(|| crate::failed("peer mutation has no game root"))?;
        if is_strict_descendant(path, game_root) {
            Ok(())
        } else {
            Err(crate::failed(format!(
                "active topology path is not a strict descendant of the explicit game root: {}",
                path.as_str()
            )))
        }
    }

    pub(crate) fn require_sealed_path(&self, path: &PathRef) -> Result<(), ServiceError> {
        if self
            .scope
            .roots()
            .iter()
            .any(|root| is_strict_descendant(path, root))
        {
            Ok(())
        } else {
            Err(crate::failed(format!(
                "active sealed path is not a strict descendant of an explicit sealed root: {}",
                path.as_str()
            )))
        }
    }
}

fn is_strict_descendant(path: &PathRef, root: &std::path::Path) -> bool {
    let Some(path) = strict_absolute_guard_path(path.as_str()) else {
        return false;
    };
    let Ok(candidate) = crate::paths::canonical_candidate(std::path::Path::new(&path)) else {
        return false;
    };
    crate::paths::is_within(&candidate, root) && !crate::paths::same_path(&candidate, root)
}

/// Mirrors the storage boundary's lexical path contract before filesystem
/// canonicalization. This keeps a path such as `game/sub/../file.dll` from
/// becoming apparently safe only because the filesystem resolves it inward.
fn strict_absolute_guard_path(value: &str) -> Option<String> {
    if value.is_empty() || value.trim() != value || value.contains('\0') {
        return None;
    }
    let mut normalized = value.replace('\\', "/");
    if normalized.starts_with("//./") {
        return None;
    }
    if let Some(verbatim) = normalized.strip_prefix("//?/") {
        if let Some(unc) = verbatim.strip_prefix("UNC/") {
            normalized = format!("//{unc}");
        } else if verbatim.len() >= 3
            && verbatim.as_bytes()[1] == b':'
            && verbatim.as_bytes()[2] == b'/'
        {
            normalized = verbatim.to_owned();
        } else {
            return None;
        }
    }

    let is_drive = normalized.len() >= 2 && normalized.as_bytes()[1] == b':';
    if is_drive {
        let bytes = normalized.as_bytes();
        if bytes.len() < 3
            || !bytes[0].is_ascii_alphabetic()
            || bytes[2] != b'/'
            || normalized[3..].contains(':')
        {
            return None;
        }
        if !valid_components(&normalized[3..]) {
            return None;
        }
    } else if let Some(unc) = normalized.strip_prefix("//") {
        let mut count = 0;
        let all_valid = unc.split('/').all(|component| {
            count += 1;
            is_valid_component(component)
        });
        if count < 2 || !all_valid {
            return None;
        }
    } else {
        let absolute = normalized.strip_prefix('/')?;
        if !valid_components(absolute) {
            return None;
        }
    }

    let is_root = normalized == "/"
        || (normalized.len() == 3 && normalized.as_bytes()[1] == b':' && normalized.ends_with('/'));
    if !is_root && normalized.ends_with('/') {
        return None;
    }
    Some(normalized)
}

fn valid_components(value: &str) -> bool {
    if value.is_empty() {
        return true;
    }
    value.split('/').all(is_valid_component)
}

#[inline]
fn is_valid_component(component: &str) -> bool {
    !component.is_empty() && component != "." && component != ".." && !component.contains(':')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_first_seen_order_and_rejects_explicit_equal_roots() {
        let game = tempfile::tempdir().expect("game");
        let payload = tempfile::tempdir().expect("payload");
        let roots = PeerRoots::new(
            game.path().to_path_buf(),
            Some(payload.path().to_path_buf()),
        )
        .expect("roots");
        assert_eq!(
            roots.roots()[0],
            crate::paths::canonicalize_existing(game.path()).expect("game canonical")
        );
        assert_eq!(
            roots.roots()[1],
            crate::paths::canonicalize_existing(payload.path()).expect("payload canonical")
        );

        let duplicate = PeerRoots::new(game.path().to_path_buf(), Some(game.path().join(".")));
        assert!(duplicate.is_err(), "equal explicit roots must fail closed");
    }

    #[test]
    fn rejects_unreachable_and_file_authority_roots_before_planning() {
        let game = tempfile::tempdir().expect("game");
        let missing = game.path().join("does-not-exist");
        assert!(PeerRoots::new(missing, None).is_err());

        let file = game.path().join("file");
        std::fs::write(&file, b"x").expect("file");
        assert!(PeerRoots::new(file, None).is_err());
    }

    #[test]
    fn requires_strict_descendants_and_rejects_lexical_escape_or_component_prefix() {
        let game = tempfile::tempdir().expect("game");
        let payload = tempfile::tempdir().expect("payload");
        let roots = PeerRoots::new(
            game.path().to_path_buf(),
            Some(payload.path().to_path_buf()),
        )
        .expect("roots");

        let game_child = PathRef::new(game.path().join("child.dll").to_string_lossy().into_owned())
            .expect("path");
        roots.require_game_path(&game_child).expect("game child");
        let game_equal = PathRef::new(game.path().to_string_lossy().into_owned()).expect("path");
        assert!(roots.require_game_path(&game_equal).is_err());

        let payload_child = PathRef::new(
            payload
                .path()
                .join("child.dll")
                .to_string_lossy()
                .into_owned(),
        )
        .expect("path");
        roots
            .require_sealed_path(&payload_child)
            .expect("payload child");
        let payload_equal =
            PathRef::new(payload.path().to_string_lossy().into_owned()).expect("path");
        assert!(roots.require_sealed_path(&payload_equal).is_err());

        let siblings = tempfile::tempdir().expect("sibling roots");
        let payload_prefix = siblings.path().join("payload");
        let payload2_prefix = siblings.path().join("payload2");
        std::fs::create_dir_all(&payload_prefix).expect("payload prefix");
        std::fs::create_dir_all(&payload2_prefix).expect("payload2 prefix");
        let prefix_roots =
            PeerRoots::new(game.path().to_path_buf(), Some(payload_prefix)).expect("prefix roots");
        let payload2_child = PathRef::new(
            payload2_prefix
                .join("child.dll")
                .to_string_lossy()
                .into_owned(),
        )
        .expect("path");
        assert!(prefix_roots.require_sealed_path(&payload2_child).is_err());

        let game_only = PeerRoots::new(game.path().to_path_buf(), None).expect("game-only roots");
        assert!(game_only.require_sealed_path(&payload_child).is_err());

        let lexical_escape = PathRef::new(
            game.path()
                .join("..")
                .join("outside.dll")
                .to_string_lossy()
                .into_owned(),
        )
        .expect("path");
        assert!(roots.require_game_path(&lexical_escape).is_err());

        let lexical_inside = PathRef::new(format!(
            "{}/sub/../inside.dll",
            game.path().to_string_lossy()
        ))
        .expect("path");
        assert!(roots.require_game_path(&lexical_inside).is_err());

        let duplicate_separator =
            PathRef::new(format!("{}/sub//inside.dll", game.path().to_string_lossy()))
                .expect("path");
        assert!(roots.require_game_path(&duplicate_separator).is_err());
    }
}
