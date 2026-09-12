use super::super::*;

impl VerifiedDir {
    pub(crate) fn observe_leaf(
        &self,
        name: &LeafName,
    ) -> Result<Option<EntryObservation>, ServiceError> {
        #[cfg(target_os = "linux")]
        {
            let fd = match rustix::fs::openat(
                &self.fd,
                name.as_os_str(),
                rustix::fs::OFlags::RDONLY
                    | rustix::fs::OFlags::CLOEXEC
                    | rustix::fs::OFlags::NOFOLLOW
                    | rustix::fs::OFlags::NONBLOCK,
                rustix::fs::Mode::empty(),
            ) {
                Ok(fd) => fd,
                Err(error) if error == rustix::io::Errno::NOENT => return Ok(None),
                Err(error) => {
                    return Err(crate::failed(format!(
                        "failed to open relative entry: {error}"
                    )));
                }
            };
            VerifiedEntry {
                metadata_path: self.metadata_path.join(name.as_os_str()),
                fd,
            }
            .observe()
            .map(Some)
        }
        #[cfg(windows)]
        {
            let handle =
                match windows_open_entry_relative(&self.handle, name, WindowsOpenIntent::Observe) {
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
                        return Ok(None);
                    }
                    Err(error) => {
                        return Err(crate::failed(format!(
                            "failed to open relative entry: {error}"
                        )));
                    }
                };
            VerifiedEntry {
                metadata_path: self.metadata_path.join(name.as_os_str()),
                handle,
            }
            .observe()
            .map(Some)
        }
        #[cfg(all(
            not(any(target_os = "linux", windows)),
            feature = "development-host-fallback"
        ))]
        {
            let path = self.metadata_path.join(name.as_os_str());
            return match std::fs::symlink_metadata(&path) {
                Ok(_) => self
                    .open_leaf(name)
                    .map(|entry| entry.observe())
                    .transpose(),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(error) => Err(crate::failed(error.to_string())),
            };
        }
        #[cfg(all(
            not(any(target_os = "linux", windows)),
            not(feature = "development-host-fallback")
        ))]
        {
            let _ = name;
            Err(crate::failed(
                "native entry authority is unsupported on this host",
            ))
        }
    }

    /// Read one regular file through a retained child handle.
    pub(crate) fn read_regular_file(
        &self,
        name: &LeafName,
        expected: Option<&EntryObservation>,
    ) -> Result<(Vec<u8>, EntryObservation), ServiceError> {
        self.open_leaf(name)?.read_regular_file(expected)
    }

    /// Read one regular file through a retained child handle with a strict
    /// allocation limit. The child is opened only after the directory
    /// capability has been retained, matching [`Self::read_regular_file`].
    pub(crate) fn read_regular_file_bounded(
        &self,
        name: &LeafName,
        expected: Option<&EntryObservation>,
        max_bytes: usize,
    ) -> Result<(Vec<u8>, EntryObservation), ServiceError> {
        self.open_leaf(name)?
            .read_regular_file_bounded(expected, max_bytes)
    }

    /// Rewrite the bytes of an already verified regular file through its
    /// retained native handle. The directory entry is never replaced, so a
    /// durable file identity remains stable across an Owned content update.
    /// This is identity-safe but not atomically visible: durable transaction
    /// recovery protects the before-image if a write is interrupted.
    pub(crate) fn overwrite_regular_file(
        &self,
        name: &LeafName,
        expected: &EntryObservation,
        bytes: &[u8],
    ) -> Result<EntryObservation, ServiceError> {
        let entry = self.open_leaf_for_update(name)?;
        entry.overwrite_regular_file(expected, bytes)
    }

    /// Rewrite a retained regular file after an exact current observation,
    /// while binding the update to the entry identity that was authorized by
    /// the caller.  The digest may have advanced since the durable intent was
    /// written, but the native object may not have been replaced.
    pub(crate) fn overwrite_regular_file_with_stable_identity(
        &self,
        name: &LeafName,
        current: &EntryObservation,
        stable_identity: &str,
        bytes: &[u8],
    ) -> Result<EntryObservation, ServiceError> {
        let entry = self.open_leaf_for_update(name)?;
        entry.overwrite_regular_file_with_stable_identity(current, stable_identity, bytes)
    }

    /// Create, write, sync, and retain a regular file relative to this
    /// directory. The native create disposition is exclusive: an occupied
    /// destination returns [`CreateFileNoReplace::Occupied`] and leaves both
    /// entries untouched.
    pub(crate) fn create_file_no_replace(
        &self,
        name: &LeafName,
        bytes: &[u8],
        mode: AuthorityMode,
    ) -> Result<CreateFileNoReplace, ServiceError> {
        mode.preflight()?;
        #[cfg(target_os = "linux")]
        {
            let fd = match rustix::fs::openat(
                &self.fd,
                name.as_os_str(),
                rustix::fs::OFlags::RDWR
                    | rustix::fs::OFlags::CREATE
                    | rustix::fs::OFlags::EXCL
                    | rustix::fs::OFlags::CLOEXEC
                    | rustix::fs::OFlags::NOFOLLOW,
                rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR,
            ) {
                Ok(fd) => fd,
                Err(error) if error == rustix::io::Errno::EXIST => {
                    return Ok(CreateFileNoReplace::Occupied);
                }
                Err(error) => {
                    return Err(crate::failed(format!(
                        "failed to create relative file without replacement: {error}"
                    )));
                }
            };
            let mut written = 0;
            while written < bytes.len() {
                let count = rustix::io::write(&fd, &bytes[written..]).map_err(|error| {
                    crate::failed(format!("failed to write retained file: {error}"))
                });
                let count = match count {
                    Ok(count) if count != 0 => count,
                    Ok(_) => {
                        drop(fd);
                        return Err(linux_cleanup_created_file(
                            self,
                            name,
                            "native file write returned zero before completion",
                        ));
                    }
                    Err(error) => {
                        drop(fd);
                        return Err(linux_cleanup_created_file(self, name, &error.to_string()));
                    }
                };
                written += count;
            }
            if let Err(error) = rustix::fs::fsync(&fd) {
                drop(fd);
                return Err(linux_cleanup_created_file(
                    self,
                    name,
                    &format!("failed to sync retained file: {error}"),
                ));
            }
            let entry = VerifiedEntry {
                metadata_path: self.metadata_path.join(name.as_os_str()),
                fd,
            };
            let observation = match entry.observe() {
                Ok(observation) if observation.kind == EntryKind::File => observation,
                Ok(_) => {
                    drop(entry);
                    return Err(linux_cleanup_created_file(
                        self,
                        name,
                        "new relative entry is not a regular file",
                    ));
                }
                Err(error) => {
                    drop(entry);
                    return Err(linux_cleanup_created_file(self, name, &error.to_string()));
                }
            };
            if let Err(error) = self.sync() {
                drop(entry);
                return Err(linux_cleanup_created_file(self, name, &error.to_string()));
            }
            Ok(CreateFileNoReplace::Created {
                _entry: entry,
                observation,
            })
        }
        #[cfg(windows)]
        {
            let file = match windows_create_file_relative(&self.handle, name) {
                Ok(file) => file,
                Err(error)
                    if matches!(
                        error
                            .raw_os_error()
                            .and_then(|code| u32::try_from(code).ok()),
                        Some(
                            windows_sys::Win32::Foundation::ERROR_FILE_EXISTS
                                | windows_sys::Win32::Foundation::ERROR_ALREADY_EXISTS,
                        )
                    ) =>
                {
                    return Ok(CreateFileNoReplace::Occupied);
                }
                Err(error) => {
                    return Err(crate::failed(format!(
                        "failed to create relative file without replacement: {error}"
                    )));
                }
            };
            let mut file = file;
            if let Err(error) = file.write_all(bytes).and_then(|()| file.sync_all()) {
                let cleanup = windows_dispose_by_handle(&file);
                return Err(crate::failed(match cleanup {
                    Ok(()) => format!("failed to write and sync retained file: {error}"),
                    Err(cleanup_error) => format!(
                        "failed to write and sync retained file: {error}; cleanup also failed: {cleanup_error}"
                    ),
                }));
            }
            let entry = VerifiedEntry {
                metadata_path: self.metadata_path.join(name.as_os_str()),
                handle: file,
            };
            let observation = match entry.observe() {
                Ok(observation) if observation.kind == EntryKind::File => observation,
                Ok(_) => {
                    let cleanup = windows_dispose_by_handle(&entry.handle);
                    return Err(crate::failed(match cleanup {
                        Ok(()) => "new relative entry is not a regular file".to_owned(),
                        Err(cleanup_error) => format!(
                            "new relative entry is not a regular file; cleanup also failed: {cleanup_error}"
                        ),
                    }));
                }
                Err(error) => {
                    let cleanup = windows_dispose_by_handle(&entry.handle);
                    return Err(crate::failed(match cleanup {
                        Ok(()) => error.to_string(),
                        Err(cleanup_error) => {
                            format!("{error}; cleanup also failed: {cleanup_error}")
                        }
                    }));
                }
            };
            if let Err(error) = self.sync() {
                let cleanup = windows_dispose_by_handle(&entry.handle);
                return Err(crate::failed(match cleanup {
                    Ok(()) => error.to_string(),
                    Err(cleanup_error) => {
                        format!("{error}; cleanup also failed: {cleanup_error}")
                    }
                }));
            }
            Ok(CreateFileNoReplace::Created {
                _entry: entry,
                observation,
            })
        }
        #[cfg(all(
            not(any(target_os = "linux", windows)),
            feature = "development-host-fallback"
        ))]
        {
            let path = self.metadata_path.join(name.as_os_str());
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            let mut file = match options.open(&path) {
                Ok(file) => file,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    return Ok(CreateFileNoReplace::Occupied);
                }
                Err(error) => return Err(crate::failed(error.to_string())),
            };
            file.write_all(bytes)
                .and_then(|()| file.sync_all())
                .map_err(|error| crate::failed(error.to_string()))?;
            let entry = VerifiedEntry {
                metadata_path: path,
            };
            let observation = entry.observe()?;
            self.sync()?;
            Ok(CreateFileNoReplace::Created {
                _entry: entry,
                observation,
            })
        }
        #[cfg(all(
            not(any(target_os = "linux", windows)),
            not(feature = "development-host-fallback")
        ))]
        {
            let _ = (name, bytes);
            Err(crate::failed(
                "native relative file creation is unsupported on this host",
            ))
        }
    }
}
