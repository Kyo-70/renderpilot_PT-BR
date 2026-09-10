use crate::ServiceError;
use sha2::Digest;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};

pub(crate) fn hash_file(file: &File) -> Result<String, ServiceError> {
    let mut file = file.try_clone().map_err(|error| {
        crate::failed(format!(
            "failed to duplicate file handle for hashing: {error}"
        ))
    })?;
    file.seek(SeekFrom::Start(0)).map_err(|error| {
        crate::failed(format!("failed to seek retained file for hashing: {error}"))
    })?;
    let mut hasher = sha2::Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| crate::failed(format!("failed to hash retained file: {error}")))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
}

#[expect(unsafe_code, reason = "Windows native file identity observation")]
pub(crate) fn windows_identity(file: &File) -> Result<String, ServiceError> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ID_128, FILE_ID_INFO, FileIdInfo, GetFileInformationByHandleEx,
    };
    let mut info = FILE_ID_INFO {
        VolumeSerialNumber: 0,
        FileId: FILE_ID_128 {
            Identifier: [0; 16],
        },
    };
    let ok = unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle() as _,
            FileIdInfo,
            (&mut info as *mut FILE_ID_INFO).cast(),
            u32::try_from(std::mem::size_of::<FILE_ID_INFO>())
                .map_err(|_| crate::failed("Windows identity buffer is too large"))?,
        )
    } != 0;
    if !ok {
        return Err(crate::failed(format!(
            "failed to inspect retained Windows identity: {}",
            std::io::Error::last_os_error()
        )));
    }
    if info.VolumeSerialNumber == 0 || info.FileId.Identifier.iter().all(|byte| *byte == 0) {
        return Err(crate::failed(
            "Windows filesystem returned an unstable zero file identity",
        ));
    }
    Ok(format!(
        "windows:{:016x}:{}",
        info.VolumeSerialNumber,
        hex::encode(info.FileId.Identifier)
    ))
}

#[expect(unsafe_code, reason = "Windows NTSTATUS conversion")]
pub(crate) fn windows_status_error(status: i32, context: &str) -> std::io::Error {
    use windows_sys::Win32::Foundation::RtlNtStatusToDosError;
    let dos_error = unsafe { RtlNtStatusToDosError(status) };
    if dos_error == 0 {
        std::io::Error::other(format!("{context} with NTSTATUS {status:#010x}"))
    } else {
        std::io::Error::from_raw_os_error(dos_error as i32)
    }
}
