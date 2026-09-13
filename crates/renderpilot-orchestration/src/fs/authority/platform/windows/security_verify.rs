use crate::ServiceError;
use std::fs::File;

use super::security_create::windows_current_user_sid;

#[expect(
    unsafe_code,
    reason = "Windows owner-only namespace security verification"
)]
pub(crate) fn windows_verify_private_security(handle: &File) -> Result<(), ServiceError> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Security::{
        ACCESS_ALLOWED_ACE, ACL_SIZE_INFORMATION, AclSizeInformation, DACL_SECURITY_INFORMATION,
        GetAce, GetAclInformation, GetKernelObjectSecurity, GetSecurityDescriptorControl,
        GetSecurityDescriptorDacl, GetSecurityDescriptorOwner, OWNER_SECURITY_INFORMATION,
        SE_DACL_PROTECTED,
    };
    use windows_sys::Win32::Storage::FileSystem::FILE_ALL_ACCESS;
    let mut needed = 0_u32;
    let _ = unsafe {
        GetKernelObjectSecurity(
            handle.as_raw_handle().cast(),
            DACL_SECURITY_INFORMATION | OWNER_SECURITY_INFORMATION,
            std::ptr::null_mut(),
            0,
            &raw mut needed,
        )
    };
    if needed == 0 {
        return Err(crate::failed(format!(
            "failed to size private namespace security descriptor: {}",
            std::io::Error::last_os_error()
        )));
    }
    let buffer_words = (needed as usize).div_ceil(std::mem::size_of::<usize>());
    let mut buffer = vec![0_usize; buffer_words];
    if unsafe {
        GetKernelObjectSecurity(
            handle.as_raw_handle().cast(),
            DACL_SECURITY_INFORMATION | OWNER_SECURITY_INFORMATION,
            buffer.as_mut_ptr().cast(),
            u32::try_from(buffer.len() * std::mem::size_of::<usize>())
                .map_err(|_| crate::failed("private namespace security buffer is too large"))?,
            &raw mut needed,
        )
    } == 0
    {
        return Err(crate::failed(format!(
            "failed to read private namespace security descriptor: {}",
            std::io::Error::last_os_error()
        )));
    }
    let descriptor = buffer
        .as_mut_ptr()
        .cast::<windows_sys::Win32::Security::SECURITY_DESCRIPTOR>()
        .cast::<core::ffi::c_void>();
    let mut owner = std::ptr::null_mut();
    let mut owner_defaulted = 0;
    let mut dacl_present = 0;
    let mut dacl = std::ptr::null_mut();
    let mut dacl_defaulted = 0;
    if unsafe { GetSecurityDescriptorOwner(descriptor, &raw mut owner, &raw mut owner_defaulted) }
        == 0
        || unsafe {
            GetSecurityDescriptorDacl(
                descriptor,
                &raw mut dacl_present,
                &raw mut dacl,
                &raw mut dacl_defaulted,
            )
        } == 0
        || owner.is_null()
        || dacl_present == 0
        || dacl.is_null()
    {
        return Err(crate::failed(
            "private namespace does not have an explicit owner-only DACL",
        ));
    }
    let mut control = 0;
    let mut revision = 0_u32;
    if unsafe { GetSecurityDescriptorControl(descriptor, &raw mut control, &raw mut revision) } == 0
        || control & SE_DACL_PROTECTED == 0
    {
        return Err(crate::failed(
            "private namespace DACL is not protected from inheritance",
        ));
    }
    let current_sid = windows_current_user_sid()?;
    let current_sid_ptr = current_sid.as_ptr().cast_mut().cast::<core::ffi::c_void>();
    if unsafe { windows_sys::Win32::Security::EqualSid(owner, current_sid_ptr) } == 0 {
        return Err(crate::failed(
            "private namespace owner is not the current user",
        ));
    }
    let mut acl_info = ACL_SIZE_INFORMATION::default();
    if unsafe {
        GetAclInformation(
            dacl,
            (&raw mut acl_info).cast(),
            u32::try_from(std::mem::size_of::<ACL_SIZE_INFORMATION>())
                .map_err(|_| crate::failed("Windows ACL information buffer is too large"))?,
            AclSizeInformation,
        )
    } == 0
        || acl_info.AceCount != 1
    {
        return Err(crate::failed(
            "private namespace DACL is not exactly one owner ACE",
        ));
    }
    let mut ace = std::ptr::null_mut();
    if unsafe { GetAce(dacl, 0, &raw mut ace) } == 0 || ace.is_null() {
        return Err(crate::failed("private namespace owner ACE is unreadable"));
    }
    let ace = unsafe { &*ace.cast::<ACCESS_ALLOWED_ACE>() };
    if ace.Header.AceType != 0 || ace.Header.AceFlags != 0 || ace.Mask != FILE_ALL_ACCESS {
        return Err(crate::failed(
            "private namespace DACL owner ACE is not full-control-only",
        ));
    }
    let ace_sid = std::ptr::addr_of!(ace.SidStart)
        .cast_mut()
        .cast::<core::ffi::c_void>();
    if unsafe { windows_sys::Win32::Security::EqualSid(ace_sid, current_sid_ptr) } == 0 {
        return Err(crate::failed(
            "private namespace DACL ACE is not for the current user",
        ));
    }
    Ok(())
}
