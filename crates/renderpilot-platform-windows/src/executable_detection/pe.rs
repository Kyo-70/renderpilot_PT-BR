use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

/// Returns whether `path` is a readable Windows PE executable.
///
/// This intentionally validates only the DOS and PE signatures required to
/// distinguish a real executable from an arbitrary file named `*.exe`.
#[must_use]
pub fn is_readable_windows_pe_executable(path: &Path) -> bool {
    let Ok(mut file) = File::open(path) else {
        return false;
    };
    let mut dos_header = [0_u8; 64];
    if file.read_exact(&mut dos_header).is_err() || &dos_header[..2] != b"MZ" {
        return false;
    }
    let pe_offset = u32::from_le_bytes([
        dos_header[0x3c],
        dos_header[0x3d],
        dos_header[0x3e],
        dos_header[0x3f],
    ]);
    if file.seek(SeekFrom::Start(u64::from(pe_offset))).is_err() {
        return false;
    }
    let mut signature = [0_u8; 4];
    file.read_exact(&mut signature).is_ok() && signature == *b"PE\0\0"
}
