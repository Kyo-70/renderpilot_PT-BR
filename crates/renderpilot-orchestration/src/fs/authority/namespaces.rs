use super::*;

/// Private namespace capability with retained parent and child authorities.
pub(crate) struct PrivateNamespace {
    pub(in crate::fs::authority) metadata_path: PathBuf,
    pub(in crate::fs::authority) identity: String,
    pub(in crate::fs::authority) parent: VerifiedDir,
    // Windows removes this directory through its retained directory handle.
    // Unix needs the leaf for the parent-relative unlinkat ceremony.
    #[cfg(not(windows))]
    pub(in crate::fs::authority) leaf: LeafName,
    pub(in crate::fs::authority) dir: VerifiedDir,
}

impl std::fmt::Debug for PrivateNamespace {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PrivateNamespace")
            .field("metadata_path", &self.metadata_path)
            .field("identity", &self.identity)
            .finish_non_exhaustive()
    }
}

impl PrivateNamespace {
    #[cfg(test)]
    pub(crate) fn metadata_path(&self) -> &Path {
        &self.metadata_path
    }

    pub(crate) fn identity(&self) -> &str {
        &self.identity
    }

    pub(crate) fn directory(&self) -> &VerifiedDir {
        &self.dir
    }

    pub(crate) fn reopen(
        parent: &VerifiedDir,
        name: &LeafName,
        expected_identity: &str,
    ) -> Result<Self, ServiceError> {
        #[cfg(windows)]
        let entry = parent.open_directory_leaf(name)?;
        #[cfg(not(windows))]
        let entry = parent.open_leaf(name)?;
        let observed = entry.observe()?;
        if observed.kind != EntryKind::Directory || observed.identity != expected_identity {
            return Err(crate::failed("private namespace identity or kind changed"));
        }
        #[cfg(target_os = "linux")]
        {
            let dir = entry.into_directory(expected_identity)?;
            let stat = rustix::fs::fstat(dir.as_fd()).map_err(|error| {
                crate::failed(format!(
                    "failed to inspect reopened private namespace: {error}"
                ))
            })?;
            if stat.st_mode & 0o777 != 0o700 || stat.st_uid != linux_effective_uid() {
                return Err(crate::failed("reopened private namespace security changed"));
            }
            Ok(Self {
                metadata_path: dir.metadata_path.clone(),
                identity: expected_identity.to_owned(),
                parent: parent.clone_capability()?,
                #[cfg(not(windows))]
                leaf: name.clone(),
                dir,
            })
        }
        #[cfg(windows)]
        {
            let dir = entry.into_directory(expected_identity)?;
            windows_verify_private_security(&dir.handle)?;
            Ok(Self {
                metadata_path: dir.metadata_path.clone(),
                identity: expected_identity.to_owned(),
                parent: parent.clone_capability()?,
                dir,
            })
        }
        #[cfg(all(
            not(any(target_os = "linux", windows)),
            feature = "development-host-fallback"
        ))]
        {
            let _ = (parent, name, expected_identity);
            Err(crate::failed("namespace reopen fallback is unsupported"))
        }
        #[cfg(all(
            not(any(target_os = "linux", windows)),
            not(feature = "development-host-fallback")
        ))]
        {
            Err(crate::failed(
                "native namespace reopen is unsupported on this host",
            ))
        }
    }

    /// Reopens a deterministic private namespace after observing its exact
    /// identity through the retained parent authority. This is used only for
    /// a journaled create-intent recovery edge where the identity CAS may not
    /// have happened yet.
    pub(crate) fn reopen_observed(
        parent: &VerifiedDir,
        name: &LeafName,
    ) -> Result<Self, ServiceError> {
        let observed = parent
            .observe_leaf(name)?
            .ok_or_else(|| crate::failed("private namespace is absent"))?;
        if observed.kind != EntryKind::Directory {
            return Err(crate::failed(
                "private namespace recovery object is not a directory",
            ));
        }
        Self::reopen(parent, name, &observed.identity)
    }

    #[cfg(test)]
    pub(crate) fn enumerate_reserved_children(
        &self,
        reserved: &[LeafName],
    ) -> Result<Vec<NamespaceChild>, ServiceError> {
        self.dir.enumerate_reserved_children(reserved)
    }

    pub(crate) fn remove_empty_typed(
        self,
        mode: AuthorityMode,
    ) -> Result<RemoveEmptyDir, ServiceError> {
        mode.preflight()?;
        #[cfg(windows)]
        {
            if windows_identity(&self.dir.handle)? != self.identity {
                return Ok(RemoveEmptyDir::IdentityChanged);
            }
            match windows_dispose_by_handle(&self.dir.handle) {
                Ok(()) => {
                    self.parent.sync()?;
                    Ok(RemoveEmptyDir::Removed)
                }
                Err(error)
                    if error
                        .raw_os_error()
                        .and_then(|code| u32::try_from(code).ok())
                        == Some(windows_sys::Win32::Foundation::ERROR_DIR_NOT_EMPTY) =>
                {
                    Ok(RemoveEmptyDir::NotEmpty)
                }
                Err(error)
                    if matches!(
                        error
                            .raw_os_error()
                            .and_then(|code| u32::try_from(code).ok()),
                        Some(
                            windows_sys::Win32::Foundation::ERROR_FILE_NOT_FOUND
                                | windows_sys::Win32::Foundation::ERROR_PATH_NOT_FOUND,
                        )
                    ) =>
                {
                    Ok(RemoveEmptyDir::Absent)
                }
                Err(error) => Err(crate::failed(format!(
                    "failed to remove exact private namespace: {error}"
                ))),
            }
        }
        #[cfg(not(windows))]
        {
            self.parent
                .remove_empty_dir(&self.leaf, &self.identity, mode)
        }
    }

    pub(crate) fn remove_empty(self, mode: AuthorityMode) -> Result<(), ServiceError> {
        match self.remove_empty_typed(mode)? {
            RemoveEmptyDir::Removed => Ok(()),
            RemoveEmptyDir::Absent => Err(crate::failed("private namespace is absent")),
            RemoveEmptyDir::NotEmpty => Err(crate::failed("private namespace is not empty")),
            RemoveEmptyDir::IdentityChanged => {
                Err(crate::failed("private namespace identity changed"))
            }
        }
    }
}

/// Control namespace whose leaf is deterministically bound to both the
/// persisted transaction id and the caller-supplied opaque 256-bit
/// capability. This separate type prevents a control token from weakening
/// participant namespace validation.
pub(crate) struct ControlNamespace {
    namespace: PrivateNamespace,
    transaction_id: String,
    #[cfg(test)]
    capability: [u8; 32],
}

impl std::fmt::Debug for ControlNamespace {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ControlNamespace")
            .field("metadata_path", &self.namespace.metadata_path)
            .field("identity", &self.namespace.identity)
            .field("transaction_id", &self.transaction_id)
            .finish_non_exhaustive()
    }
}

impl ControlNamespace {
    pub(crate) fn create(
        parent: &VerifiedDir,
        transaction_id: &str,
        capability: &[u8; 32],
        mode: AuthorityMode,
    ) -> Result<Self, ServiceError> {
        let leaf = control_leaf(transaction_id, capability)?;
        let namespace = parent.create_private_namespace_with_mode(&leaf, capability, mode)?;
        Ok(Self {
            namespace,
            transaction_id: transaction_id.to_owned(),
            #[cfg(test)]
            capability: *capability,
        })
    }

    pub(crate) fn reopen(
        parent: &VerifiedDir,
        transaction_id: &str,
        capability: &[u8; 32],
        expected_identity: &str,
    ) -> Result<Self, ServiceError> {
        let leaf = control_leaf(transaction_id, capability)?;
        let namespace = PrivateNamespace::reopen(parent, &leaf, expected_identity)?;
        Ok(Self {
            namespace,
            transaction_id: transaction_id.to_owned(),
            #[cfg(test)]
            capability: *capability,
        })
    }

    /// Reopens the capability-derived control namespace after observing its
    /// exact identity. The leaf is still reconstructed from the durable
    /// transaction id and capability before any native object is accepted.
    pub(crate) fn reopen_observed(
        parent: &VerifiedDir,
        transaction_id: &str,
        capability: &[u8; 32],
    ) -> Result<Self, ServiceError> {
        let leaf = control_leaf(transaction_id, capability)?;
        let namespace = PrivateNamespace::reopen_observed(parent, &leaf)?;
        Ok(Self {
            namespace,
            transaction_id: transaction_id.to_owned(),
            #[cfg(test)]
            capability: *capability,
        })
    }

    #[cfg(test)]
    pub(crate) fn metadata_path(&self) -> &Path {
        self.namespace.metadata_path()
    }

    pub(crate) fn identity(&self) -> &str {
        self.namespace.identity()
    }

    #[cfg(test)]
    pub(crate) fn transaction_id(&self) -> &str {
        &self.transaction_id
    }

    #[cfg(test)]
    pub(crate) fn capability(&self) -> &[u8; 32] {
        &self.capability
    }

    #[cfg(test)]
    pub(crate) fn enumerate_reserved_children(
        &self,
        reserved: &[LeafName],
    ) -> Result<Vec<NamespaceChild>, ServiceError> {
        self.namespace.enumerate_reserved_children(reserved)
    }

    pub(crate) fn remove_empty(self, mode: AuthorityMode) -> Result<(), ServiceError> {
        self.namespace.remove_empty(mode)
    }
}

fn control_leaf(transaction_id: &str, capability: &[u8; 32]) -> Result<LeafName, ServiceError> {
    if capability.iter().all(|byte| *byte == 0) {
        return Err(crate::failed(
            "control namespace capability must be nonzero",
        ));
    }
    let transaction_leaf = LeafName::parse(OsStr::new(transaction_id))?;
    let encoded = format!(
        "control-{}-{}",
        transaction_leaf.as_os_str().to_string_lossy(),
        hex::encode(capability)
    );
    let leaf = LeafName::parse(OsStr::new(&encoded))?;
    if leaf.as_os_str().to_string_lossy() != encoded {
        return Err(crate::failed(
            "control namespace leaf encoding is not canonical",
        ));
    }
    if !leaf.is_bound_to_capability(capability) {
        return Err(crate::failed(
            "control namespace leaf is not bound to its capability",
        ));
    }
    Ok(leaf)
}
