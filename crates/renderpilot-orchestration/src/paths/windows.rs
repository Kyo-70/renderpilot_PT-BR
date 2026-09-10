use std::ffi::{OsStr, OsString};
use std::io;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

use windows_sys::Win32::Storage::FileSystem::GetLongPathNameW;

/// Expands every DOS 8.3 component in an existing canonical Windows path.
pub(super) fn expand_short_names(path: &Path) -> io::Result<PathBuf> {
    let input = wide_null(path.as_os_str());
    let required = get_long_path_name(&input, std::ptr::null_mut(), 0)?;
    let mut output = vec![0_u16; required as usize];
    let written = get_long_path_name(&input, output.as_mut_ptr(), required)?;
    if written >= required {
        return Err(io::Error::other(
            "Windows long-path result changed while it was being read",
        ));
    }
    output.truncate(written as usize);
    Ok(PathBuf::from(OsString::from_wide(&output)))
}

fn wide_null(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(std::iter::once(0)).collect()
}

#[expect(
    unsafe_code,
    reason = "GetLongPathNameW is the Windows API for expanding DOS 8.3 path aliases"
)]
fn get_long_path_name(input: &[u16], output: *mut u16, capacity: u32) -> io::Result<u32> {
    // SAFETY: `input` is NUL-terminated. `output` is either null for the size
    // query or points to `capacity` writable UTF-16 code units owned by the
    // caller.
    let result = unsafe { GetLongPathNameW(input.as_ptr(), output, capacity) };
    if result == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(result)
    }
}
