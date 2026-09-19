//! Immutable installation context and collision-free installation identifier.

use std::io;
use std::path::{Path, PathBuf};

/// Collision-free installation identifier wrapping canonical installation root path.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InstallationId(PathBuf);

impl InstallationId {
    #[must_use]
    pub fn from_canonical_path(path: &Path) -> Self {
        Self(path.to_path_buf())
    }
}

/// Immutable game installation context. Owns the canonical root directory and ID.
#[derive(Debug)]
pub struct GameInstallationContext {
    id: InstallationId,
    root_path: PathBuf,
}

impl GameInstallationContext {
    pub fn new(root_path: impl Into<PathBuf>) -> io::Result<Self> {
        let root_path = root_path.into();
        let canonical_root = std::fs::canonicalize(&root_path)?;
        let id = InstallationId::from_canonical_path(&canonical_root);
        Ok(Self {
            id,
            root_path: canonical_root,
        })
    }

    #[must_use]
    pub fn id(&self) -> &InstallationId {
        &self.id
    }

    #[must_use]
    pub fn root_path(&self) -> &Path {
        &self.root_path
    }

    #[cfg(test)]
    pub(crate) fn synthetic(root_path: PathBuf) -> Self {
        let id = InstallationId::from_canonical_path(&root_path);
        Self { id, root_path }
    }
}
