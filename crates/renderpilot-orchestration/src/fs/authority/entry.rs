use super::*;
#[cfg(windows)]
use std::os::windows::fs::MetadataExt;

/// A retained file or directory entry opened relative to a [`VerifiedDir`].
pub(crate) struct VerifiedEntry {
    pub(in crate::fs::authority) metadata_path: PathBuf,
    #[cfg(target_os = "linux")]
    pub(in crate::fs::authority) fd: rustix::fd::OwnedFd,
    #[cfg(windows)]
    pub(in crate::fs::authority) handle: File,
}

impl std::fmt::Debug for VerifiedEntry {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("VerifiedEntry")
            .field("metadata_path", &self.metadata_path)
            .finish_non_exhaustive()
    }
}

fn read_to_end_bounded<R: Read>(
    reader: &mut R,
    initial_len: u64,
    max_bytes: usize,
) -> Result<Vec<u8>, ServiceError> {
    let max_bytes_u64 = u64::try_from(max_bytes)
        .map_err(|_| crate::failed("bounded retained-file read limit is too large"))?;
    if initial_len > max_bytes_u64 {
        return Err(crate::failed(format!(
            "retained file exceeds bounded read limit of {max_bytes_u64} bytes"
        )));
    }
    let capacity = usize::try_from(initial_len)
        .map_err(|_| crate::failed("retained file length cannot fit in memory"))?;
    let mut bytes = Vec::with_capacity(capacity);
    let mut buffer = [0_u8; 64 * 1024];
    while bytes.len() < max_bytes {
        if bytes.capacity() == bytes.len() {
            let next_capacity = bytes.capacity().saturating_mul(2).max(1).min(max_bytes);
            bytes
                .try_reserve_exact(next_capacity - bytes.len())
                .map_err(|error| {
                    crate::failed(format!(
                        "failed to allocate bounded retained-file buffer: {error}"
                    ))
                })?;
        }
        let available = (bytes.capacity() - bytes.len()).min(max_bytes - bytes.len());
        let chunk_len = available.min(buffer.len());
        let read = reader
            .read(&mut buffer[..chunk_len])
            .map_err(|error| crate::failed(format!("failed to read retained file: {error}")))?;
        if read == 0 {
            return Ok(bytes);
        }
        bytes.extend_from_slice(&buffer[..read]);
    }
    let mut extra = [0_u8; 1];
    if reader
        .read(&mut extra)
        .map_err(|error| crate::failed(format!("failed to read retained file: {error}")))?
        != 0
    {
        return Err(crate::failed(format!(
            "retained file grew beyond bounded read limit of {max_bytes_u64} bytes"
        )));
    }
    Ok(bytes)
}

fn read_file_stream<R: Read>(
    reader: &mut R,
    initial_len: u64,
    max_bytes: Option<usize>,
) -> Result<Vec<u8>, ServiceError> {
    if let Some(max_bytes) = max_bytes {
        read_to_end_bounded(reader, initial_len, max_bytes)
    } else {
        let mut bytes = Vec::new();
        reader
            .read_to_end(&mut bytes)
            .map_err(|error| crate::failed(format!("failed to read retained file: {error}")))?;
        Ok(bytes)
    }
}

#[cfg(target_os = "linux")]
struct PositionalReader<'a> {
    file: &'a File,
    offset: u64,
}

#[cfg(target_os = "linux")]
impl Read for PositionalReader<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        use std::os::unix::fs::FileExt;
        let count = self.file.read_at(buf, self.offset)?;
        self.offset = self
            .offset
            .checked_add(count as u64)
            .ok_or_else(|| std::io::Error::other("file offset overflow"))?;
        Ok(count)
    }
}

impl VerifiedEntry {
    /// Read a regular file through the retained entry handle and return the
    /// bytes together with the observation derived from that same handle.
    /// When `expected` is supplied, identity, kind, and digest must all match
    /// before the result is accepted.
    pub(crate) fn read_regular_file(
        &self,
        expected: Option<&EntryObservation>,
    ) -> Result<(Vec<u8>, EntryObservation), ServiceError> {
        self.read_regular_file_with_limit(expected, None)
    }

    fn read_regular_file_with_limit(
        &self,
        expected: Option<&EntryObservation>,
        max_bytes: Option<usize>,
    ) -> Result<(Vec<u8>, EntryObservation), ServiceError> {
        #[cfg(target_os = "linux")]
        {
            let duplicate = rustix::io::fcntl_dupfd_cloexec(&self.fd, 0).map_err(|error| {
                crate::failed(format!("failed to duplicate retained file handle: {error}"))
            })?;
            let file = File::from(duplicate);
            let metadata = file.metadata().map_err(|error| {
                crate::failed(format!("failed to inspect retained file: {error}"))
            })?;
            use std::os::unix::fs::MetadataExt;
            if !metadata.is_file() {
                return Err(crate::failed("retained entry is not a regular file"));
            }
            if !linux_identity_is_stable(metadata.dev(), metadata.ino()) {
                return Err(crate::failed(
                    "retained file has an unstable filesystem identity",
                ));
            }
            let mut reader = PositionalReader {
                file: &file,
                offset: 0,
            };
            let bytes = read_file_stream(&mut reader, metadata.len(), max_bytes)?;
            let observation = EntryObservation {
                kind: EntryKind::File,
                identity: linux_identity(metadata.dev(), metadata.ino()),
                digest: Some(hex::encode(sha2::Sha256::digest(&bytes))),
            };
            if expected.is_some_and(|expected| expected != &observation) {
                return Err(crate::failed("retained file identity or digest changed"));
            }
            Ok((bytes, observation))
        }
        #[cfg(windows)]
        {
            let mut file = self.handle.try_clone().map_err(|error| {
                crate::failed(format!("failed to duplicate retained file handle: {error}"))
            })?;
            let metadata = file.metadata().map_err(|error| {
                crate::failed(format!("failed to inspect retained file: {error}"))
            })?;
            if metadata.file_attributes() & 0x400 != 0 || !metadata.is_file() {
                return Err(crate::failed(
                    "retained entry is not a regular non-reparse file",
                ));
            }
            file.seek(SeekFrom::Start(0))
                .map_err(|error| crate::failed(format!("failed to seek retained file: {error}")))?;
            let bytes = read_file_stream(&mut file, metadata.len(), max_bytes)?;
            let observation = EntryObservation {
                kind: EntryKind::File,
                identity: windows_identity(&self.handle)?,
                digest: Some(hex::encode(sha2::Sha256::digest(&bytes))),
            };
            if expected.is_some_and(|expected| expected != &observation) {
                return Err(crate::failed("retained file identity or digest changed"));
            }
            Ok((bytes, observation))
        }
        #[cfg(all(
            not(any(target_os = "linux", windows)),
            feature = "development-host-fallback"
        ))]
        {
            let mut file = std::fs::OpenOptions::new()
                .read(true)
                .open(&self.metadata_path)
                .map_err(|error| crate::failed(error.to_string()))?;
            let metadata = file
                .metadata()
                .map_err(|error| crate::failed(error.to_string()))?;
            if !metadata.is_file() {
                return Err(crate::failed("retained entry is not a regular file"));
            }
            let bytes = read_file_stream(&mut file, metadata.len(), max_bytes)?;
            let observation = self.observe()?;
            if observation.kind != EntryKind::File {
                return Err(crate::failed("retained entry is not a regular file"));
            }
            if expected.is_some_and(|expected| expected != &observation) {
                return Err(crate::failed("retained file identity or digest changed"));
            }
            Ok((bytes, observation))
        }
        #[cfg(all(
            not(any(target_os = "linux", windows)),
            not(feature = "development-host-fallback")
        ))]
        {
            let _ = expected;
            Err(crate::failed(
                "native retained-file read is unsupported on this host",
            ))
        }
    }

    pub(in crate::fs::authority) fn overwrite_regular_file(
        &self,
        expected: &EntryObservation,
        bytes: &[u8],
    ) -> Result<EntryObservation, ServiceError> {
        let observed = self.observe()?;
        if &observed != expected || observed.kind != EntryKind::File {
            return Err(crate::failed(
                "retained file identity or digest changed before update",
            ));
        }
        self.overwrite_regular_file_after_observation(&observed.identity, bytes)
    }

    pub(in crate::fs::authority) fn overwrite_regular_file_with_stable_identity(
        &self,
        current: &EntryObservation,
        stable_identity: &str,
        bytes: &[u8],
    ) -> Result<EntryObservation, ServiceError> {
        let observed = self.observe()?;
        if &observed != current
            || observed.kind != EntryKind::File
            || observed.identity != stable_identity
        {
            return Err(crate::failed(
                "retained file identity or current observation changed before update",
            ));
        }
        self.overwrite_regular_file_after_observation(stable_identity, bytes)
    }

    fn overwrite_regular_file_after_observation(
        &self,
        stable_identity: &str,
        bytes: &[u8],
    ) -> Result<EntryObservation, ServiceError> {
        #[cfg(target_os = "linux")]
        {
            let duplicate = rustix::io::fcntl_dupfd_cloexec(&self.fd, 0).map_err(|error| {
                crate::failed(format!("failed to duplicate retained file handle: {error}"))
            })?;
            let mut file = File::from(duplicate);
            file.set_len(0)
                .and_then(|()| file.seek(SeekFrom::Start(0)).map(|_| ()))
                .and_then(|()| file.write_all(bytes))
                .and_then(|()| file.sync_all())
                .map_err(|error| {
                    crate::failed(format!("failed to update retained file: {error}"))
                })?;
        }
        #[cfg(windows)]
        {
            let mut file = self.handle.try_clone().map_err(|error| {
                crate::failed(format!("failed to duplicate retained file handle: {error}"))
            })?;
            file.set_len(0)
                .and_then(|()| file.seek(SeekFrom::Start(0)).map(|_| ()))
                .and_then(|()| file.write_all(bytes))
                .and_then(|()| file.sync_all())
                .map_err(|error| {
                    crate::failed(format!("failed to update retained file: {error}"))
                })?;
        }
        #[cfg(all(
            not(any(target_os = "linux", windows)),
            feature = "development-host-fallback"
        ))]
        {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .open(&self.metadata_path)
                .map_err(|error| crate::failed(error.to_string()))?;
            file.set_len(0)
                .and_then(|()| file.seek(SeekFrom::Start(0)))
                .and_then(|()| file.write_all(bytes))
                .and_then(|()| file.sync_all())
                .map_err(|error| crate::failed(error.to_string()))?;
        }
        #[cfg(all(
            not(any(target_os = "linux", windows)),
            not(feature = "development-host-fallback")
        ))]
        {
            let _ = bytes;
            return Err(crate::failed(
                "native file update authority is unsupported on this host",
            ));
        }
        let updated = self.observe()?;
        if updated.kind != EntryKind::File || updated.identity != stable_identity {
            return Err(crate::failed(
                "retained file identity changed during update",
            ));
        }
        Ok(updated)
    }

    pub(crate) fn observe(&self) -> Result<EntryObservation, ServiceError> {
        #[cfg(target_os = "linux")]
        {
            let duplicate = rustix::io::fcntl_dupfd_cloexec(&self.fd, 0).map_err(|error| {
                crate::failed(format!("failed to duplicate entry handle: {error}"))
            })?;
            let metadata = File::from(duplicate).metadata().map_err(|error| {
                crate::failed(format!("failed to inspect retained entry: {error}"))
            })?;
            use std::os::unix::fs::MetadataExt;
            let kind = if metadata.is_dir() {
                EntryKind::Directory
            } else if metadata.is_file() {
                EntryKind::File
            } else {
                return Err(crate::failed(
                    "retained entry is not a regular file or directory",
                ));
            };
            if !linux_identity_is_stable(metadata.dev(), metadata.ino()) {
                return Err(crate::failed(
                    "retained entry has an unstable filesystem identity",
                ));
            }
            let digest = if kind == EntryKind::File {
                Some(hash_fd(&self.fd)?)
            } else {
                None
            };
            Ok(EntryObservation {
                kind,
                identity: linux_identity(metadata.dev(), metadata.ino()),
                digest,
            })
        }
        #[cfg(windows)]
        {
            let metadata = self.handle.metadata().map_err(|error| {
                crate::failed(format!("failed to inspect retained entry: {error}"))
            })?;
            if metadata.file_attributes() & 0x400 != 0 {
                return Err(crate::failed("retained entry is a reparse point"));
            }
            let kind = if metadata.is_dir() {
                EntryKind::Directory
            } else if metadata.is_file() {
                EntryKind::File
            } else {
                return Err(crate::failed(
                    "retained entry is not a regular file or directory",
                ));
            };
            let digest = if kind == EntryKind::File {
                Some(hash_file(&self.handle)?)
            } else {
                None
            };
            Ok(EntryObservation {
                kind,
                identity: windows_identity(&self.handle)?,
                digest,
            })
        }
        #[cfg(all(
            not(any(target_os = "linux", windows)),
            feature = "development-host-fallback"
        ))]
        {
            let metadata = std::fs::symlink_metadata(&self.metadata_path)
                .map_err(|error| crate::failed(error.to_string()))?;
            let kind = if metadata.is_dir() {
                EntryKind::Directory
            } else {
                EntryKind::File
            };
            let digest = if kind == EntryKind::File {
                Some(
                    crate::fs::sha256_of_non_empty_file(&self.metadata_path)
                        .map_err(|error| crate::failed(error.to_string()))?,
                )
            } else {
                None
            };
            Ok(EntryObservation {
                kind,
                identity: String::new(),
                digest,
            })
        }
        #[cfg(all(
            not(any(target_os = "linux", windows)),
            not(feature = "development-host-fallback")
        ))]
        {
            Err(crate::failed(
                "native entry authority is unsupported on this host",
            ))
        }
    }
}
