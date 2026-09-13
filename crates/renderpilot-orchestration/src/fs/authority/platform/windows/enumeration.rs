use crate::ServiceError;
use std::ffi::OsString;
use std::fs::File;

use super::identity::windows_status_error;

#[expect(unsafe_code, reason = "Windows retained-handle directory enumeration")]
pub(crate) fn windows_enumerate_directory_names(
    handle: &File,
) -> Result<Vec<OsString>, ServiceError> {
    use std::os::windows::ffi::OsStringExt;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Wdk::Storage::FileSystem::{
        FILE_BOTH_DIR_INFORMATION, FileBothDirectoryInformation, NtQueryDirectoryFile,
    };
    use windows_sys::Win32::Foundation::STATUS_NO_MORE_FILES;
    use windows_sys::Win32::System::IO::IO_STATUS_BLOCK;

    let buffer_words = 64 * 1024usize / std::mem::size_of::<usize>();
    let mut buffer = vec![0_usize; buffer_words];
    let buffer_bytes = u32::try_from(buffer.len() * std::mem::size_of::<usize>())
        .map_err(|_| crate::failed("Windows directory enumeration buffer is too large"))?;
    let buffer_size = buffer.len() * std::mem::size_of::<usize>();
    let mut restart_scan = true;
    let mut names = Vec::new();
    loop {
        let mut io_status = IO_STATUS_BLOCK::default();
        let status = unsafe {
            NtQueryDirectoryFile(
                handle.as_raw_handle().cast(),
                std::ptr::null_mut(),
                None,
                std::ptr::null(),
                &raw mut io_status,
                buffer.as_mut_ptr().cast(),
                buffer_bytes,
                FileBothDirectoryInformation,
                false,
                std::ptr::null(),
                restart_scan,
            )
        };
        if status == STATUS_NO_MORE_FILES {
            break;
        }
        if status < 0 {
            return Err(crate::failed(format!(
                "failed to enumerate retained Windows directory: {}",
                windows_status_error(status, "directory enumeration failed")
            )));
        }
        let returned_bytes = io_status.Information;
        if returned_bytes == 0 {
            break;
        }
        if returned_bytes > buffer_size {
            return Err(crate::failed(
                "Windows directory enumeration returned an oversized buffer length",
            ));
        }
        let mut offset = 0usize;
        loop {
            if offset + std::mem::size_of::<FILE_BOTH_DIR_INFORMATION>() > returned_bytes {
                return Err(crate::failed(
                    "Windows directory enumeration returned an invalid record offset",
                ));
            }
            let record_address = unsafe { buffer.as_ptr().cast::<u8>().add(offset) };
            if !(record_address as usize)
                .is_multiple_of(std::mem::align_of::<FILE_BOTH_DIR_INFORMATION>())
            {
                return Err(crate::failed(
                    "Windows directory enumeration returned an unaligned record offset",
                ));
            }
            #[expect(
                clippy::cast_ptr_alignment,
                reason = "the record address was checked against FILE_BOTH_DIR_INFORMATION alignment"
            )]
            let record = record_address.cast::<FILE_BOTH_DIR_INFORMATION>();
            let record_length = unsafe { (*record).NextEntryOffset as usize };
            let name_length = unsafe { (*record).FileNameLength as usize };
            let name_offset = std::mem::offset_of!(FILE_BOTH_DIR_INFORMATION, FileName);
            let record_end = if record_length == 0 {
                returned_bytes
            } else {
                offset.checked_add(record_length).ok_or_else(|| {
                    crate::failed("Windows directory enumeration record offset overflow")
                })?
            };
            if record_end > returned_bytes
                || name_length % 2 != 0
                || name_offset
                    .checked_add(name_length)
                    .is_none_or(|end| end > record_end.saturating_sub(offset))
            {
                return Err(crate::failed(
                    "Windows directory enumeration returned an invalid name length",
                ));
            }
            let name_ptr = unsafe { std::ptr::addr_of!((*record).FileName).cast::<u16>() };
            let name = unsafe { std::slice::from_raw_parts(name_ptr, name_length / 2) };
            let name = OsString::from_wide(name);
            if name != "." && name != ".." {
                names.push(name);
            }
            if record_length == 0 {
                break;
            }
            let next_offset = offset
                .checked_add(record_length)
                .ok_or_else(|| crate::failed("Windows directory enumeration offset overflow"))?;
            if next_offset >= returned_bytes {
                return Err(crate::failed(
                    "Windows directory enumeration returned an out-of-bounds offset",
                ));
            }
            offset = next_offset;
        }
        restart_scan = false;
    }
    Ok(names)
}
