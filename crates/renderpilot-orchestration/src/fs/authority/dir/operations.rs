use super::super::*;

impl VerifiedDir {
    /// Create one direct child directory without replacement and retain the
    /// handle opened for that exact child.  The operation deliberately has no
    /// `Occupied` success variant: an occupied leaf is a failed acquisition,
    /// and the existing entry is never opened, removed, or otherwise touched.
    ///
    /// Creation is fenced by [`AuthorityMode::preflight`] before the first
    /// namespace syscall.  On native hosts, the returned identity is taken
    /// from the retained child handle; its metadata path is diagnostic only.
    pub(crate) fn create_directory_no_replace(
        &self,
        name: &LeafName,
        mode: AuthorityMode,
    ) -> Result<Self, ServiceError> {
        mode.preflight()?;
        #[cfg(target_os = "linux")]
        {
            rustix::fs::mkdirat(
                &self.fd,
                name.as_os_str(),
                rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR | rustix::fs::Mode::XUSR,
            )
            .map_err(|error| {
                crate::failed(format!(
                    "failed to create relative directory without replacement: {error}"
                ))
            })?;

            // Do not attempt path-based cleanup if retaining or validating the
            // just-created child fails.  Leaving the artifact for the durable
            // cleanup fence is safer than risking deletion of a raced foreign
            // entry under the same leaf.
            let fd = rustix::fs::openat(
                &self.fd,
                name.as_os_str(),
                rustix::fs::OFlags::RDONLY
                    | rustix::fs::OFlags::DIRECTORY
                    | rustix::fs::OFlags::CLOEXEC
                    | rustix::fs::OFlags::NOFOLLOW,
                rustix::fs::Mode::empty(),
            )
            .map_err(|error| {
                crate::failed(format!(
                    "failed to retain relative directory without replacement: {error}"
                ))
            })?;
            let metadata = rustix::fs::fstat(&fd).map_err(|error| {
                crate::failed(format!(
                    "failed to inspect retained relative directory: {error}"
                ))
            })?;
            let mode_bits = metadata.st_mode & 0o777;
            if mode_bits != 0o700 {
                return Err(crate::failed(format!(
                    "created relative directory is not owner-only: {mode_bits:o}"
                )));
            }
            if metadata.st_uid != linux_effective_uid() {
                return Err(crate::failed(
                    "created relative directory is not owned by the current effective UID",
                ));
            }
            if !linux_identity_is_stable(metadata.st_dev, metadata.st_ino) {
                return Err(crate::failed(
                    "created relative directory has an unstable filesystem identity",
                ));
            }
            let directory = Self {
                metadata_path: self.metadata_path.join(name.as_os_str()),
                identity: linux_identity(metadata.st_dev, metadata.st_ino),
                fd,
            };
            self.sync()?;
            Ok(directory)
        }
        #[cfg(windows)]
        {
            // FILE_CREATE plus a RootDirectory handle gives relative,
            // no-replace creation.  The helper supplies the protected
            // owner-only DACL in the same NtCreateFile request.
            let (handle, identity) = windows_create_owner_only_directory(&self.handle, name)?;
            let directory = Self {
                metadata_path: self.metadata_path.join(name.as_os_str()),
                identity,
                handle,
            };
            if let Err(error) = windows_verify_private_security(&directory.handle) {
                let cleanup = windows_dispose_by_handle(&directory.handle);
                return Err(crate::failed(match cleanup {
                    Ok(()) => error.to_string(),
                    Err(cleanup_error) => {
                        format!("{error}; created directory cleanup also failed: {cleanup_error}")
                    }
                }));
            }
            self.sync()?;
            Ok(directory)
        }
        #[cfg(all(
            not(any(target_os = "linux", windows)),
            feature = "development-host-fallback"
        ))]
        {
            let _ = (self, name);
            Err(crate::failed(
                "native relative directory creation is unsupported on this host",
            ))
        }
        #[cfg(all(
            not(any(target_os = "linux", windows)),
            not(feature = "development-host-fallback")
        ))]
        {
            let _ = (self, name);
            Err(crate::failed(
                "native relative directory creation is unsupported on this host",
            ))
        }
    }

    /// Enumerate direct children through this retained directory handle. Every
    /// child must be a listed reserved leaf; unknown names, malformed names,
    /// and reparse/link entries fail closed.
    pub(crate) fn enumerate_reserved_children(
        &self,
        reserved: &[LeafName],
    ) -> Result<Vec<NamespaceChild>, ServiceError> {
        #[cfg(target_os = "linux")]
        {
            use std::os::unix::ffi::OsStrExt;
            let mut directory = rustix::fs::Dir::read_from(&self.fd).map_err(|error| {
                crate::failed(format!("failed to enumerate retained directory: {error}"))
            })?;
            let mut names = Vec::new();
            for result in &mut directory {
                let entry = result.map_err(|error| {
                    crate::failed(format!("failed to enumerate retained directory: {error}"))
                })?;
                let bytes = entry.file_name().to_bytes();
                if bytes == b"." || bytes == b".." {
                    continue;
                }
                let name = LeafName::parse(OsStr::from_bytes(bytes))?;
                if !reserved.iter().any(|reserved| reserved == &name) {
                    return Err(crate::failed(
                        "private namespace contains an unknown direct child",
                    ));
                }
                names.push(name);
            }
            let mut children = Vec::with_capacity(names.len());
            for name in names {
                let entry = self.open_leaf(&name)?;
                let observation = entry.observe()?;
                children.push(NamespaceChild { name, observation });
            }
            Ok(children)
        }
        #[cfg(windows)]
        {
            let names = windows_enumerate_directory_names(&self.handle)?;
            let mut children = Vec::with_capacity(names.len());
            for os_name in names {
                let name = LeafName::parse(&os_name)?;
                if !reserved.iter().any(|reserved| reserved == &name) {
                    return Err(crate::failed(
                        "private namespace contains an unknown direct child",
                    ));
                }
                let entry = self.open_leaf(&name)?;
                let observation = entry.observe()?;
                children.push(NamespaceChild { name, observation });
            }
            Ok(children)
        }
        #[cfg(all(
            not(any(target_os = "linux", windows)),
            feature = "development-host-fallback"
        ))]
        {
            let mut children = Vec::new();
            for result in std::fs::read_dir(&self.metadata_path)
                .map_err(|error| crate::failed(error.to_string()))?
            {
                let entry = result.map_err(|error| crate::failed(error.to_string()))?;
                let name = LeafName::parse(&entry.file_name())?;
                if !reserved.iter().any(|reserved| reserved == &name) {
                    return Err(crate::failed(
                        "private namespace contains an unknown direct child",
                    ));
                }
                let verified = self.open_leaf(&name)?;
                children.push(NamespaceChild {
                    name,
                    observation: verified.observe()?,
                });
            }
            Ok(children)
        }
        #[cfg(all(
            not(any(target_os = "linux", windows)),
            not(feature = "development-host-fallback")
        ))]
        {
            let _ = reserved;
            Err(crate::failed(
                "native directory enumeration is unsupported on this host",
            ))
        }
    }

    /// Create a private namespace. The caller supplies the complete 256-bit
    /// capability used to derive the unpredictable leaf.
    pub(crate) fn create_private_namespace(
        &self,
        name: &LeafName,
        capability: &[u8; 32],
    ) -> Result<PrivateNamespace, ServiceError> {
        self.create_private_namespace_with_mode(name, capability, AuthorityMode::CooperativeSameUid)
    }

    pub(crate) fn create_private_namespace_with_mode(
        &self,
        name: &LeafName,
        capability: &[u8; 32],
        mode: AuthorityMode,
    ) -> Result<PrivateNamespace, ServiceError> {
        mode.preflight()?;
        if capability.iter().all(|byte| *byte == 0) {
            return Err(crate::failed(
                "private namespace capability must be nonzero",
            ));
        }
        if !name.is_bound_to_capability(capability) {
            return Err(crate::failed(
                "private namespace leaf is not cryptographically bound to its capability",
            ));
        }
        let parent = self.clone_capability()?;
        #[cfg(target_os = "linux")]
        {
            rustix::fs::mkdirat(
                &self.fd,
                name.as_os_str(),
                rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR | rustix::fs::Mode::XUSR,
            )
            .map_err(|error| {
                crate::failed(format!("failed to create private namespace: {error}"))
            })?;
            let fd = match rustix::fs::openat(
                &self.fd,
                name.as_os_str(),
                rustix::fs::OFlags::RDONLY
                    | rustix::fs::OFlags::DIRECTORY
                    | rustix::fs::OFlags::CLOEXEC
                    | rustix::fs::OFlags::NOFOLLOW,
                rustix::fs::Mode::empty(),
            ) {
                Ok(fd) => fd,
                Err(error) => {
                    let cleanup = rustix::fs::unlinkat(
                        &self.fd,
                        name.as_os_str(),
                        rustix::fs::AtFlags::REMOVEDIR,
                    );
                    return Err(crate::failed(match cleanup {
                        Ok(()) => format!("failed to retain private namespace: {error}"),
                        Err(cleanup_error) => format!(
                            "failed to retain private namespace: {error}; cleanup also failed: {cleanup_error}"
                        ),
                    }));
                }
            };
            let metadata = match rustix::fs::fstat(&fd) {
                Ok(metadata) => metadata,
                Err(error) => {
                    drop(fd);
                    let cleanup = rustix::fs::unlinkat(
                        &self.fd,
                        name.as_os_str(),
                        rustix::fs::AtFlags::REMOVEDIR,
                    );
                    return Err(crate::failed(match cleanup {
                        Ok(()) => format!("failed to inspect private namespace: {error}"),
                        Err(cleanup_error) => format!(
                            "failed to inspect private namespace: {error}; cleanup also failed: {cleanup_error}"
                        ),
                    }));
                }
            };
            let mode_bits = metadata.st_mode & 0o777;
            if mode_bits != 0o700 {
                drop(fd);
                let cleanup = rustix::fs::unlinkat(
                    &self.fd,
                    name.as_os_str(),
                    rustix::fs::AtFlags::REMOVEDIR,
                );
                return Err(crate::failed(format!(
                    "private namespace mode is not 0700: {mode_bits:o}; cleanup result: {cleanup:?}"
                )));
            }
            if metadata.st_uid != linux_effective_uid() {
                drop(fd);
                let cleanup = rustix::fs::unlinkat(
                    &self.fd,
                    name.as_os_str(),
                    rustix::fs::AtFlags::REMOVEDIR,
                );
                return Err(crate::failed(format!(
                    "private namespace is not owned by the current effective UID; cleanup result: {cleanup:?}"
                )));
            }
            if !linux_identity_is_stable(metadata.st_dev, metadata.st_ino) {
                drop(fd);
                let cleanup = rustix::fs::unlinkat(
                    &self.fd,
                    name.as_os_str(),
                    rustix::fs::AtFlags::REMOVEDIR,
                );
                return Err(crate::failed(format!(
                    "private namespace has an unstable filesystem identity; cleanup result: {cleanup:?}"
                )));
            }
            let identity = linux_identity(metadata.st_dev, metadata.st_ino);
            let child = VerifiedDir {
                metadata_path: self.metadata_path.join(name.as_os_str()),
                identity: identity.clone(),
                fd,
            };
            Ok(PrivateNamespace {
                metadata_path: child.metadata_path.clone(),
                identity,
                parent,
                #[cfg(not(windows))]
                leaf: name.clone(),
                dir: child,
            })
        }
        #[cfg(windows)]
        {
            let (handle, identity) = windows_create_owner_only_directory(&self.handle, name)?;
            let path = self.metadata_path.join(name.as_os_str());
            let dir = VerifiedDir {
                metadata_path: path.clone(),
                identity: identity.clone(),
                handle,
            };
            if let Err(error) = windows_verify_private_security(&dir.handle) {
                let cleanup = windows_dispose_by_handle(&dir.handle);
                return Err(crate::failed(match cleanup {
                    Ok(()) => error.to_string(),
                    Err(cleanup_error) => {
                        format!("{error}; cleanup also failed: {cleanup_error}")
                    }
                }));
            }
            Ok(PrivateNamespace {
                metadata_path: path,
                identity,
                parent,
                dir,
            })
        }
        #[cfg(all(
            not(any(target_os = "linux", windows)),
            feature = "development-host-fallback"
        ))]
        {
            let path = self.metadata_path.join(name.as_os_str());
            std::fs::create_dir(&path).map_err(|error| crate::failed(error.to_string()))?;
            #[cfg(unix)]
            std::fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(0o700))
                .map_err(|error| crate::failed(error.to_string()))?;
            Ok(PrivateNamespace {
                metadata_path: path,
                identity: String::new(),
                parent,
                #[cfg(not(windows))]
                leaf: name.clone(),
                dir: VerifiedDir {
                    metadata_path: self.metadata_path.join(name.as_os_str()),
                    identity: String::new(),
                },
            })
        }
        #[cfg(all(
            not(any(target_os = "linux", windows)),
            not(feature = "development-host-fallback")
        ))]
        {
            let _ = (name, capability);
            Err(crate::failed(
                "native private namespace is unsupported on this host",
            ))
        }
    }

    /// Native no-replace rename between retained directories.
    pub(crate) fn rename_no_replace(
        &self,
        source: &LeafName,
        destination_parent: &VerifiedDir,
        destination: &LeafName,
        expected: Option<&EntryObservation>,
        mode: AuthorityMode,
    ) -> Result<RenameNoReplace, ServiceError> {
        mode.preflight()?;
        #[cfg(windows)]
        let source_handle =
            windows_open_entry_relative(&self.handle, source, WindowsOpenIntent::DeleteEntry)
                .map_err(|error| {
                    crate::failed(format!("failed to retain rename source: {error}"))
                })?;
        #[cfg(windows)]
        let source_entry = VerifiedEntry {
            metadata_path: self.metadata_path.join(source.as_os_str()),
            handle: source_handle,
        };
        if let Some(expected) = expected {
            #[cfg(target_os = "linux")]
            let observed = self
                .observe_leaf(source)?
                .ok_or_else(|| crate::failed("rename source is absent"))?;
            #[cfg(windows)]
            let observed = source_entry.observe()?;
            if &observed != expected {
                return Err(crate::failed("rename source identity or digest changed"));
            }
        }
        #[cfg(target_os = "linux")]
        {
            let result = rustix::fs::renameat_with(
                &self.fd,
                source.as_os_str(),
                &destination_parent.fd,
                destination.as_os_str(),
                rustix::fs::RenameFlags::NOREPLACE,
            );
            if let Err(error) = result {
                if error == rustix::io::Errno::EXIST {
                    return Ok(RenameNoReplace::Occupied);
                }
                return Err(crate::failed(format!(
                    "failed to rename entry without replacement: {error}"
                )));
            }
            self.sync()?;
            if !std::ptr::eq(self, destination_parent) {
                destination_parent.sync()?;
            }
            Ok(RenameNoReplace::Moved)
        }
        #[cfg(windows)]
        {
            if let Err(error) = windows_rename_handle_no_replace(
                &source_entry.handle,
                &destination_parent.handle,
                destination,
            ) {
                if matches!(
                    error
                        .raw_os_error()
                        .and_then(|code| u32::try_from(code).ok()),
                    Some(
                        windows_sys::Win32::Foundation::ERROR_FILE_EXISTS
                            | windows_sys::Win32::Foundation::ERROR_ALREADY_EXISTS,
                    )
                ) {
                    return Ok(RenameNoReplace::Occupied);
                }
                return Err(crate::failed(format!(
                    "failed to rename entry without replacement: {error}"
                )));
            }
            self.sync()?;
            if !std::ptr::eq(self, destination_parent) {
                destination_parent.sync()?;
            }
            Ok(RenameNoReplace::Moved)
        }
        #[cfg(all(
            not(any(target_os = "linux", windows)),
            feature = "development-host-fallback"
        ))]
        {
            let source_path = self.metadata_path.join(source.as_os_str());
            let destination_path = destination_parent
                .metadata_path
                .join(destination.as_os_str());
            if destination_path.exists() {
                return Err(crate::failed("rename destination is occupied"));
            }
            std::fs::rename(source_path, destination_path)
                .map_err(|error| crate::failed(error.to_string()))?;
            Ok(RenameNoReplace::Moved)
        }
        #[cfg(all(
            not(any(target_os = "linux", windows)),
            not(feature = "development-host-fallback")
        ))]
        {
            let _ = (source, destination_parent, destination);
            Err(crate::failed(
                "native rename authority is unsupported on this host",
            ))
        }
    }

    /// Publishes a private staged entry into a public directory without
    /// replacement.  The ordinary rename API deliberately preserves the
    /// source security descriptor for custody moves and rollback.  On Windows
    /// only this publication boundary derives the destination parent's DACL
    /// on the retained staged entry immediately before the native rename.
    pub(crate) fn publish_staged_no_replace(
        &self,
        source: &LeafName,
        destination_parent: &VerifiedDir,
        destination: &LeafName,
        expected: &EntryObservation,
        mode: AuthorityMode,
    ) -> Result<RenameNoReplace, ServiceError> {
        mode.preflight()?;
        #[cfg(target_os = "linux")]
        {
            self.rename_no_replace(
                source,
                destination_parent,
                destination,
                Some(expected),
                mode,
            )
        }
        #[cfg(windows)]
        {
            let source_handle = windows_open_entry_relative(
                &self.handle,
                source,
                WindowsOpenIntent::PublishStagedEntry,
            )
            .map_err(|error| {
                crate::failed(format!(
                    "failed to retain staged publication source: {error}"
                ))
            })?;
            let source_entry = VerifiedEntry {
                metadata_path: self.metadata_path.join(source.as_os_str()),
                handle: source_handle,
            };
            let observed = source_entry.observe()?;
            if &observed != expected {
                return Err(crate::failed(
                    "staged publication source identity or digest changed",
                ));
            }
            windows_prepare_staged_publish_security(
                &destination_parent.handle,
                &source_entry.handle,
                observed.is_directory(),
            )?;
            if let Err(error) = windows_rename_handle_no_replace(
                &source_entry.handle,
                &destination_parent.handle,
                destination,
            ) {
                if matches!(
                    error
                        .raw_os_error()
                        .and_then(|code| u32::try_from(code).ok()),
                    Some(
                        windows_sys::Win32::Foundation::ERROR_FILE_EXISTS
                            | windows_sys::Win32::Foundation::ERROR_ALREADY_EXISTS,
                    )
                ) {
                    return Ok(RenameNoReplace::Occupied);
                }
                return Err(crate::failed(format!(
                    "failed to publish staged entry without replacement: {error}"
                )));
            }
            self.sync()?;
            if !std::ptr::eq(self, destination_parent) {
                destination_parent.sync()?;
            }
            Ok(RenameNoReplace::Moved)
        }
        #[cfg(all(
            not(any(target_os = "linux", windows)),
            feature = "development-host-fallback"
        ))]
        {
            self.rename_no_replace(
                source,
                destination_parent,
                destination,
                Some(expected),
                mode,
            )
        }
        #[cfg(all(
            not(any(target_os = "linux", windows)),
            not(feature = "development-host-fallback")
        ))]
        {
            let _ = (source, destination_parent, destination, expected);
            Err(crate::failed(
                "native staged publication is unsupported on this host",
            ))
        }
    }

    /// Remove one exact entry. For directories the native remove operation is
    /// the authoritative emptiness check; no racy pre-check is used.
    pub(crate) fn remove_empty_dir(
        &self,
        name: &LeafName,
        expected_identity: &str,
        mode: AuthorityMode,
    ) -> Result<RemoveEmptyDir, ServiceError> {
        mode.preflight()?;
        #[cfg(target_os = "linux")]
        {
            let fd = match rustix::fs::openat(
                &self.fd,
                name.as_os_str(),
                rustix::fs::OFlags::RDONLY
                    | rustix::fs::OFlags::DIRECTORY
                    | rustix::fs::OFlags::CLOEXEC
                    | rustix::fs::OFlags::NOFOLLOW,
                rustix::fs::Mode::empty(),
            ) {
                Ok(fd) => fd,
                Err(error) if error == rustix::io::Errno::NOENT => {
                    return Ok(RemoveEmptyDir::Absent);
                }
                Err(error) => {
                    return Err(crate::failed(format!(
                        "failed to retain empty-directory target: {error}"
                    )));
                }
            };
            let stat = rustix::fs::fstat(&fd).map_err(|error| {
                crate::failed(format!("failed to inspect empty-directory target: {error}"))
            })?;
            if !linux_identity_is_stable(stat.st_dev, stat.st_ino) {
                return Err(crate::failed(
                    "empty-directory target has an unstable filesystem identity",
                ));
            }
            if linux_identity(stat.st_dev, stat.st_ino) != expected_identity {
                return Ok(RemoveEmptyDir::IdentityChanged);
            }
            match rustix::fs::unlinkat(&self.fd, name.as_os_str(), rustix::fs::AtFlags::REMOVEDIR) {
                Ok(()) => {
                    self.sync()?;
                    Ok(RemoveEmptyDir::Removed)
                }
                Err(error) if error == rustix::io::Errno::NOENT => Ok(RemoveEmptyDir::Absent),
                Err(error) if error == rustix::io::Errno::NOTEMPTY => Ok(RemoveEmptyDir::NotEmpty),
                Err(error) => Err(crate::failed(format!(
                    "failed to remove exact empty directory: {error}"
                ))),
            }
        }
        #[cfg(windows)]
        {
            let entry = match windows_open_entry_relative(
                &self.handle,
                name,
                WindowsOpenIntent::DeleteDirectory,
            ) {
                Ok(handle) => handle,
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
                    return Ok(RemoveEmptyDir::Absent);
                }
                Err(error) => {
                    return Err(crate::failed(format!(
                        "failed to retain empty-directory target: {error}"
                    )));
                }
            };
            if windows_identity(&entry)? != expected_identity {
                return Ok(RemoveEmptyDir::IdentityChanged);
            }
            match windows_dispose_by_handle(&entry) {
                Ok(()) => {
                    self.sync()?;
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
                    "failed to remove exact empty directory: {error}"
                ))),
            }
        }
        #[cfg(all(
            not(any(target_os = "linux", windows)),
            feature = "development-host-fallback"
        ))]
        {
            let path = self.metadata_path.join(name.as_os_str());
            let metadata = match std::fs::symlink_metadata(&path) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    return Ok(RemoveEmptyDir::Absent);
                }
                Err(error) => return Err(crate::failed(error.to_string())),
            };
            if !metadata.is_dir() {
                return Ok(RemoveEmptyDir::IdentityChanged);
            }
            if !expected_identity.is_empty() {
                return Ok(RemoveEmptyDir::IdentityChanged);
            }
            match std::fs::remove_dir(path) {
                Ok(()) => Ok(RemoveEmptyDir::Removed),
                Err(error) if error.kind() == std::io::ErrorKind::DirectoryNotEmpty => {
                    Ok(RemoveEmptyDir::NotEmpty)
                }
                Err(error) => Err(crate::failed(error.to_string())),
            }
        }
        #[cfg(all(
            not(any(target_os = "linux", windows)),
            not(feature = "development-host-fallback")
        ))]
        {
            let _ = (name, expected_identity);
            Err(crate::failed(
                "native empty-directory authority is unsupported on this host",
            ))
        }
    }

    pub(crate) fn remove_empty_dir_exact(
        &self,
        name: &LeafName,
        expected_identity: &str,
        mode: AuthorityMode,
    ) -> Result<(), ServiceError> {
        match self.remove_empty_dir(name, expected_identity, mode)? {
            RemoveEmptyDir::Removed | RemoveEmptyDir::Absent => Ok(()),
            RemoveEmptyDir::NotEmpty => Err(crate::failed("exact directory is not empty")),
            RemoveEmptyDir::IdentityChanged => {
                Err(crate::failed("exact removal target identity changed"))
            }
        }
    }

    pub(crate) fn remove_exact(
        &self,
        name: &LeafName,
        expected: &EntryObservation,
        mode: AuthorityMode,
    ) -> Result<(), ServiceError> {
        mode.preflight()?;
        if expected.is_directory() {
            return match self.remove_empty_dir(name, &expected.identity, mode)? {
                RemoveEmptyDir::Removed => Ok(()),
                RemoveEmptyDir::Absent => Err(crate::failed("exact removal target is absent")),
                RemoveEmptyDir::NotEmpty => Err(crate::failed("exact directory is not empty")),
                RemoveEmptyDir::IdentityChanged => {
                    Err(crate::failed("exact removal target identity changed"))
                }
            };
        }
        #[cfg(not(windows))]
        let observed = self
            .observe_leaf(name)?
            .ok_or_else(|| crate::failed("exact removal target is absent"))?;
        #[cfg(windows)]
        let entry =
            match windows_open_entry_relative(&self.handle, name, WindowsOpenIntent::DeleteEntry) {
                Ok(handle) => VerifiedEntry {
                    metadata_path: self.metadata_path.join(name.as_os_str()),
                    handle,
                },
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
                    return Err(crate::failed("exact removal target is absent"));
                }
                Err(error) => {
                    return Err(crate::failed(format!(
                        "failed to retain exact removal target: {error}"
                    )));
                }
            };
        #[cfg(windows)]
        let observed = entry.observe()?;
        if &observed != expected {
            return Err(crate::failed(
                "exact removal target identity or digest changed",
            ));
        }
        #[cfg(target_os = "linux")]
        {
            rustix::fs::unlinkat(&self.fd, name.as_os_str(), rustix::fs::AtFlags::empty())
                .map_err(|error| crate::failed(format!("failed to remove exact entry: {error}")))?;
            self.sync()?;
            Ok(())
        }
        #[cfg(windows)]
        {
            windows_dispose_by_handle(&entry.handle)
                .map_err(|error| crate::failed(format!("failed to remove exact entry: {error}")))?;
            self.sync()?;
            Ok(())
        }
        #[cfg(all(
            not(any(target_os = "linux", windows)),
            feature = "development-host-fallback"
        ))]
        {
            let path = self.metadata_path.join(name.as_os_str());
            if expected.is_directory() {
                std::fs::remove_dir(path).map_err(|error| crate::failed(error.to_string()))?;
            } else {
                std::fs::remove_file(path).map_err(|error| crate::failed(error.to_string()))?;
            }
            Ok(())
        }
        #[cfg(all(
            not(any(target_os = "linux", windows)),
            not(feature = "development-host-fallback")
        ))]
        {
            let _ = (name, expected);
            Err(crate::failed(
                "native removal authority is unsupported on this host",
            ))
        }
    }

    pub(crate) fn sync(&self) -> Result<(), ServiceError> {
        #[cfg(target_os = "linux")]
        {
            rustix::fs::fsync(&self.fd).map_err(|error| {
                crate::failed(format!("failed to sync directory authority: {error}"))
            })?;
            Ok(())
        }
        #[cfg(windows)]
        {
            let _ = self;
            // The retained root/parent contract deliberately omits generic
            // write access. Windows directory flush APIs reject that narrow
            // mask; directory-entry durability is sequenced by the durable
            // binding layer, while file writes are synced on their retained
            // file handles.
            Ok(())
        }
        #[cfg(not(any(target_os = "linux", windows)))]
        {
            let _ = self;
            Ok(())
        }
    }

    pub(crate) fn clone_capability(&self) -> Result<Self, ServiceError> {
        #[cfg(target_os = "linux")]
        {
            let fd = rustix::io::fcntl_dupfd_cloexec(&self.fd, 0).map_err(|error| {
                crate::failed(format!("failed to duplicate directory authority: {error}"))
            })?;
            Ok(Self {
                metadata_path: self.metadata_path.clone(),
                identity: self.identity.clone(),
                fd,
            })
        }
        #[cfg(windows)]
        {
            Ok(Self {
                metadata_path: self.metadata_path.clone(),
                identity: self.identity.clone(),
                handle: self.handle.try_clone().map_err(|error| {
                    crate::failed(format!("failed to duplicate directory authority: {error}"))
                })?,
            })
        }
        #[cfg(all(
            not(any(target_os = "linux", windows)),
            feature = "development-host-fallback"
        ))]
        {
            Ok(Self {
                metadata_path: self.metadata_path.clone(),
                identity: self.identity.clone(),
            })
        }
    }
}
