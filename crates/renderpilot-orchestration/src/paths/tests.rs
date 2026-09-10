use super::*;
use std::path::Path;

#[test]
fn normalized_key_is_case_insensitive_and_forward_slash() {
    assert_eq!(
        normalized_key(Path::new(r"C:\Games\DLSS.dll")),
        "c:/games/dlss.dll"
    );
    assert_eq!(
        normalized_key(Path::new("C:/Games/DLSS.dll")),
        normalized_key(Path::new(r"c:\games\dlss.dll"))
    );
}

#[test]
fn is_within_accepts_self_and_descendants_only() {
    let root = Path::new(r"C:\Games");
    assert!(is_within(Path::new(r"C:\Games"), root));
    assert!(is_within(Path::new(r"C:\Games\sub\file.dll"), root));
    assert!(!is_within(Path::new(r"C:\GamesOther\file.dll"), root));
    assert!(!is_within(Path::new(r"D:\Games\file.dll"), root));
}

#[test]
fn is_within_handles_drive_root_scope() {
    assert!(is_within(Path::new("D:/foo"), Path::new("D:/")));
    assert!(is_within(Path::new("D:/"), Path::new("D:/")));
    assert!(!is_within(Path::new("E:/foo"), Path::new("D:/")));
}

#[test]
fn canonical_candidate_walks_up_to_existing_ancestor() {
    let dir = tempfile::tempdir().expect("tempdir");
    let target = dir.path().join("missing_dir").join("nvngx_dlss.dll");
    assert!(!target.exists());

    let resolved = canonical_candidate(&target).expect("walks up");
    let canonical_root = canonicalize_existing(dir.path()).expect("canonicalize tempdir");
    assert!(is_within(&resolved, &canonical_root));
    assert_eq!(resolved.file_name().unwrap(), "nvngx_dlss.dll");
}

#[cfg(windows)]
#[test]
fn canonicalize_existing_expands_short_name_components() {
    use std::ffi::OsString;
    use std::os::windows::ffi::{OsStrExt, OsStringExt};

    use windows_sys::Win32::Storage::FileSystem::GetShortPathNameW;

    #[expect(
        unsafe_code,
        reason = "GetShortPathNameW creates the Windows alias needed by this regression test"
    )]
    fn short_path(path: &Path) -> std::io::Result<std::path::PathBuf> {
        let input = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        // SAFETY: `input` is NUL-terminated and the null output performs only
        // the documented size query.
        let required = unsafe { GetShortPathNameW(input.as_ptr(), std::ptr::null_mut(), 0) };
        if required == 0 {
            return Err(std::io::Error::last_os_error());
        }
        let mut output = vec![0_u16; required as usize];
        // SAFETY: `output` owns `required` writable UTF-16 code units.
        let written = unsafe { GetShortPathNameW(input.as_ptr(), output.as_mut_ptr(), required) };
        if written == 0 {
            return Err(std::io::Error::last_os_error());
        }
        output.truncate(written as usize);
        Ok(std::path::PathBuf::from(OsString::from_wide(&output)))
    }

    let dir = tempfile::tempdir().expect("tempdir");
    let short = short_path(dir.path()).expect("short path");
    if normalized_key(&short) == normalized_key(dir.path()) {
        // 8.3 name creation can be disabled per volume.
        return;
    }

    let actual = canonicalize_existing(&short).expect("canonical short path");
    let expected = canonicalize_existing(dir.path()).expect("canonical long path");
    assert_eq!(normalized_key(&actual), normalized_key(&expected));
}

#[test]
fn same_path_is_case_insensitive_when_targets_are_missing() {
    assert!(same_path(
        Path::new(r"C:\Games\Missing\nvngx_dlss.dll"),
        Path::new("c:/games/missing/NVNGX_DLSS.DLL"),
    ));
    assert!(!same_path(
        Path::new(r"C:\Games\Missing\a.dll"),
        Path::new(r"C:\Games\Missing\b.dll"),
    ));
}
