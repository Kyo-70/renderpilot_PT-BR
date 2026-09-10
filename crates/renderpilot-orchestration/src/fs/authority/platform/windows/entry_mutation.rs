use std::fs::File;

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
