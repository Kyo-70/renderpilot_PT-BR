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
        DACL_SECURITY_INFORMATION, GetKernelObjectSecurity, OWNER_SECURITY_INFORMATION,
    };
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
    let buffer_bytes = needed;
    let buffer_words = (buffer_bytes as usize).div_ceil(std::mem::size_of::<usize>());
    let mut buffer = vec![0_usize; buffer_words];
    if unsafe {
        GetKernelObjectSecurity(
            handle.as_raw_handle().cast(),
            DACL_SECURITY_INFORMATION | OWNER_SECURITY_INFORMATION,
            buffer.as_mut_ptr().cast(),
            buffer_bytes,
            &raw mut needed,
        )
    } == 0
    {
        return Err(crate::failed(format!(
            "failed to read private namespace security descriptor: {}",
            std::io::Error::last_os_error()
        )));
    }
    let descriptor = buffer.as_mut_ptr().cast::<core::ffi::c_void>();
    let current_sid = windows_current_user_sid()?;
    // SAFETY: `descriptor` and `current_sid` were produced by the validated
    // Win32 paths above and remain alive for the duration of the call.
    unsafe { windows_verify_private_security_descriptor(descriptor, &current_sid) }
}

/// Verifies that the provided security descriptor enforces a private, owner-only DACL.
///
/// # Safety
///
/// - `descriptor` must point to a live, correctly initialized Windows `SECURITY_DESCRIPTOR`
///   for the entire duration of the call.
/// - Any nested owner and DACL pointers obtained from `descriptor` via Win32 query APIs must
///   remain valid and accessible for the duration of the call.
/// - `current_sid` must contain a fully accessible, valid Windows SID for the duration of the call.
#[expect(
    unsafe_code,
    reason = "Windows owner-only namespace security descriptor verification"
)]
unsafe fn windows_verify_private_security_descriptor(
    descriptor: *mut core::ffi::c_void,
    current_sid: &[u8],
) -> Result<(), ServiceError> {
    use windows_sys::Win32::Security::{
        ACCESS_ALLOWED_ACE, ACE_HEADER, ACL, ACL_SIZE_INFORMATION, AclSizeInformation, EqualSid,
        GetAce, GetAclInformation, GetSecurityDescriptorControl, GetSecurityDescriptorDacl,
        GetSecurityDescriptorOwner, IsValidSid, SE_DACL_PROTECTED, SID,
    };
    use windows_sys::Win32::Storage::FileSystem::FILE_ALL_ACCESS;
    use windows_sys::Win32::System::SystemServices::{
        ACCESS_ALLOWED_ACE_TYPE, SID_MAX_SUB_AUTHORITIES,
    };

    const ACL_INFO_SIZE: u32 = std::mem::size_of::<ACL_SIZE_INFORMATION>() as u32;
    const MASK_OFFSET: usize = std::mem::offset_of!(ACCESS_ALLOWED_ACE, Mask);
    const SID_START_OFFSET: usize = std::mem::offset_of!(ACCESS_ALLOWED_ACE, SidStart);
    const SID_HEADER_SIZE: usize = std::mem::offset_of!(SID, SubAuthority);
    const MIN_FIXED_ACE_SIZE: usize = SID_START_OFFSET + SID_HEADER_SIZE;
    const SUB_AUTHORITY_COUNT_OFFSET: usize = std::mem::offset_of!(SID, SubAuthorityCount);

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
    let current_sid_ptr = current_sid.as_ptr().cast_mut().cast::<core::ffi::c_void>();
    if unsafe { EqualSid(owner, current_sid_ptr) } == 0 {
        return Err(crate::failed(
            "private namespace owner is not the current user",
        ));
    }
    let mut acl_info = ACL_SIZE_INFORMATION::default();
    if unsafe {
        GetAclInformation(
            dacl,
            (&raw mut acl_info).cast(),
            ACL_INFO_SIZE,
            AclSizeInformation,
        )
    } == 0
        || acl_info.AceCount != 1
    {
        return Err(crate::failed(
            "private namespace DACL is not exactly one owner ACE",
        ));
    }

    // Win32 guarantees the descriptor/DACL structure; below we validate the ACE's
    // variable-length layout before reading its contents.
    let acl_size = usize::from(unsafe { (*dacl).AclSize });
    let bytes_in_use = usize::try_from(acl_info.AclBytesInUse)
        .map_err(|_| crate::failed("private namespace DACL used bytes overflow"))?;
    let acl_header_size = std::mem::size_of::<ACL>();
    if acl_size < acl_header_size || bytes_in_use < acl_header_size || bytes_in_use > acl_size {
        return Err(crate::failed(
            "private namespace DACL has an invalid used-byte range",
        ));
    }
    let dacl_start = dacl.addr();
    let dacl_end = dacl_start
        .checked_add(bytes_in_use)
        .ok_or_else(|| crate::failed("private namespace DACL range overflow"))?;
    let expected_first_ace_start = dacl_start
        .checked_add(acl_header_size)
        .ok_or_else(|| crate::failed("private namespace DACL first ACE range overflow"))?;

    let mut ace = std::ptr::null_mut();
    if unsafe { GetAce(dacl, 0, &raw mut ace) } == 0 || ace.is_null() {
        return Err(crate::failed("private namespace owner ACE is unreadable"));
    }

    let ace_start = ace.addr();
    let ace_header_size = std::mem::size_of::<ACE_HEADER>();
    let header_end = ace_start
        .checked_add(ace_header_size)
        .ok_or_else(|| crate::failed("private namespace DACL ACE header range overflow"))?;
    if ace_start != expected_first_ace_start || header_end > dacl_end {
        return Err(crate::failed(
            "private namespace DACL ACE header lies outside DACL used range",
        ));
    }
    // Read by value so no Rust reference is created before the ACE layout is validated.
    let header = unsafe { ace.cast::<ACE_HEADER>().read_unaligned() };
    if u32::from(header.AceType) != ACCESS_ALLOWED_ACE_TYPE || header.AceFlags != 0 {
        return Err(crate::failed(
            "private namespace DACL owner ACE is not an access-allowed ACE without inheritance flags",
        ));
    }

    let ace_size = usize::from(header.AceSize);
    let ace_end = ace_start
        .checked_add(ace_size)
        .ok_or_else(|| crate::failed("private namespace DACL ACE range overflow"))?;
    if ace_size < MIN_FIXED_ACE_SIZE || ace_end > dacl_end {
        return Err(crate::failed(
            "private namespace DACL ACE size is invalid for an access-allowed ACE",
        ));
    }

    let mask = unsafe {
        ace.cast::<u8>()
            .add(MASK_OFFSET)
            .cast::<u32>()
            .read_unaligned()
    };
    if mask != FILE_ALL_ACCESS {
        return Err(crate::failed(
            "private namespace DACL owner ACE is not full-control-only",
        ));
    }

    let candidate_sid_ptr = unsafe { ace.cast::<u8>().add(SID_START_OFFSET) };
    let sub_authority_count = unsafe { candidate_sid_ptr.add(SUB_AUTHORITY_COUNT_OFFSET).read() };
    if u32::from(sub_authority_count) > SID_MAX_SUB_AUTHORITIES {
        return Err(crate::failed(
            "private namespace DACL ACE candidate SID has too many subauthorities",
        ));
    }
    let candidate_sid_length =
        SID_HEADER_SIZE + usize::from(sub_authority_count) * std::mem::size_of::<u32>();
    let required_ace_size = SID_START_OFFSET + candidate_sid_length;
    if ace_size != required_ace_size {
        return Err(crate::failed(
            "private namespace DACL ACE size does not match its candidate SID",
        ));
    }
    if ace_end != dacl_end {
        return Err(crate::failed(
            "private namespace DACL used bytes do not exactly match its single owner ACE",
        ));
    }

    let candidate_sid_void = candidate_sid_ptr.cast::<core::ffi::c_void>();
    if unsafe { IsValidSid(candidate_sid_void) } == 0 {
        return Err(crate::failed(
            "private namespace DACL ACE contains an invalid candidate SID",
        ));
    }
    if unsafe { EqualSid(candidate_sid_void, current_sid_ptr) } == 0 {
        return Err(crate::failed(
            "private namespace DACL ACE is not for the current user",
        ));
    }
    Ok(())
}

#[cfg(test)]
#[expect(unsafe_code, reason = "Windows security descriptor unit test fixtures")]
mod tests {
    use super::*;
    use windows_sys::Win32::Security::{
        ACCESS_ALLOWED_ACE, ACE_HEADER, ACL, ACL_REVISION, AddAccessAllowedAce,
        AddAccessAllowedAceEx, CreateWellKnownSid, GetAce, GetLengthSid, INHERITED_ACE,
        InitializeAcl, InitializeSecurityDescriptor, SE_DACL_PROTECTED, SECURITY_DESCRIPTOR,
        SECURITY_DESCRIPTOR_CONTROL, SECURITY_MAX_SID_SIZE, SID, SetSecurityDescriptorControl,
        SetSecurityDescriptorDacl, SetSecurityDescriptorOwner, WinWorldSid,
    };
    use windows_sys::Win32::Storage::FileSystem::{FILE_ALL_ACCESS, FILE_GENERIC_READ};
    use windows_sys::Win32::System::SystemServices::{
        ACCESS_DENIED_ACE_TYPE, SECURITY_DESCRIPTOR_REVISION,
    };

    fn make_test_dacl(
        configure: impl FnOnce(*mut ACL, *mut core::ffi::c_void),
    ) -> (Vec<usize>, Vec<u8>) {
        let sid = windows_current_user_sid().expect("current user SID");
        let sid_ptr = sid.as_ptr().cast_mut().cast::<core::ffi::c_void>();
        let sid_length = unsafe { GetLengthSid(sid_ptr) };
        assert_ne!(sid_length, 0);
        let ace_length = std::mem::offset_of!(ACCESS_ALLOWED_ACE, SidStart) + sid_length as usize;
        let acl_length = std::mem::size_of::<ACL>() + ace_length + 64; // declared allocation with slack
        let mut storage = vec![0_usize; acl_length.div_ceil(std::mem::size_of::<usize>())];
        let acl = storage.as_mut_ptr().cast::<ACL>();
        assert_ne!(
            unsafe {
                InitializeAcl(
                    acl,
                    u32::try_from(acl_length).expect("test ACL length fits in u32"),
                    ACL_REVISION,
                )
            },
            0
        );
        configure(acl, sid_ptr);
        (storage, sid)
    }

    fn make_test_descriptor(
        dacl_storage: &mut [usize],
        owner_sid: *mut core::ffi::c_void,
        is_protected: bool,
    ) -> SECURITY_DESCRIPTOR {
        let mut descriptor = SECURITY_DESCRIPTOR::default();
        let desc_ptr = (&raw mut descriptor).cast::<core::ffi::c_void>();
        unsafe {
            assert_ne!(
                InitializeSecurityDescriptor(desc_ptr, SECURITY_DESCRIPTOR_REVISION),
                0
            );
            assert_ne!(SetSecurityDescriptorOwner(desc_ptr, owner_sid, 0), 0);
            assert_ne!(
                SetSecurityDescriptorDacl(desc_ptr, 1, dacl_storage.as_mut_ptr().cast(), 0),
                0
            );
            if is_protected {
                assert_ne!(
                    SetSecurityDescriptorControl(
                        desc_ptr,
                        SE_DACL_PROTECTED as SECURITY_DESCRIPTOR_CONTROL,
                        SE_DACL_PROTECTED as SECURITY_DESCRIPTOR_CONTROL,
                    ),
                    0
                );
            }
        };
        descriptor
    }

    #[test]
    fn accepts_canonical_owner_only_security_descriptor() {
        let (mut dacl_storage, sid) = make_test_dacl(|acl, sid_ptr| {
            assert_ne!(
                unsafe { AddAccessAllowedAce(acl, ACL_REVISION, FILE_ALL_ACCESS, sid_ptr) },
                0
            );
        });
        let mut descriptor =
            make_test_descriptor(&mut dacl_storage, sid.as_ptr().cast_mut().cast(), true);
        let desc_ptr = (&raw mut descriptor).cast::<core::ffi::c_void>();
        assert!(unsafe { windows_verify_private_security_descriptor(desc_ptr, &sid) }.is_ok());
    }

    #[test]
    fn rejects_unprotected_dacl() {
        let (mut dacl_storage, sid) = make_test_dacl(|acl, sid_ptr| {
            assert_ne!(
                unsafe { AddAccessAllowedAce(acl, ACL_REVISION, FILE_ALL_ACCESS, sid_ptr) },
                0
            );
        });
        let mut descriptor = make_test_descriptor(
            &mut dacl_storage,
            sid.as_ptr().cast_mut().cast(),
            false, // not protected from inheritance
        );
        let desc_ptr = (&raw mut descriptor).cast::<core::ffi::c_void>();
        let err =
            unsafe { windows_verify_private_security_descriptor(desc_ptr, &sid) }.unwrap_err();
        assert!(err.to_string().contains("not protected from inheritance"));
    }

    #[test]
    fn rejects_foreign_owner() {
        let (mut dacl_storage, sid) = make_test_dacl(|acl, sid_ptr| {
            assert_ne!(
                unsafe { AddAccessAllowedAce(acl, ACL_REVISION, FILE_ALL_ACCESS, sid_ptr) },
                0
            );
        });
        // Construct world SID S-1-1-0 as foreign owner via standard Win32 API.
        let mut world_sid = vec![0_u8; SECURITY_MAX_SID_SIZE as usize];
        let mut sid_size = world_sid.len() as u32;
        assert_ne!(
            unsafe {
                CreateWellKnownSid(
                    WinWorldSid,
                    std::ptr::null_mut(),
                    world_sid.as_mut_ptr().cast(),
                    &raw mut sid_size,
                )
            },
            0
        );
        let mut descriptor =
            make_test_descriptor(&mut dacl_storage, world_sid.as_mut_ptr().cast(), true);
        let desc_ptr = (&raw mut descriptor).cast::<core::ffi::c_void>();
        let err =
            unsafe { windows_verify_private_security_descriptor(desc_ptr, &sid) }.unwrap_err();
        assert!(err.to_string().contains("owner is not the current user"));
    }

    #[test]
    fn rejects_dacl_with_zero_or_multiple_aces() {
        let (mut dacl_storage, sid) = make_test_dacl(|_acl, _sid_ptr| {});
        let mut descriptor =
            make_test_descriptor(&mut dacl_storage, sid.as_ptr().cast_mut().cast(), true);
        let desc_ptr = (&raw mut descriptor).cast::<core::ffi::c_void>();
        let err =
            unsafe { windows_verify_private_security_descriptor(desc_ptr, &sid) }.unwrap_err();
        assert!(err.to_string().contains("not exactly one owner ACE"));

        let (mut dacl_storage, sid) = make_test_dacl(|acl, sid_ptr| {
            assert_ne!(
                unsafe { AddAccessAllowedAce(acl, ACL_REVISION, FILE_ALL_ACCESS, sid_ptr) },
                0
            );
            assert_ne!(
                unsafe { AddAccessAllowedAce(acl, ACL_REVISION, FILE_ALL_ACCESS, sid_ptr) },
                0
            );
        });
        let mut descriptor =
            make_test_descriptor(&mut dacl_storage, sid.as_ptr().cast_mut().cast(), true);
        let desc_ptr = (&raw mut descriptor).cast::<core::ffi::c_void>();
        let err =
            unsafe { windows_verify_private_security_descriptor(desc_ptr, &sid) }.unwrap_err();
        assert!(err.to_string().contains("not exactly one owner ACE"));
    }

    #[test]
    fn rejects_inherited_ace_flags() {
        let (mut dacl_storage, sid) = make_test_dacl(|acl, sid_ptr| {
            assert_ne!(
                unsafe {
                    AddAccessAllowedAceEx(
                        acl,
                        ACL_REVISION,
                        INHERITED_ACE,
                        FILE_ALL_ACCESS,
                        sid_ptr,
                    )
                },
                0
            );
        });
        let mut descriptor =
            make_test_descriptor(&mut dacl_storage, sid.as_ptr().cast_mut().cast(), true);
        let desc_ptr = (&raw mut descriptor).cast::<core::ffi::c_void>();
        let err =
            unsafe { windows_verify_private_security_descriptor(desc_ptr, &sid) }.unwrap_err();
        assert!(err.to_string().contains("without inheritance flags"));
    }

    #[test]
    fn rejects_non_full_control_mask() {
        let (mut dacl_storage, sid) = make_test_dacl(|acl, sid_ptr| {
            assert_ne!(
                unsafe { AddAccessAllowedAce(acl, ACL_REVISION, FILE_GENERIC_READ, sid_ptr) },
                0
            );
        });
        let mut descriptor =
            make_test_descriptor(&mut dacl_storage, sid.as_ptr().cast_mut().cast(), true);
        let desc_ptr = (&raw mut descriptor).cast::<core::ffi::c_void>();
        let err =
            unsafe { windows_verify_private_security_descriptor(desc_ptr, &sid) }.unwrap_err();
        assert!(err.to_string().contains("not full-control-only"));
    }

    #[test]
    fn rejects_tampered_ace_size_and_slack() {
        // ACE size artificially enlarged with trailing padding.
        let (mut dacl_storage, sid) = make_test_dacl(|acl, sid_ptr| {
            assert_ne!(
                unsafe { AddAccessAllowedAce(acl, ACL_REVISION, FILE_ALL_ACCESS, sid_ptr) },
                0
            );
            let mut ace = std::ptr::null_mut();
            assert_ne!(unsafe { GetAce(acl, 0, &raw mut ace) }, 0);
            assert!(!ace.is_null());
            let header_ptr = ace.cast::<ACE_HEADER>();
            let mut header = unsafe { header_ptr.read_unaligned() };
            header.AceSize += 4; // Add 4 trailing bytes
            unsafe { header_ptr.write_unaligned(header) };
        });
        let mut descriptor =
            make_test_descriptor(&mut dacl_storage, sid.as_ptr().cast_mut().cast(), true);
        let desc_ptr = (&raw mut descriptor).cast::<core::ffi::c_void>();
        let err =
            unsafe { windows_verify_private_security_descriptor(desc_ptr, &sid) }.unwrap_err();
        assert!(err.to_string().contains("does not match its candidate SID"));

        // ACE size smaller than minimum fixed ACE size.
        let (mut dacl_storage, sid) = make_test_dacl(|acl, sid_ptr| {
            assert_ne!(
                unsafe { AddAccessAllowedAce(acl, ACL_REVISION, FILE_ALL_ACCESS, sid_ptr) },
                0
            );
            let mut ace = std::ptr::null_mut();
            assert_ne!(unsafe { GetAce(acl, 0, &raw mut ace) }, 0);
            assert!(!ace.is_null());
            let header_ptr = ace.cast::<ACE_HEADER>();
            let mut header = unsafe { header_ptr.read_unaligned() };
            header.AceSize = 8; // less than MIN_FIXED_ACE_SIZE (16)
            unsafe { header_ptr.write_unaligned(header) };
        });
        let mut descriptor =
            make_test_descriptor(&mut dacl_storage, sid.as_ptr().cast_mut().cast(), true);
        let desc_ptr = (&raw mut descriptor).cast::<core::ffi::c_void>();
        let err =
            unsafe { windows_verify_private_security_descriptor(desc_ptr, &sid) }.unwrap_err();
        assert!(
            err.to_string()
                .contains("size is invalid for an access-allowed ACE")
        );
    }

    #[test]
    fn rejects_tampered_sub_authority_count() {
        let (mut dacl_storage, sid) = make_test_dacl(|acl, sid_ptr| {
            assert_ne!(
                unsafe { AddAccessAllowedAce(acl, ACL_REVISION, FILE_ALL_ACCESS, sid_ptr) },
                0
            );
            let mut ace = std::ptr::null_mut();
            assert_ne!(unsafe { GetAce(acl, 0, &raw mut ace) }, 0);
            assert!(!ace.is_null());
            let sid_ptr = unsafe {
                ace.cast::<u8>()
                    .add(std::mem::offset_of!(ACCESS_ALLOWED_ACE, SidStart))
            };
            let count_ptr = unsafe { sid_ptr.add(std::mem::offset_of!(SID, SubAuthorityCount)) };
            unsafe { count_ptr.write_unaligned(16) }; // > SID_MAX_SUB_AUTHORITIES (15)
        });
        let mut descriptor =
            make_test_descriptor(&mut dacl_storage, sid.as_ptr().cast_mut().cast(), true);
        let desc_ptr = (&raw mut descriptor).cast::<core::ffi::c_void>();
        let err =
            unsafe { windows_verify_private_security_descriptor(desc_ptr, &sid) }.unwrap_err();
        assert!(err.to_string().contains("too many subauthorities"));
    }

    #[test]
    fn rejects_wrong_ace_type() {
        let (mut dacl_storage, sid) = make_test_dacl(|acl, sid_ptr| {
            assert_ne!(
                unsafe { AddAccessAllowedAce(acl, ACL_REVISION, FILE_ALL_ACCESS, sid_ptr) },
                0
            );
            let mut ace = std::ptr::null_mut();
            assert_ne!(unsafe { GetAce(acl, 0, &raw mut ace) }, 0);
            assert!(!ace.is_null());
            let header_ptr = ace.cast::<ACE_HEADER>();
            let mut header = unsafe { header_ptr.read_unaligned() };
            header.AceType =
                u8::try_from(ACCESS_DENIED_ACE_TYPE).expect("ACCESS_DENIED_ACE_TYPE fits in u8");
            unsafe { header_ptr.write_unaligned(header) };
        });
        let mut descriptor =
            make_test_descriptor(&mut dacl_storage, sid.as_ptr().cast_mut().cast(), true);
        let desc_ptr = (&raw mut descriptor).cast::<core::ffi::c_void>();
        let err =
            unsafe { windows_verify_private_security_descriptor(desc_ptr, &sid) }.unwrap_err();
        assert!(err.to_string().contains("without inheritance flags"));
    }

    #[test]
    fn rejects_invalid_candidate_sid() {
        let (mut dacl_storage, sid) = make_test_dacl(|acl, sid_ptr| {
            assert_ne!(
                unsafe { AddAccessAllowedAce(acl, ACL_REVISION, FILE_ALL_ACCESS, sid_ptr) },
                0
            );
            let mut ace = std::ptr::null_mut();
            assert_ne!(unsafe { GetAce(acl, 0, &raw mut ace) }, 0);
            assert!(!ace.is_null());
            let sid_ptr = unsafe {
                ace.cast::<u8>()
                    .add(std::mem::offset_of!(ACCESS_ALLOWED_ACE, SidStart))
            };
            let revision_ptr = unsafe { sid_ptr.add(std::mem::offset_of!(SID, Revision)) };
            // SID_REVISION in Windows must be 1. Corrupting it to 0 causes IsValidSid to fail
            // while preserving ACE size, subauthority count, and DACL boundaries.
            unsafe { revision_ptr.write_unaligned(0) };
        });
        let mut descriptor =
            make_test_descriptor(&mut dacl_storage, sid.as_ptr().cast_mut().cast(), true);
        let desc_ptr = (&raw mut descriptor).cast::<core::ffi::c_void>();
        let err =
            unsafe { windows_verify_private_security_descriptor(desc_ptr, &sid) }.unwrap_err();
        assert!(
            err.to_string()
                .contains("contains an invalid candidate SID")
        );
    }
}
