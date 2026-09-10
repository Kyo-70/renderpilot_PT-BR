use crate::fs::authority::LeafName;
use std::fs::File;

use super::identity::windows_status_error;

/// Closed result of a relative `FILE_CREATE` operation.
///
/// A non-null returned handle is retained while the native result is
/// classified, so the wrapper can deterministically map it to success or an
/// ordinary I/O failure without synthesizing a successful create.
#[derive(Debug)]
enum WindowsCreateFileRelativeOutcome {
    Created(File),
    Occupied,
    NotCreated(std::io::Error),
    RetainedIndeterminate {
        // The handle must remain owned until this outcome is dropped: a
        // non-success NT result can still have returned a live handle.
        _file: File,
        error: std::io::Error,
    },
}

pub(crate) fn windows_create_file_relative(
    parent: &File,
    name: &LeafName,
) -> std::io::Result<File> {
    match windows_create_file_relative_outcome(parent, name) {
        WindowsCreateFileRelativeOutcome::Created(file) => Ok(file),
        WindowsCreateFileRelativeOutcome::Occupied => Err(std::io::Error::from_raw_os_error(
            windows_sys::Win32::Foundation::ERROR_FILE_EXISTS as i32,
        )),
        WindowsCreateFileRelativeOutcome::NotCreated(error)
        | WindowsCreateFileRelativeOutcome::RetainedIndeterminate { error, .. } => Err(error),
    }
}

#[expect(
    unsafe_code,
    reason = "Windows retained-handle exclusive file creation"
)]
fn windows_create_file_relative_outcome(
    parent: &File,
    name: &LeafName,
) -> WindowsCreateFileRelativeOutcome {
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::{AsRawHandle, FromRawHandle};
    use windows_sys::Wdk::Foundation::OBJECT_ATTRIBUTES;
    use windows_sys::Wdk::Storage::FileSystem::{
        FILE_CREATE, FILE_NON_DIRECTORY_FILE, FILE_OPEN_REPARSE_POINT,
        FILE_SYNCHRONOUS_IO_NONALERT, NtCreateFile,
    };
    use windows_sys::Win32::Foundation::{
        GENERIC_READ, GENERIC_WRITE, HANDLE, OBJ_CASE_INSENSITIVE, UNICODE_STRING,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        DELETE, FILE_ATTRIBUTE_NORMAL, FILE_READ_ATTRIBUTES, FILE_SHARE_READ, SYNCHRONIZE,
    };
    use windows_sys::Win32::System::IO::IO_STATUS_BLOCK;

    const OBJECT_ATTRIBUTES_BYTES: u32 = std::mem::size_of::<OBJECT_ATTRIBUTES>() as u32;
    let wide = name.as_os_str().encode_wide().collect::<Vec<_>>();
    let Some(byte_length) = wide
        .len()
        .checked_mul(2)
        .and_then(|length| u16::try_from(length).ok())
    else {
        return WindowsCreateFileRelativeOutcome::NotCreated(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "entry name is too long",
        ));
    };
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
    let status = unsafe {
        NtCreateFile(
            &raw mut handle,
            DELETE | GENERIC_READ | GENERIC_WRITE | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
            &raw const attributes,
            &raw mut io_status,
            std::ptr::null(),
            FILE_ATTRIBUTE_NORMAL,
            // Exclusive no-replace file create: only allow readers while we hold
            // the handle. Rename/delete of the target are intentionally blocked.
            FILE_SHARE_READ,
            FILE_CREATE,
            FILE_NON_DIRECTORY_FILE | FILE_OPEN_REPARSE_POINT | FILE_SYNCHRONOUS_IO_NONALERT,
            std::ptr::null(),
            0,
        )
    };
    if handle.is_null() {
        let error = windows_status_error(status, "relative exclusive file create failed");
        if windows_no_replace_occupied(&error) {
            return WindowsCreateFileRelativeOutcome::Occupied;
        }
        return WindowsCreateFileRelativeOutcome::NotCreated(error);
    }
    let file = unsafe { File::from_raw_handle(handle) };
    let final_status = unsafe { io_status.Anonymous.Status };
    if status == 0 && final_status == 0 {
        return WindowsCreateFileRelativeOutcome::Created(file);
    }
    let status = if status == 0 { final_status } else { status };
    WindowsCreateFileRelativeOutcome::RetainedIndeterminate {
        _file: file,
        error: windows_status_error(
            status,
            "relative exclusive file create has indeterminate result",
        ),
    }
}

fn windows_no_replace_occupied(error: &std::io::Error) -> bool {
    matches!(
        error
            .raw_os_error()
            .and_then(|code| u32::try_from(code).ok()),
        Some(
            windows_sys::Win32::Foundation::ERROR_FILE_EXISTS
                | windows_sys::Win32::Foundation::ERROR_ALREADY_EXISTS,
        )
    )
}
