use crate::fs::authority::LeafName;
use std::fs::File;

use super::identity::windows_status_error;

#[derive(Clone, Copy)]
pub(crate) enum WindowsOpenIntent {
    Observe,
    MutateEntry,
    TraverseDirectory,
    MutateDirectoryChildren,
    ReopenDirectory,
    DeleteDirectory,
    DeleteEntry,
    PublishStagedEntry,
}

#[expect(unsafe_code, reason = "Windows retained-handle relative open")]
pub(crate) fn windows_open_entry_relative(
    parent: &File,
    name: &LeafName,
    intent: WindowsOpenIntent,
) -> std::io::Result<File> {
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::{AsRawHandle, FromRawHandle};
    use windows_sys::Wdk::Foundation::OBJECT_ATTRIBUTES;
    use windows_sys::Wdk::Storage::FileSystem::{
        FILE_DIRECTORY_FILE, FILE_OPEN, FILE_OPEN_REPARSE_POINT, FILE_SYNCHRONOUS_IO_NONALERT,
        NtCreateFile,
    };
    use windows_sys::Win32::Foundation::{
        GENERIC_READ, GENERIC_WRITE, HANDLE, OBJ_CASE_INSENSITIVE, UNICODE_STRING,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        DELETE, FILE_ATTRIBUTE_NORMAL, FILE_LIST_DIRECTORY, FILE_READ_ATTRIBUTES,
        FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_TRAVERSE, READ_CONTROL,
        SYNCHRONIZE,
    };
    use windows_sys::Win32::System::IO::IO_STATUS_BLOCK;
    const OBJECT_ATTRIBUTES_BYTES: u32 = std::mem::size_of::<OBJECT_ATTRIBUTES>() as u32;
    let wide = name.as_os_str().encode_wide().collect::<Vec<_>>();
    let byte_length = wide
        .len()
        .checked_mul(2)
        .and_then(|length| u16::try_from(length).ok())
        .ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "entry name is too long")
        })?;
    let unicode = UNICODE_STRING {
        Length: byte_length,
        MaximumLength: byte_length,
        Buffer: wide.as_ptr().cast_mut(),
    };
    let attributes = OBJECT_ATTRIBUTES {
        Length: OBJECT_ATTRIBUTES_BYTES,
        RootDirectory: parent.as_raw_handle(),
        ObjectName: &raw const unicode,
        Attributes: OBJ_CASE_INSENSITIVE,
        SecurityDescriptor: std::ptr::null(),
        SecurityQualityOfService: std::ptr::null(),
    };
    let mut handle: HANDLE = std::ptr::null_mut();
    let mut io_status = IO_STATUS_BLOCK::default();
    let mut options = FILE_OPEN_REPARSE_POINT | FILE_SYNCHRONOUS_IO_NONALERT;
    if matches!(
        intent,
        WindowsOpenIntent::TraverseDirectory
            | WindowsOpenIntent::MutateDirectoryChildren
            | WindowsOpenIntent::ReopenDirectory
            | WindowsOpenIntent::DeleteDirectory
    ) {
        options |= FILE_DIRECTORY_FILE;
    }
    let desired_access = match intent {
        WindowsOpenIntent::Observe => GENERIC_READ | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
        WindowsOpenIntent::MutateEntry => {
            GENERIC_READ | GENERIC_WRITE | FILE_READ_ATTRIBUTES | SYNCHRONIZE
        }
        WindowsOpenIntent::TraverseDirectory => {
            FILE_TRAVERSE | FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES | SYNCHRONIZE
        }
        WindowsOpenIntent::MutateDirectoryChildren => {
            FILE_TRAVERSE | FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES | READ_CONTROL | SYNCHRONIZE
        }
        WindowsOpenIntent::ReopenDirectory => {
            DELETE
                | FILE_TRAVERSE
                | FILE_LIST_DIRECTORY
                | FILE_READ_ATTRIBUTES
                | READ_CONTROL
                | SYNCHRONIZE
        }
        WindowsOpenIntent::DeleteDirectory => DELETE | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
        WindowsOpenIntent::DeleteEntry => {
            DELETE | GENERIC_READ | FILE_READ_ATTRIBUTES | SYNCHRONIZE
        }
        WindowsOpenIntent::PublishStagedEntry => {
            DELETE
                | GENERIC_READ
                | FILE_READ_ATTRIBUTES
                | READ_CONTROL
                | windows_sys::Win32::Storage::FileSystem::WRITE_DAC
                | SYNCHRONIZE
        }
    };
    let status = unsafe {
        NtCreateFile(
            &raw mut handle,
            desired_access,
            &raw const attributes,
            &raw mut io_status,
            std::ptr::null(),
            FILE_ATTRIBUTE_NORMAL,
            // Share modes enforce retained authority fencing on Windows (unlike Unix).
            // We omit FILE_SHARE_DELETE on most retained handles so rename/delete by
            // other processes (or std::fs) get STATUS_SHARING_VIOLATION until the
            // handle is dropped. We grant it only when the open itself needs DELETE
            // for by-handle cleanup or when concurrent reopens of the same dir are
            // expected inside this process.
            match intent {
                WindowsOpenIntent::Observe => {
                    FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE
                }
                WindowsOpenIntent::MutateEntry
                | WindowsOpenIntent::DeleteEntry
                | WindowsOpenIntent::PublishStagedEntry => FILE_SHARE_READ,
                WindowsOpenIntent::TraverseDirectory => {
                    // Retained handles may carry DELETE for exact cleanup. Grant sharing
                    // so traversal remains possible while a peer holds a narrower handle.
                    FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE
                }
                WindowsOpenIntent::ReopenDirectory => {
                    // Private namespace reopens need DELETE for dispose-by-handle.
                    // Grant sharing because multiple overlapping reopens (e.g. custody
                    // + observe in the same transaction) are common.
                    FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE
                }
                WindowsOpenIntent::MutateDirectoryChildren => FILE_SHARE_READ | FILE_SHARE_WRITE,
                WindowsOpenIntent::DeleteDirectory => {
                    // Delete-by-handle path must allow the subsequent dispose.
                    FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE
                }
            },
            FILE_OPEN,
            options,
            std::ptr::null(),
            0,
        )
    };
    if status < 0 || handle.is_null() {
        return Err(windows_status_error(status, "relative entry open failed"));
    }
    Ok(unsafe { File::from_raw_handle(handle) })
}
