use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AuthorityMode {
    CooperativeSameUid,
    HostileSameUid,
}

impl AuthorityMode {
    pub(crate) fn preflight(self) -> Result<(), ServiceError> {
        match self {
            Self::CooperativeSameUid => Ok(()),
            Self::HostileSameUid => Err(crate::failed(
                "hostile same-principal filesystem authority is unsupported; use a separate native helper or fail closed",
            )),
        }
    }
}

/// One normal directory-entry component. It can never contain a separator,
/// `.`/`..`, a prefix, or NUL.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LeafName(OsString);

impl LeafName {
    #[cfg(test)]
    pub(crate) fn from_capability(
        prefix: &str,
        capability: &[u8; 32],
    ) -> Result<Self, ServiceError> {
        if prefix.is_empty() || capability.iter().all(|byte| *byte == 0) {
            return Err(crate::failed("invalid private namespace capability prefix"));
        }
        let encoded = format!("{prefix}-{}", hex::encode(capability));
        Self::parse(OsStr::new(&encoded))
    }

    pub(crate) fn parse(name: &OsStr) -> Result<Self, ServiceError> {
        let textual = name.to_string_lossy();
        if name.is_empty()
            || textual
                .chars()
                .any(|character| matches!(character, '\0' | '/' | '\\' | ':'))
        {
            return Err(crate::failed(
                "filesystem leaf is empty or contains a separator, prefix, or NUL",
            ));
        }
        let mut components = Path::new(name).components();
        match components.next() {
            Some(Component::Normal(component))
                if component == name && components.next().is_none() =>
            {
                Ok(Self(name.to_owned()))
            }
            _ => Err(crate::failed(
                "filesystem leaf must be exactly one normal component",
            )),
        }
    }

    pub(crate) fn from_path(path: &Path) -> Result<Self, ServiceError> {
        let leaf = path
            .file_name()
            .ok_or_else(|| crate::failed(format!("path `{}` has no leaf", path.display())))?;
        Self::parse(leaf)
    }

    pub(crate) fn as_os_str(&self) -> &OsStr {
        &self.0
    }

    pub(in crate::fs::authority) fn is_bound_to_capability(&self, capability: &[u8; 32]) -> bool {
        let encoded = hex::encode(capability);
        self.0.to_str().is_some_and(|name| {
            name.strip_suffix(encoded.as_str())
                .is_some_and(|prefix| !prefix.is_empty() && prefix.ends_with('-'))
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum EntryKind {
    File,
    Directory,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RenameNoReplace {
    Moved,
    Occupied,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RemoveEmptyDir {
    Removed,
    Absent,
    NotEmpty,
    IdentityChanged,
}

/// Result of a native create-without-replacement operation.
///
/// `Created` retains the exact handle used to write, sync, and observe the
/// file. `Occupied` is a normal outcome and never implies that the existing
/// entry was inspected or modified.
#[derive(Debug)]
pub(crate) enum CreateFileNoReplace {
    Created {
        // Kept alive through result inspection so the exact native handle
        // remains owned until the caller has classified the create outcome.
        _entry: VerifiedEntry,
        observation: EntryObservation,
    },
    Occupied,
}

/// One direct child observed through a retained namespace directory handle.
#[derive(Clone, Debug)]
pub(crate) struct NamespaceChild {
    pub(crate) name: LeafName,
    pub(crate) observation: EntryObservation,
}

/// Identity and content captured from the same retained handle.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct EntryObservation {
    pub(crate) kind: EntryKind,
    pub(crate) identity: String,
    pub(crate) digest: Option<String>,
}

impl EntryObservation {
    pub(crate) fn is_directory(&self) -> bool {
        self.kind == EntryKind::Directory
    }
}

/// A retained, component-walked directory authority.
pub(crate) struct VerifiedDir {
    pub(in crate::fs::authority) metadata_path: PathBuf,
    pub(in crate::fs::authority) identity: String,
    #[cfg(target_os = "linux")]
    pub(in crate::fs::authority) fd: rustix::fd::OwnedFd,
    #[cfg(windows)]
    pub(in crate::fs::authority) handle: File,
}

impl std::fmt::Debug for VerifiedDir {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("VerifiedDir")
            .field("metadata_path", &self.metadata_path)
            .field("identity", &self.identity)
            .finish_non_exhaustive()
    }
}
