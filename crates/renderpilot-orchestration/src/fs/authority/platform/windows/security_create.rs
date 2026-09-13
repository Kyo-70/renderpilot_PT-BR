use crate::ServiceError;
use crate::fs::authority::LeafName;
use std::fs::File;

use super::entry_mutation::windows_dispose_by_handle;
use super::identity::windows_identity;

enum WindowsOwnerOnlyDirectoryAttempt {
    Created {
        handle: File,
        identity: String,
    },
    Occupied {
        status: i32,
    },
    Indeterminate {
        error: ServiceError,
        handle: Option<File>,
    },
}

#[expect(unsafe_code, reason = "Windows atomic owner-only namespace creation")]
fn windows_create_owner_only_directory_attempt(
    parent: &File,
    name: &LeafName,
) -> Result<WindowsOwnerOnlyDirectoryAttempt, ServiceError> {
    // The owner-only DACL is passed in the same NtCreateFile call. A
    // post-create ACL repair would expose an insecure window and is not a
    // valid authority implementation.
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::{AsRawHandle, FromRawHandle};
    use windows_sys::Wdk::Foundation::OBJECT_ATTRIBUTES;
    use windows_sys::Wdk::Storage::FileSystem::{
        FILE_CREATE, FILE_DIRECTORY_FILE, FILE_OPEN_REPARSE_POINT, FILE_SYNCHRONOUS_IO_NONALERT,
        NtCreateFile,
    };
    use windows_sys::Win32::Foundation::{
        HANDLE, OBJ_CASE_INSENSITIVE, STATUS_OBJECT_NAME_COLLISION, UNICODE_STRING,
    };
    use windows_sys::Win32::Security::{
        ACCESS_ALLOWED_ACE, ACL, ACL_REVISION, AddAccessAllowedAce, GetLengthSid, InitializeAcl,
        InitializeSecurityDescriptor, MakeSelfRelativeSD, SE_DACL_PROTECTED, SECURITY_DESCRIPTOR,
        SECURITY_DESCRIPTOR_CONTROL, SetSecurityDescriptorControl, SetSecurityDescriptorDacl,
        SetSecurityDescriptorOwner,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        DELETE, FILE_ALL_ACCESS, FILE_ATTRIBUTE_NORMAL, FILE_READ_ATTRIBUTES, FILE_SHARE_READ,
        FILE_SHARE_WRITE, SYNCHRONIZE,
    };
    use windows_sys::Win32::System::IO::IO_STATUS_BLOCK;
    use windows_sys::Win32::System::SystemServices::SECURITY_DESCRIPTOR_REVISION;
    const OBJECT_ATTRIBUTES_BYTES: u32 = std::mem::size_of::<OBJECT_ATTRIBUTES>() as u32;
    let sid = windows_current_user_sid()?;
    let sid_ptr = sid.as_ptr().cast_mut().cast::<core::ffi::c_void>();
    let sid_length = unsafe { GetLengthSid(sid_ptr) };
    if sid_length == 0 {
        return Err(crate::failed("current Windows user SID has no length"));
    }
    let acl_length = std::mem::size_of::<ACL>()
        .checked_add(
            std::mem::size_of::<ACCESS_ALLOWED_ACE>()
                .checked_sub(std::mem::size_of::<u32>())
                .ok_or_else(|| crate::failed("private namespace ACL layout is invalid"))?,
        )
        .and_then(|length| length.checked_add(sid_length as usize))
        .and_then(|length| u32::try_from(length).ok())
        .ok_or_else(|| crate::failed("private namespace ACL is too large"))?;
    let acl_words = (acl_length as usize).div_ceil(std::mem::size_of::<usize>());
    let mut acl_storage = vec![0_usize; acl_words];
    let acl = acl_storage.as_mut_ptr().cast::<ACL>();
    let mut descriptor = SECURITY_DESCRIPTOR::default();
    let descriptor_ptr = std::ptr::addr_of_mut!(descriptor).cast::<core::ffi::c_void>();
    let ok = unsafe { InitializeAcl(acl, acl_length, ACL_REVISION) } != 0
        && unsafe { AddAccessAllowedAce(acl, ACL_REVISION, FILE_ALL_ACCESS, sid_ptr) } != 0
        && unsafe { InitializeSecurityDescriptor(descriptor_ptr, SECURITY_DESCRIPTOR_REVISION) }
            != 0
        && unsafe { SetSecurityDescriptorOwner(descriptor_ptr, sid_ptr, 0) } != 0
        && unsafe { SetSecurityDescriptorDacl(descriptor_ptr, 1, acl, 0) } != 0
        && unsafe {
            SetSecurityDescriptorControl(
                descriptor_ptr,
                SE_DACL_PROTECTED as SECURITY_DESCRIPTOR_CONTROL,
                SE_DACL_PROTECTED as SECURITY_DESCRIPTOR_CONTROL,
            )
        } != 0;
    if !ok {
        return Err(crate::failed(format!(
            "failed to construct private namespace security descriptor: {}",
            std::io::Error::last_os_error()
        )));
    }
    let mut self_relative_length = 0_u32;
    let _ = unsafe {
        MakeSelfRelativeSD(
            descriptor_ptr,
            std::ptr::null_mut(),
            &raw mut self_relative_length,
        )
    };
    if self_relative_length == 0 {
        return Err(crate::failed(format!(
            "failed to size private namespace self-relative security descriptor: {}",
            std::io::Error::last_os_error()
        )));
    }
    let self_relative_words =
        (self_relative_length as usize).div_ceil(std::mem::size_of::<usize>());
    let mut self_relative_storage = vec![0_usize; self_relative_words];
    let self_relative = self_relative_storage
        .as_mut_ptr()
        .cast::<SECURITY_DESCRIPTOR>();
    if unsafe {
        MakeSelfRelativeSD(
            descriptor_ptr,
            self_relative.cast(),
            &raw mut self_relative_length,
        )
    } == 0
    {
        return Err(crate::failed(format!(
            "failed to construct private namespace self-relative security descriptor: {}",
            std::io::Error::last_os_error()
        )));
    }
    let wide = name.as_os_str().encode_wide().collect::<Vec<_>>();
    let byte_length = u16::try_from(
        wide.len()
            .checked_mul(2)
            .ok_or_else(|| crate::failed("private namespace name is too long"))?,
    )
    .map_err(|_| crate::failed("private namespace name is too long"))?;
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
        SecurityDescriptor: self_relative.cast(),
        SecurityQualityOfService: std::ptr::null(),
    };
    let mut handle: HANDLE = std::ptr::null_mut();
    let mut io_status = IO_STATUS_BLOCK::default();
    let status = unsafe {
        NtCreateFile(
            &raw mut handle,
            FILE_ALL_ACCESS | FILE_READ_ATTRIBUTES | DELETE | SYNCHRONIZE,
            &raw const attributes,
            &raw mut io_status,
            std::ptr::null(),
            FILE_ATTRIBUTE_NORMAL,
            // Omit FILE_SHARE_DELETE on purpose. The returned handle is retained
            // (see VerifiedDir / PrivateNamespace) and acts as a fence: external
            // rename/delete (including std::fs) will get sharing violation until
            // this authority is dropped. Reopens of private namespaces use a
            // separate handle that does grant sharing.
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            FILE_CREATE,
            FILE_DIRECTORY_FILE | FILE_OPEN_REPARSE_POINT | FILE_SYNCHRONOUS_IO_NONALERT,
            std::ptr::null(),
            0,
        )
    };
    if status == STATUS_OBJECT_NAME_COLLISION {
        if !handle.is_null() {
            drop(unsafe { File::from_raw_handle(handle) });
        }
        return Ok(WindowsOwnerOnlyDirectoryAttempt::Occupied { status });
    }
    if status < 0 || handle.is_null() {
        let handle = if handle.is_null() {
            None
        } else {
            Some(unsafe { File::from_raw_handle(handle) })
        };
        return Ok(WindowsOwnerOnlyDirectoryAttempt::Indeterminate {
            error: crate::failed(format!(
                "failed to atomically create private namespace: NTSTATUS {status:#010x}"
            )),
            handle,
        });
    }
    let file = unsafe { File::from_raw_handle(handle) };
    let identity = match windows_identity(&file) {
        Ok(identity) => identity,
        Err(error) => {
            return Ok(WindowsOwnerOnlyDirectoryAttempt::Indeterminate {
                error,
                handle: Some(file),
            });
        }
    };
    Ok(WindowsOwnerOnlyDirectoryAttempt::Created {
        handle: file,
        identity,
    })
}

pub(crate) fn windows_create_owner_only_directory(
    parent: &File,
    name: &LeafName,
) -> Result<(File, String), ServiceError> {
    match windows_create_owner_only_directory_attempt(parent, name)? {
        WindowsOwnerOnlyDirectoryAttempt::Created { handle, identity } => Ok((handle, identity)),
        WindowsOwnerOnlyDirectoryAttempt::Occupied { status } => Err(crate::failed(format!(
            "failed to atomically create private namespace: NTSTATUS {status:#010x}"
        ))),
        WindowsOwnerOnlyDirectoryAttempt::Indeterminate { error, handle } => {
            if let Some(file) = handle {
                let cleanup = windows_dispose_by_handle(&file);
                return Err(crate::failed(match cleanup {
                    Ok(()) => error.to_string(),
                    Err(cleanup_error) => {
                        format!("{error}; private namespace cleanup also failed: {cleanup_error}")
                    }
                }));
            }
            Err(error)
        }
    }
}

#[expect(
    unsafe_code,
    reason = "Windows current-user security principal observation"
)]
pub(crate) fn windows_current_user_sid() -> Result<Vec<u8>, ServiceError> {
    use std::os::windows::io::FromRawHandle;
    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::Security::{
        GetLengthSid, GetTokenInformation, TOKEN_QUERY, TOKEN_USER, TokenUser,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    let mut token: HANDLE = std::ptr::null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &raw mut token) } == 0 {
        return Err(crate::failed(format!(
            "failed to open current process token: {}",
            std::io::Error::last_os_error()
        )));
    }
    let _token_guard = unsafe { File::from_raw_handle(token) };
    let mut length = 0_u32;
    let _ =
        unsafe { GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &raw mut length) };
    if length == 0 {
        return Err(crate::failed(format!(
            "failed to size current user token: {}",
            std::io::Error::last_os_error()
        )));
    }
    let buffer_words = (length as usize).div_ceil(std::mem::size_of::<usize>());
    let mut buffer = vec![0_usize; buffer_words];
    if unsafe {
        GetTokenInformation(
            token,
            TokenUser,
            buffer.as_mut_ptr().cast(),
            u32::try_from(buffer.len() * std::mem::size_of::<usize>())
                .map_err(|_| crate::failed("current user token buffer is too large"))?,
            &raw mut length,
        )
    } == 0
    {
        return Err(crate::failed(format!(
            "failed to read current user token: {}",
            std::io::Error::last_os_error()
        )));
    }
    let user = unsafe { &*buffer.as_ptr().cast::<TOKEN_USER>() };
    let sid_length = unsafe { GetLengthSid(user.User.Sid) } as usize;
    if sid_length == 0 {
        return Err(crate::failed("current user token returned an invalid SID"));
    }
    Ok(unsafe { std::slice::from_raw_parts(user.User.Sid.cast::<u8>(), sid_length) }.to_vec())
}
