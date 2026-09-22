//! Unreal Engine 3 fast-path package probe.
//!
//! Validates `FPackageFileSummary` headers (`.upk`, `.pck`, `.u`) using the historic
//! version provenance range [500, 950].

use std::io::{self, Read, Seek, SeekFrom};

/// Unreal Engine 3 package file tag in standard Little-Endian format (`0x9E2A83C1`).
pub const PACKAGE_FILE_TAG: u32 = 0x9E2A_83C1;

/// Unreal Engine 3 package file tag in byte-swapped Big-Endian format (`0xC1832A9E`).
pub const PACKAGE_FILE_TAG_SWAPPED: u32 = 0xC183_2A9E;

/// Minimum proven late-UE3 file version with the verified compressed package summary layout.
pub const MIN_PROVEN_COMPRESSED_VERSION: u16 = 767;

/// Bitmask flag indicating a compressed package (`PKG_StoreCompressed`).
pub const PKG_STORE_COMPRESSED: u32 = 0x0200_0000;

/// Minimum historical UE3 package file version (RoboBlitz / early UE3).
pub const MIN_UE3_FILE_VERSION: u16 = 500;

/// Maximum historical UE3 package file version (late UE3: Batman Arkham Knight, BioShock Infinite).
pub const MAX_UE3_FILE_VERSION: u16 = 950;

/// Minimum required bytes to parse the fixed prefix of `FPackageFileSummary`.
pub const MIN_PACKAGE_HEADER_BYTES: usize = 32;

/// Stable header summary of an Unreal Engine 3 package file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ue3PackageSummary {
    /// Raw tag observed in the header.
    pub tag: u32,
    /// Unreal Engine file version (must be in 500..=950 for UE3).
    pub file_version: u16,
    /// Licensee-specific version number.
    pub licensee_version: u16,
    /// Total size of the package headers in bytes.
    pub headers_size: i32,
}

/// Errors occurring during UE3 package header parsing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ue3PackageError {
    /// Low-level I/O error while reading the package stream.
    Io {
        /// Categorized I/O error kind.
        kind: io::ErrorKind,
        /// Descriptive error message.
        message: String,
    },
    /// File tag does not match standard or swapped UE3 package magic.
    InvalidTag(u32),
    /// Package file version is outside the UE3 historical range [500, 950].
    VersionOutOfRange(u16),
    /// Header size is negative, smaller than minimum header bytes, or exceeds total file length.
    InvalidHeaderSize(i32),
    /// Stream ended before reading the minimum 32-byte header prefix.
    TruncatedHeader,
    /// Malformed compressed package metadata or invalid chunk layout.
    InvalidCompressedChunks(&'static str),
}

/// Parses and structurally validates the header of a UE3 package (UPK / PCK / U).
///
/// Verifies the signature, checks that `file_version` falls in `500..=950`, and ensures
/// `headers_size >= 32` and `headers_size <= file_len` (or satisfies the late-UE3 compressed package fallback).
pub fn parse_ue3_package_summary<R: Read + Seek>(
    reader: &mut R,
    file_len: u64,
) -> Result<Ue3PackageSummary, Ue3PackageError> {
    if file_len < MIN_PACKAGE_HEADER_BYTES as u64 {
        return Err(Ue3PackageError::TruncatedHeader);
    }

    let mut buf = [0u8; 32];
    reader
        .seek(SeekFrom::Start(0))
        .map_err(|e| Ue3PackageError::Io {
            kind: e.kind(),
            message: e.to_string(),
        })?;
    reader.read_exact(&mut buf).map_err(|e| match e.kind() {
        io::ErrorKind::UnexpectedEof => Ue3PackageError::TruncatedHeader,
        _ => Ue3PackageError::Io {
            kind: e.kind(),
            message: e.to_string(),
        },
    })?;

    let raw_tag = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
    let is_little_endian = if raw_tag == PACKAGE_FILE_TAG {
        true
    } else if raw_tag == PACKAGE_FILE_TAG_SWAPPED {
        false
    } else {
        return Err(Ue3PackageError::InvalidTag(raw_tag));
    };

    let file_version = if is_little_endian {
        u16::from_le_bytes([buf[4], buf[5]])
    } else {
        u16::from_be_bytes([buf[4], buf[5]])
    };

    if !(MIN_UE3_FILE_VERSION..=MAX_UE3_FILE_VERSION).contains(&file_version) {
        return Err(Ue3PackageError::VersionOutOfRange(file_version));
    }

    let licensee_version = if is_little_endian {
        u16::from_le_bytes([buf[6], buf[7]])
    } else {
        u16::from_be_bytes([buf[6], buf[7]])
    };

    let headers_size = if is_little_endian {
        i32::from_le_bytes([buf[8], buf[9], buf[10], buf[11]])
    } else {
        i32::from_be_bytes([buf[8], buf[9], buf[10], buf[11]])
    };

    if headers_size < MIN_PACKAGE_HEADER_BYTES as i32 {
        return Err(Ue3PackageError::InvalidHeaderSize(headers_size));
    }

    if (headers_size as u64) > file_len {
        validate_compressed_package_fallback(
            reader,
            file_len,
            file_version,
            headers_size as u64,
            is_little_endian,
        )?;
    }

    Ok(Ue3PackageSummary {
        tag: raw_tag,
        file_version,
        licensee_version,
        headers_size,
    })
}

#[inline]
fn read_u32<R: Read>(reader: &mut R, is_le: bool) -> Result<u32, Ue3PackageError> {
    let mut buf = [0u8; 4];
    reader.read_exact(&mut buf).map_err(|e| match e.kind() {
        io::ErrorKind::UnexpectedEof => Ue3PackageError::TruncatedHeader,
        _ => Ue3PackageError::Io {
            kind: e.kind(),
            message: e.to_string(),
        },
    })?;
    Ok(if is_le {
        u32::from_le_bytes(buf)
    } else {
        u32::from_be_bytes(buf)
    })
}

#[inline]
fn read_i32<R: Read>(reader: &mut R, is_le: bool) -> Result<i32, Ue3PackageError> {
    let mut buf = [0u8; 4];
    reader.read_exact(&mut buf).map_err(|e| match e.kind() {
        io::ErrorKind::UnexpectedEof => Ue3PackageError::TruncatedHeader,
        _ => Ue3PackageError::Io {
            kind: e.kind(),
            message: e.to_string(),
        },
    })?;
    Ok(if is_le {
        i32::from_le_bytes(buf)
    } else {
        i32::from_be_bytes(buf)
    })
}

/// Minimal structural validation for late-UE3 compressed packages where `headers_size > file_len`.
fn validate_compressed_package_fallback<R: Read + Seek>(
    reader: &mut R,
    file_len: u64,
    file_version: u16,
    headers_size: u64,
    is_little_endian: bool,
) -> Result<(), Ue3PackageError> {
    if file_version < MIN_PROVEN_COMPRESSED_VERSION {
        return Err(Ue3PackageError::InvalidHeaderSize(headers_size as i32));
    }

    // Read folder_name length at offset 12
    reader
        .seek(SeekFrom::Start(12))
        .map_err(|e| Ue3PackageError::Io {
            kind: e.kind(),
            message: e.to_string(),
        })?;
    let folder_len = read_i32(reader, is_little_endian)?;
    let str_bytes = match folder_len.cmp(&0) {
        std::cmp::Ordering::Greater => folder_len as u64,
        std::cmp::Ordering::Less => {
            let abs_chars =
                folder_len
                    .checked_neg()
                    .ok_or(Ue3PackageError::InvalidCompressedChunks(
                        "overflow in folder name length",
                    ))? as u64;
            abs_chars
                .checked_mul(2)
                .ok_or(Ue3PackageError::InvalidCompressedChunks(
                    "overflow in utf16 folder name length",
                ))?
        }
        std::cmp::Ordering::Equal => 0,
    };

    let flags_pos =
        16u64
            .checked_add(str_bytes)
            .ok_or(Ue3PackageError::InvalidCompressedChunks(
                "overflow in flags position",
            ))?;

    // Read PackageFlags
    reader
        .seek(SeekFrom::Start(flags_pos))
        .map_err(|e| Ue3PackageError::Io {
            kind: e.kind(),
            message: e.to_string(),
        })?;
    let package_flags = read_u32(reader, is_little_endian)?;
    if (package_flags & PKG_STORE_COMPRESSED) == 0 {
        return Err(Ue3PackageError::InvalidHeaderSize(headers_size as i32));
    }

    // Fixed prefix between PackageFlags and Generations:
    // Names(8) + Exports(8) + Imports(8) + Depends(4) + Guids/Thumb(16) + Guid(16) = 60 bytes
    let gen_pos = flags_pos
        .checked_add(4 + 60)
        .ok_or(Ue3PackageError::InvalidCompressedChunks(
            "overflow in generations position",
        ))?;

    reader
        .seek(SeekFrom::Start(gen_pos))
        .map_err(|e| Ue3PackageError::Io {
            kind: e.kind(),
            message: e.to_string(),
        })?;
    let gen_count = read_i32(reader, is_little_endian)?;
    if gen_count <= 0 {
        return Err(Ue3PackageError::InvalidCompressedChunks(
            "invalid generations count",
        ));
    }

    // After Generations: EngineVer(4) + CookerVer(4) + CompFlags(4) + PkgSource(4) = 16 bytes
    let num_chunks_pos = gen_pos
        .checked_add(4)
        .and_then(|p| p.checked_add((gen_count as u64).checked_mul(8)?))
        .and_then(|p| p.checked_add(16))
        .ok_or(Ue3PackageError::InvalidCompressedChunks(
            "overflow in num_chunks position",
        ))?;

    reader
        .seek(SeekFrom::Start(num_chunks_pos))
        .map_err(|e| Ue3PackageError::Io {
            kind: e.kind(),
            message: e.to_string(),
        })?;
    let num_chunks = read_u32(reader, is_little_endian)?;
    if num_chunks == 0 {
        return Err(Ue3PackageError::InvalidCompressedChunks(
            "empty chunks table",
        ));
    }

    // Natural structural bound: descriptor array must fit within file_len
    let descriptor_table_bytes =
        (num_chunks as u64)
            .checked_mul(16)
            .ok_or(Ue3PackageError::InvalidCompressedChunks(
                "num_chunks multiplication overflow",
            ))?;
    let chunks_start_pos =
        num_chunks_pos
            .checked_add(4)
            .ok_or(Ue3PackageError::InvalidCompressedChunks(
                "overflow in chunks start",
            ))?;
    let chunks_end_pos = chunks_start_pos.checked_add(descriptor_table_bytes).ok_or(
        Ue3PackageError::InvalidCompressedChunks("overflow in chunks end"),
    )?;
    if chunks_end_pos > file_len {
        return Err(Ue3PackageError::TruncatedHeader);
    }

    // Read first chunk descriptor (16 bytes)
    reader
        .seek(SeekFrom::Start(chunks_start_pos))
        .map_err(|e| Ue3PackageError::Io {
            kind: e.kind(),
            message: e.to_string(),
        })?;
    let u_off = read_i32(reader, is_little_endian)?;
    let u_size = read_i32(reader, is_little_endian)?;
    let c_off = read_i32(reader, is_little_endian)?;
    let c_size = read_i32(reader, is_little_endian)?;

    if u_off < 0 || u_size <= 0 || c_off < 0 || c_size <= 0 {
        return Err(Ue3PackageError::InvalidCompressedChunks(
            "negative or zero chunk bounds",
        ));
    }

    let uncomp_offset = u_off as u64;
    let uncomp_size = u_size as u64;
    let comp_offset = c_off as u64;
    let comp_size = c_size as u64;

    let logical_end =
        uncomp_offset
            .checked_add(uncomp_size)
            .ok_or(Ue3PackageError::InvalidCompressedChunks(
                "logical range overflow",
            ))?;
    let physical_end =
        comp_offset
            .checked_add(comp_size)
            .ok_or(Ue3PackageError::InvalidCompressedChunks(
                "physical range overflow",
            ))?;

    let summary_min_end =
        chunks_end_pos
            .checked_add(12)
            .ok_or(Ue3PackageError::InvalidCompressedChunks(
                "overflow in summary minimum end",
            ))?;

    if comp_offset < summary_min_end {
        return Err(Ue3PackageError::InvalidCompressedChunks(
            "first chunk overlaps package summary",
        ));
    }
    if uncomp_offset > headers_size || headers_size > logical_end {
        return Err(Ue3PackageError::InvalidCompressedChunks(
            "chunk logical range does not cover headers_size",
        ));
    }
    if physical_end > file_len {
        return Err(Ue3PackageError::InvalidCompressedChunks(
            "chunk physical range exceeds file length",
        ));
    }

    // Structural validation of inner FCompressedChunkHeader at comp_offset
    validate_inner_chunk_container(
        reader,
        comp_offset,
        comp_size,
        uncomp_size,
        is_little_endian,
    )?;

    Ok(())
}

/// Validates the physical FCompressedChunkHeader at `comp_offset` without decompression.
fn validate_inner_chunk_container<R: Read + Seek>(
    reader: &mut R,
    comp_offset: u64,
    comp_size: u64,
    expected_uncomp_size: u64,
    is_little_endian: bool,
) -> Result<(), Ue3PackageError> {
    reader
        .seek(SeekFrom::Start(comp_offset))
        .map_err(|e| Ue3PackageError::Io {
            kind: e.kind(),
            message: e.to_string(),
        })?;

    let tag = read_u32(reader, is_little_endian)?;
    if tag != PACKAGE_FILE_TAG && tag != PACKAGE_FILE_TAG_SWAPPED {
        return Err(Ue3PackageError::InvalidCompressedChunks(
            "invalid chunk container tag",
        ));
    }

    let block_size = read_i32(reader, is_little_endian)?;
    let summary_comp_size = read_i32(reader, is_little_endian)?;
    let summary_uncomp_size = read_i32(reader, is_little_endian)?;

    if block_size <= 0 || summary_comp_size <= 0 || summary_uncomp_size <= 0 {
        return Err(Ue3PackageError::InvalidCompressedChunks(
            "invalid chunk container dimensions",
        ));
    }
    if (summary_uncomp_size as u64) != expected_uncomp_size {
        return Err(Ue3PackageError::InvalidCompressedChunks(
            "chunk container uncompressed size mismatch",
        ));
    }

    let num_blocks = (summary_uncomp_size as u64).div_ceil(block_size as u64);
    let header_overhead = 16u64
        .checked_add(
            num_blocks
                .checked_mul(8)
                .ok_or(Ue3PackageError::InvalidCompressedChunks("overflow"))?,
        )
        .ok_or(Ue3PackageError::InvalidCompressedChunks("overflow"))?;
    let total_required = header_overhead
        .checked_add(summary_comp_size as u64)
        .ok_or(Ue3PackageError::InvalidCompressedChunks("overflow"))?;

    if total_required > comp_size {
        return Err(Ue3PackageError::InvalidCompressedChunks(
            "chunk container exceeds declared compressed size",
        ));
    }

    Ok(())
}

/// Fast predicate testing whether a reader stream represents a valid UE3 package.
pub fn probe_ue3_package<R: Read + Seek>(reader: &mut R, file_len: u64) -> bool {
    parse_ue3_package_summary(reader, file_len).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn build_ue3_header(tag: u32, version: u16, licensee: u16, headers_size: i32) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(&tag.to_le_bytes());
        buf.extend_from_slice(&version.to_le_bytes());
        buf.extend_from_slice(&licensee.to_le_bytes());
        buf.extend_from_slice(&headers_size.to_le_bytes());
        buf.resize(64, 0);
        buf
    }

    #[test]
    fn test_valid_ue3_package() {
        let data = build_ue3_header(PACKAGE_FILE_TAG, 832, 0, 50);
        let mut cursor = Cursor::new(&data);
        let summary = parse_ue3_package_summary(&mut cursor, data.len() as u64).unwrap();
        assert_eq!(summary.file_version, 832);
        assert_eq!(summary.headers_size, 50);
        assert!(probe_ue3_package(
            &mut Cursor::new(&data),
            data.len() as u64
        ));
    }

    #[test]
    fn test_rejects_ue2_version() {
        let data = build_ue3_header(PACKAGE_FILE_TAG, 128, 0, 50);
        let mut cursor = Cursor::new(&data);
        assert_eq!(
            parse_ue3_package_summary(&mut cursor, data.len() as u64),
            Err(Ue3PackageError::VersionOutOfRange(128))
        );
    }

    #[test]
    fn test_rejects_invalid_tag() {
        let data = build_ue3_header(0x12345678, 832, 0, 50);
        let mut cursor = Cursor::new(&data);
        assert_eq!(
            parse_ue3_package_summary(&mut cursor, data.len() as u64),
            Err(Ue3PackageError::InvalidTag(0x12345678))
        );
    }

    #[test]
    fn test_rejects_too_small_headers_size() {
        let data = build_ue3_header(PACKAGE_FILE_TAG, 832, 0, 10);
        let mut cursor = Cursor::new(&data);
        assert_eq!(
            parse_ue3_package_summary(&mut cursor, data.len() as u64),
            Err(Ue3PackageError::InvalidHeaderSize(10))
        );
    }
    #[test]
    fn test_rejects_truncated_file() {
        let data = vec![0u8; 16];
        let mut cursor = Cursor::new(&data);
        assert_eq!(
            parse_ue3_package_summary(&mut cursor, data.len() as u64),
            Err(Ue3PackageError::TruncatedHeader)
        );
    }

    struct FailingReader;
    impl Read for FailingReader {
        fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "permission denied",
            ))
        }
    }
    impl Seek for FailingReader {
        fn seek(&mut self, _pos: SeekFrom) -> io::Result<u64> {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "permission denied",
            ))
        }
    }

    #[test]
    fn test_io_error_preserves_error_kind() {
        let mut failing = FailingReader;
        let res = parse_ue3_package_summary(&mut failing, 100);
        match res {
            Err(Ue3PackageError::Io { kind, message }) => {
                assert_eq!(kind, io::ErrorKind::PermissionDenied);
                assert!(message.contains("permission denied"));
            }
            other => panic!("expected Ue3PackageError::Io, got {:?}", other),
        }
    }

    #[derive(Clone, Copy)]
    struct SyntheticCompressedPackageSpec {
        file_version: u16,
        headers_size: i32,
        uncompressed_offset: i32,
        uncompressed_size: i32,
        compressed_offset: i32,
        compressed_size: i32,
        container_tag: u32,
        block_size: i32,
        summary_compressed_size: i32,
        summary_uncompressed_size: i32,
        total_file_len: usize,
    }

    fn valid_synthetic_compressed_package_spec() -> SyntheticCompressedPackageSpec {
        SyntheticCompressedPackageSpec {
            file_version: 845,
            headers_size: 1470,
            uncompressed_offset: 129,
            uncompressed_size: 1866,
            compressed_offset: 145,
            compressed_size: 904,
            container_tag: PACKAGE_FILE_TAG,
            block_size: 131072,
            summary_compressed_size: 880,
            summary_uncompressed_size: 1866,
            total_file_len: 1049,
        }
    }

    fn build_synthetic_compressed_package(spec: SyntheticCompressedPackageSpec) -> Vec<u8> {
        let SyntheticCompressedPackageSpec {
            file_version,
            headers_size,
            uncompressed_offset: u_off,
            uncompressed_size: u_size,
            compressed_offset: c_off,
            compressed_size: c_size,
            container_tag,
            block_size,
            summary_compressed_size: summary_comp,
            summary_uncompressed_size: summary_uncomp,
            total_file_len,
        } = spec;
        let mut buf = Vec::new();
        buf.extend_from_slice(&PACKAGE_FILE_TAG.to_le_bytes());
        buf.extend_from_slice(&file_version.to_le_bytes());
        buf.extend_from_slice(&4u16.to_le_bytes());
        buf.extend_from_slice(&headers_size.to_le_bytes());
        buf.extend_from_slice(&5i32.to_le_bytes());
        buf.extend_from_slice(b"None\0");
        buf.extend_from_slice(&0x0288_0009u32.to_le_bytes());
        buf.resize(85, 0);
        buf.extend_from_slice(&1i32.to_le_bytes());
        buf.extend_from_slice(&4i32.to_le_bytes());
        buf.extend_from_slice(&36i32.to_le_bytes());
        buf.extend_from_slice(&0i32.to_le_bytes());
        buf.extend_from_slice(&8916i32.to_le_bytes());
        buf.extend_from_slice(&0x87u32.to_le_bytes());
        buf.extend_from_slice(&2u32.to_le_bytes());
        buf.extend_from_slice(&1u32.to_le_bytes());
        buf.extend_from_slice(&u_off.to_le_bytes());
        buf.extend_from_slice(&u_size.to_le_bytes());
        buf.extend_from_slice(&c_off.to_le_bytes());
        buf.extend_from_slice(&c_size.to_le_bytes());

        if buf.len() < c_off as usize {
            buf.resize(c_off as usize, 0);
        }

        buf.extend_from_slice(&container_tag.to_le_bytes());
        buf.extend_from_slice(&block_size.to_le_bytes());
        buf.extend_from_slice(&summary_comp.to_le_bytes());
        buf.extend_from_slice(&summary_uncomp.to_le_bytes());
        buf.extend_from_slice(&summary_comp.to_le_bytes());
        buf.extend_from_slice(&summary_uncomp.to_le_bytes());

        if buf.len() < total_file_len {
            buf.resize(total_file_len, 0);
        }
        buf
    }

    #[test]
    fn test_uncompressed_rejects_headers_size_larger_than_file() {
        let data = build_ue3_header(PACKAGE_FILE_TAG, 832, 0, 500);
        let mut cursor = Cursor::new(&data);
        assert_eq!(
            parse_ue3_package_summary(&mut cursor, data.len() as u64),
            Err(Ue3PackageError::InvalidHeaderSize(500))
        );
    }

    #[test]
    fn test_compressed_flag_without_chunk_metadata_rejected() {
        let mut data = build_ue3_header(PACKAGE_FILE_TAG, 845, 4, 1470);
        // Set folder_len = 5, "None\0", and PKG_STORE_COMPRESSED
        data.truncate(12);
        data.extend_from_slice(&5i32.to_le_bytes());
        data.extend_from_slice(b"None\0");
        data.extend_from_slice(&0x0288_0009u32.to_le_bytes());
        let len = data.len() as u64;
        let mut cursor = Cursor::new(&data);
        assert_eq!(
            parse_ue3_package_summary(&mut cursor, len),
            Err(Ue3PackageError::TruncatedHeader)
        );
    }

    #[test]
    fn test_dmc_like_valid_compressed_package_accepted() {
        let data = build_synthetic_compressed_package(valid_synthetic_compressed_package_spec());
        let len = data.len() as u64;
        let mut cursor = Cursor::new(&data);
        let summary = parse_ue3_package_summary(&mut cursor, len).unwrap();
        assert_eq!(summary.file_version, 845);
        assert_eq!(summary.headers_size, 1470);
        assert!(probe_ue3_package(&mut Cursor::new(&data), len));
    }

    #[test]
    fn test_compressed_rejects_physical_chunk_beyond_eof() {
        // c_off (145) + c_size (904) = 1049, but file_len is only 1000
        let data = build_synthetic_compressed_package(SyntheticCompressedPackageSpec {
            total_file_len: 1000,
            ..valid_synthetic_compressed_package_spec()
        });
        let mut cursor = Cursor::new(&data);
        assert_eq!(
            parse_ue3_package_summary(&mut cursor, 1000),
            Err(Ue3PackageError::InvalidCompressedChunks(
                "chunk physical range exceeds file length"
            ))
        );
    }

    #[test]
    fn test_compressed_rejects_logical_chunk_not_covering_headers_size() {
        // u_off (129) + u_size (500) = 629, which does not cover headers_size = 1470
        let data = build_synthetic_compressed_package(SyntheticCompressedPackageSpec {
            uncompressed_size: 500,
            summary_uncompressed_size: 500,
            ..valid_synthetic_compressed_package_spec()
        });
        let len = data.len() as u64;
        let mut cursor = Cursor::new(&data);
        assert_eq!(
            parse_ue3_package_summary(&mut cursor, len),
            Err(Ue3PackageError::InvalidCompressedChunks(
                "chunk logical range does not cover headers_size"
            ))
        );
    }

    #[test]
    fn test_compressed_rejects_corrupted_inner_container_header() {
        // Container tag is 0xDEADBEEF instead of PACKAGE_FILE_TAG
        let data = build_synthetic_compressed_package(SyntheticCompressedPackageSpec {
            container_tag: 0xDEAD_BEEF,
            ..valid_synthetic_compressed_package_spec()
        });
        let len = data.len() as u64;
        let mut cursor = Cursor::new(&data);
        assert_eq!(
            parse_ue3_package_summary(&mut cursor, len),
            Err(Ue3PackageError::InvalidCompressedChunks(
                "invalid chunk container tag"
            ))
        );
    }

    #[test]
    fn test_compressed_rejects_overflow_or_negative_serialized_field() {
        // Negative uncompressed_offset (-1)
        let data = build_synthetic_compressed_package(SyntheticCompressedPackageSpec {
            uncompressed_offset: -1,
            ..valid_synthetic_compressed_package_spec()
        });
        let len = data.len() as u64;
        let mut cursor = Cursor::new(&data);
        assert_eq!(
            parse_ue3_package_summary(&mut cursor, len),
            Err(Ue3PackageError::InvalidCompressedChunks(
                "negative or zero chunk bounds"
            ))
        );
    }

    #[test]
    fn test_compressed_rejects_first_chunk_overlapping_chunk_metadata() {
        // chunks_end_pos is 133, but c_off is 120 (starts inside chunks array)
        let data = build_synthetic_compressed_package(SyntheticCompressedPackageSpec {
            compressed_offset: 120,
            ..valid_synthetic_compressed_package_spec()
        });
        let len = data.len() as u64;
        let mut cursor = Cursor::new(&data);
        assert_eq!(
            parse_ue3_package_summary(&mut cursor, len),
            Err(Ue3PackageError::InvalidCompressedChunks(
                "first chunk overlaps package summary"
            ))
        );
    }

    #[test]
    fn test_compressed_rejects_first_chunk_overlapping_trailing_summary_bytes() {
        // chunks_end_pos is 133; trailing summary requires at least 12 bytes (up to 145).
        // c_off is 140 (outside chunks array, but inside trailing summary fields).
        let data = build_synthetic_compressed_package(SyntheticCompressedPackageSpec {
            compressed_offset: 140,
            ..valid_synthetic_compressed_package_spec()
        });
        let len = data.len() as u64;
        let mut cursor = Cursor::new(&data);
        assert_eq!(
            parse_ue3_package_summary(&mut cursor, len),
            Err(Ue3PackageError::InvalidCompressedChunks(
                "first chunk overlaps package summary"
            ))
        );
    }

    #[test]
    fn test_compressed_accepts_negative_utf16_folder_name_len() {
        // Negative length -5 means UTF-16LE string of 5 characters (10 bytes)
        let mut data = Vec::new();
        data.extend_from_slice(&PACKAGE_FILE_TAG.to_le_bytes());
        data.extend_from_slice(&845u16.to_le_bytes());
        data.extend_from_slice(&4u16.to_le_bytes());
        data.extend_from_slice(&1470i32.to_le_bytes());
        // FolderName len = -5 (UTF-16LE, 10 bytes)
        data.extend_from_slice(&(-5i32).to_le_bytes());
        data.extend_from_slice(b"N\0o\0n\0e\0\0\0");
        // PackageFlags at 16 + 10 = 26
        data.extend_from_slice(&0x0288_0009u32.to_le_bytes());
        // 60 bytes of tables/guids
        data.resize(data.len() + 60, 0);
        // gen_count = 1
        data.extend_from_slice(&1i32.to_le_bytes());
        data.extend_from_slice(&4i32.to_le_bytes());
        data.extend_from_slice(&36i32.to_le_bytes());
        // EngineVer(0), CookerVer(8916), CompFlags(0x87), PkgSource(2)
        data.extend_from_slice(&0i32.to_le_bytes());
        data.extend_from_slice(&8916i32.to_le_bytes());
        data.extend_from_slice(&0x87u32.to_le_bytes());
        data.extend_from_slice(&2u32.to_le_bytes());
        // num_chunks = 1
        data.extend_from_slice(&1u32.to_le_bytes());
        let c_off = 160i32;
        let c_size = 904i32;
        // Chunk 0: u_off=129, u_size=1866, c_off=160, c_size=904
        data.extend_from_slice(&129i32.to_le_bytes());
        data.extend_from_slice(&1866i32.to_le_bytes());
        data.extend_from_slice(&c_off.to_le_bytes());
        data.extend_from_slice(&c_size.to_le_bytes());

        if data.len() < c_off as usize {
            data.resize(c_off as usize, 0);
        }

        // FCompressedChunkHeader at c_off
        data.extend_from_slice(&PACKAGE_FILE_TAG.to_le_bytes());
        data.extend_from_slice(&131072i32.to_le_bytes());
        data.extend_from_slice(&880i32.to_le_bytes());
        data.extend_from_slice(&1866i32.to_le_bytes());
        data.extend_from_slice(&880i32.to_le_bytes());
        data.extend_from_slice(&1866i32.to_le_bytes());

        let total_file_len = (c_off + c_size) as usize;
        if data.len() < total_file_len {
            data.resize(total_file_len, 0);
        }

        let len = data.len() as u64;
        let mut cursor = Cursor::new(&data);
        let summary = parse_ue3_package_summary(&mut cursor, len).unwrap();
        assert_eq!(summary.file_version, 845);
        assert_eq!(summary.headers_size, 1470);
    }
}
