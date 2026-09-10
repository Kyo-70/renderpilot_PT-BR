use super::super::*;

impl VerifiedDir {
    #[cfg(target_os = "linux")]
    pub(in crate::fs::authority) fn open_linux(
        path: &Path,
        expected_identity: Option<&str>,
    ) -> Result<Self, ServiceError> {
        let mut fd = rustix::fs::open(
            Path::new("/"),
            rustix::fs::OFlags::RDONLY
                | rustix::fs::OFlags::DIRECTORY
                | rustix::fs::OFlags::CLOEXEC
                | rustix::fs::OFlags::NOFOLLOW,
            rustix::fs::Mode::empty(),
        )
        .map_err(|error| {
            crate::failed(format!("failed to open directory authority root: {error}"))
        })?;
        for component in path.components() {
            let Component::Normal(name) = component else {
                if matches!(component, Component::RootDir) {
                    continue;
                }
                return Err(crate::failed(format!(
                    "directory authority path contains unsupported component: {}",
                    path.display()
                )));
            };
            fd = rustix::fs::openat(
                &fd,
                name,
                rustix::fs::OFlags::RDONLY
                    | rustix::fs::OFlags::DIRECTORY
                    | rustix::fs::OFlags::CLOEXEC
                    | rustix::fs::OFlags::NOFOLLOW,
                rustix::fs::Mode::empty(),
            )
            .map_err(|error| {
                crate::failed(format!(
                    "failed to walk directory authority component `{}` of `{}`: {error}",
                    name.to_string_lossy(),
                    path.display()
                ))
            })?;
        }
        let stat = rustix::fs::fstat(&fd).map_err(|error| {
            crate::failed(format!("failed to inspect directory authority: {error}"))
        })?;
        if !linux_identity_is_stable(stat.st_dev, stat.st_ino) {
            return Err(crate::failed(
                "directory authority has an unstable filesystem identity",
            ));
        }
        let identity = linux_identity(stat.st_dev, stat.st_ino);
        if let Some(expected_identity) = expected_identity
            && expected_identity != identity
        {
            return Err(crate::failed("directory authority identity changed"));
        }
        Ok(Self {
            metadata_path: path.to_owned(),
            identity,
            fd,
        })
    }
}

#[cfg(target_os = "linux")]
pub(crate) fn linux_effective_uid() -> u32 {
    #[expect(unsafe_code, reason = "geteuid is the Linux native ownership boundary")]
    unsafe extern "C" {
        fn geteuid() -> u32;
    }
    #[expect(unsafe_code, reason = "geteuid is the Linux native ownership boundary")]
    unsafe {
        geteuid()
    }
}

#[cfg(target_os = "linux")]
pub(crate) fn linux_identity(device: u64, inode: u64) -> String {
    format!("unix:{device:x}:{inode:x}")
}

#[cfg(target_os = "linux")]
pub(crate) fn linux_identity_is_stable(device: u64, inode: u64) -> bool {
    device != 0 && inode != 0
}

#[cfg(target_os = "linux")]
pub(crate) fn linux_cleanup_created_file(
    parent: &VerifiedDir,
    name: &LeafName,
    context: &str,
) -> ServiceError {
    match rustix::fs::unlinkat(&parent.fd, name.as_os_str(), rustix::fs::AtFlags::empty()) {
        Ok(()) => match parent.sync() {
            Ok(()) => crate::failed(context),
            Err(sync_error) => crate::failed(format!(
                "{context}; created-file cleanup sync failed: {sync_error}"
            )),
        },
        Err(error) if error == rustix::io::Errno::NOENT => {
            crate::failed(format!("{context}; created-file cleanup found no artifact"))
        }
        Err(error) => crate::failed(format!("{context}; created-file cleanup failed: {error}")),
    }
}

#[cfg(target_os = "linux")]
pub(crate) fn hash_fd(fd: &rustix::fd::OwnedFd) -> Result<String, ServiceError> {
    use std::os::unix::fs::FileExt;
    let duplicate = rustix::io::fcntl_dupfd_cloexec(fd, 0).map_err(|error| {
        crate::failed(format!(
            "failed to duplicate file handle for hashing: {error}"
        ))
    })?;
    let file = File::from(duplicate);
    let mut hasher = sha2::Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut offset = 0_u64;
    loop {
        let read = file
            .read_at(&mut buffer, offset)
            .map_err(|error| crate::failed(format!("failed to hash retained file: {error}")))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        offset = offset
            .checked_add(read as u64)
            .ok_or_else(|| crate::failed("file offset overflow during hashing"))?;
    }
    Ok(hex::encode(hasher.finalize()))
}
