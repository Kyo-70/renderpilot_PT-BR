use crate::ServiceError;
use crate::fs::authority::{LeafName, VerifiedDir};
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Component, Path, PathBuf};

use super::entry_open::{WindowsOpenIntent, windows_open_entry_relative};
use super::identity::windows_identity;

impl VerifiedDir {
    pub(in crate::fs::authority) fn open_windows(
        path: &Path,
        expected_identity: Option<&str>,
    ) -> Result<Self, ServiceError> {
        let mut components = path.components().peekable();
        let prefix = match components.next() {
            Some(Component::Prefix(prefix)) => prefix.as_os_str().to_owned(),
            _ => {
                return Err(crate::failed(
                    "Windows directory authority requires an absolute prefix",
                ));
            }
        };
        if !matches!(components.next(), Some(Component::RootDir)) {
            return Err(crate::failed(
                "Windows directory authority requires a rooted path",
            ));
        }
        let mut root_path = PathBuf::from(prefix);
        root_path.push(Path::new(r"\"));
        let mut handle = windows_open_directory_root(&root_path).map_err(|error| {
            crate::failed(format!("failed to open directory authority root: {error}"))
        })?;
        let mut walked_path = root_path;
        while let Some(component) = components.next() {
            let Component::Normal(name) = component else {
                return Err(crate::failed(
                    "Windows directory authority contains unsupported component",
                ));
            };
            let name = LeafName::parse(name)?;
            let intent = if components.peek().is_none() {
                WindowsOpenIntent::MutateDirectoryChildren
            } else {
                WindowsOpenIntent::TraverseDirectory
            };
            walked_path.push(name.as_os_str());
            handle = windows_open_entry_relative(&handle, &name, intent).map_err(|error| {
                crate::failed(format!(
                    "failed to walk directory authority {}: {error}",
                    walked_path.display()
                ))
            })?;
            let metadata = handle.metadata().map_err(|error| {
                crate::failed(format!(
                    "failed to inspect walked directory authority: {error}"
                ))
            })?;
            if !metadata.is_dir() || metadata.file_attributes() & 0x400 != 0 {
                return Err(crate::failed(
                    "directory authority walk encountered a non-directory or reparse point",
                ));
            }
        }
        let identity = windows_identity(&handle)?;
        if let Some(expected_identity) = expected_identity
            && expected_identity != identity
        {
            return Err(crate::failed("directory authority identity changed"));
        }
        Ok(Self {
            metadata_path: path.to_owned(),
            identity,
            handle,
        })
    }
}

fn windows_open_directory_root(path: &Path) -> std::io::Result<std::fs::File> {
    let handle = windows_open_directory_handle(path)?;
    let metadata = handle.metadata()?;
    if !metadata.is_dir() || metadata.file_attributes() & 0x400 != 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Windows authority root is not a non-reparse directory",
        ));
    }
    Ok(handle)
}

fn windows_open_directory_handle(path: &Path) -> std::io::Result<std::fs::File> {
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_LIST_DIRECTORY,
        FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_TRAVERSE,
        READ_CONTROL, SYNCHRONIZE,
    };
    let handle = std::fs::OpenOptions::new()
        .read(true)
        .access_mode(
            FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES | FILE_TRAVERSE | READ_CONTROL | SYNCHRONIZE,
        )
        // Root walk handle shares deletion so that traversal stays compatible
        // with a peer holding a narrower (non-delete) retained child handle.
        // The final MutateDirectoryChildren handle deliberately drops the share.
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)?;
    Ok(handle)
}
