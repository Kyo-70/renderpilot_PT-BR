use crate::fs::authority::LeafName;
use std::fs::File;

use super::identity::windows_status_error;

/// Closed native classification for a relative no-replace rename.
///
/// `Indeterminate` covers a pending operation, a wait anomaly, or an
/// unfamiliar final status. The public wrapper maps it to an ordinary I/O
/// failure rather than reporting a successful rename.
#[derive(Debug)]
enum WindowsRenameNoReplaceOutcome {
    Moved,
    Occupied,
    Indeterminate(std::io::Error),
}

pub(crate) fn windows_rename_handle_no_replace(
    source: &File,
    destination_parent: &File,
    destination: &LeafName,
) -> std::io::Result<()> {
    match windows_rename_handle_no_replace_outcome(source, destination_parent, destination) {
        WindowsRenameNoReplaceOutcome::Moved => Ok(()),
        WindowsRenameNoReplaceOutcome::Occupied => Err(std::io::Error::from_raw_os_error(
            windows_sys::Win32::Foundation::ERROR_FILE_EXISTS as i32,
        )),
        WindowsRenameNoReplaceOutcome::Indeterminate(error) => Err(error),
    }
}

#[expect(unsafe_code, reason = "Windows retained-handle no-replace rename")]
fn windows_rename_handle_no_replace_outcome(
    source: &File,
    destination_parent: &File,
    destination: &LeafName,
) -> WindowsRenameNoReplaceOutcome {
    use std::mem::MaybeUninit;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Wdk::Storage::FileSystem::{
        FILE_RENAME_INFORMATION, FileRenameInformation, NtSetInformationFile,
    };
    use windows_sys::Win32::{
        Foundation::{STATUS_PENDING, WAIT_FAILED, WAIT_OBJECT_0},
        System::{
            IO::IO_STATUS_BLOCK,
            Threading::{INFINITE, WaitForSingleObject},
        },
    };
    let wide = destination.as_os_str().encode_wide().collect::<Vec<_>>();
    let Some(name_bytes) = wide
        .len()
        .checked_mul(2)
        .and_then(|length| u32::try_from(length).ok())
    else {
        return WindowsRenameNoReplaceOutcome::Indeterminate(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "destination name is too long",
        ));
    };
    let Some(required_bytes) = std::mem::size_of::<FILE_RENAME_INFORMATION>()
        .checked_add(name_bytes as usize)
        .and_then(|length| u32::try_from(length).ok())
    else {
        return WindowsRenameNoReplaceOutcome::Indeterminate(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "rename buffer is too large",
        ));
    };
    let slots = (required_bytes as usize).div_ceil(std::mem::size_of::<FILE_RENAME_INFORMATION>());
    let mut storage = Vec::with_capacity(slots);
    storage.resize_with(slots, MaybeUninit::<FILE_RENAME_INFORMATION>::zeroed);
    let rename = storage.as_mut_ptr().cast::<FILE_RENAME_INFORMATION>();
    unsafe {
        (*rename).Anonymous.ReplaceIfExists = false;
        (*rename).RootDirectory = destination_parent.as_raw_handle();
        (*rename).FileNameLength = name_bytes;
        std::ptr::copy_nonoverlapping(
            wide.as_ptr(),
            std::ptr::addr_of_mut!((*rename).FileName).cast::<u16>(),
            wide.len(),
        );
        let mut io_status = IO_STATUS_BLOCK::default();
        let status = NtSetInformationFile(
            source.as_raw_handle(),
            &mut io_status,
            rename.cast(),
            required_bytes,
            FileRenameInformation,
        );
        if status == STATUS_PENDING {
            match WaitForSingleObject(source.as_raw_handle(), INFINITE) {
                WAIT_OBJECT_0 => {}
                WAIT_FAILED => {
                    return WindowsRenameNoReplaceOutcome::Indeterminate(
                        std::io::Error::last_os_error(),
                    );
                }
                result => {
                    return WindowsRenameNoReplaceOutcome::Indeterminate(std::io::Error::other(
                        format!("unexpected rename wait result {result:#010x}"),
                    ));
                }
            }
        }
        let final_status = match status {
            0 | STATUS_PENDING => io_status.Anonymous.Status,
            _ => {
                let error = windows_status_error(
                    status,
                    "relative no-replace rename returned an unexpected status",
                );
                if matches!(
                    error
                        .raw_os_error()
                        .and_then(|code| u32::try_from(code).ok()),
                    Some(
                        windows_sys::Win32::Foundation::ERROR_FILE_EXISTS
                            | windows_sys::Win32::Foundation::ERROR_ALREADY_EXISTS,
                    )
                ) {
                    return WindowsRenameNoReplaceOutcome::Occupied;
                }
                return WindowsRenameNoReplaceOutcome::Indeterminate(error);
            }
        };
        if final_status == 0 {
            return WindowsRenameNoReplaceOutcome::Moved;
        }
        let error = windows_status_error(final_status, "relative no-replace rename failed");
        if matches!(
            error
                .raw_os_error()
                .and_then(|code| u32::try_from(code).ok()),
            Some(
                windows_sys::Win32::Foundation::ERROR_FILE_EXISTS
                    | windows_sys::Win32::Foundation::ERROR_ALREADY_EXISTS,
            )
        ) {
            return WindowsRenameNoReplaceOutcome::Occupied;
        }
        WindowsRenameNoReplaceOutcome::Indeterminate(error)
    }
}

#[expect(unsafe_code, reason = "Windows by-handle disposition")]
pub(crate) fn windows_dispose_by_handle(file: &File) -> std::io::Result<()> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_DISPOSITION_INFO, FileDispositionInfo, SetFileInformationByHandle,
    };
    let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
    let ok = unsafe {
        SetFileInformationByHandle(
            file.as_raw_handle() as _,
            FileDispositionInfo,
            (&raw const disposition).cast(),
            u32::try_from(std::mem::size_of::<FILE_DISPOSITION_INFO>()).map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "FILE_DISPOSITION_INFO does not fit in a Win32 buffer",
                )
            })?,
        )
    } != 0;
    if ok {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}
