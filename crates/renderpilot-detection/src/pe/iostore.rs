//! Unreal Engine IoStore (.utoc) structural header parser and validator.
//!
//! Validates the fixed 144-byte `FIoStoreTocHeader` for container presence and version extraction.

use std::io::{self, Read, Seek, SeekFrom};

/// Unreal Engine IoStore TOC 16-byte magic signature (`"-==--==--==--==-"`).
pub const IOSTORE_TOC_MAGIC: [u8; 16] = *b"-==--==--==--==-";

/// Fixed size of the `FIoStoreTocHeader` in bytes (144 bytes / 0x90).
pub const IOSTORE_TOC_HEADER_SIZE: usize = 144;

/// Expected size of a single compressed block entry in bytes (`sizeof(FIoStoreTocCompressedBlockEntry)`).
pub const IOSTORE_COMPRESSED_BLOCK_ENTRY_SIZE: u32 = 12;

/// Stable header summary of an Unreal Engine IoStore `.utoc` container.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IoStoreTocSummary {
    /// Raw TOC version byte at offset 0x10.
    pub toc_version: u8,
    /// Total number of TOC chunk entries.
    pub entry_count: u32,
    /// Total number of compressed block entries.
    pub compressed_block_entry_count: u32,
    /// Container flags bitfield (e.g. compressed, encrypted, signed, indexed).
    pub container_flags: u8,
    /// Size of a compression block in bytes.
    pub compression_block_size: u32,
    /// Size of the serialized directory index in bytes.
    pub directory_index_size: u32,
    /// Number of container partitions.
    pub partition_count: u32,
    /// 64-bit container identifier.
    pub container_id: u64,
}

/// Errors occurring during IoStore TOC header parsing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IoStoreTocError {
    /// Low-level I/O error while reading the TOC stream.
    Io {
        /// Categorized I/O error kind.
        kind: io::ErrorKind,
        /// Descriptive error message.
        message: String,
    },
    /// Stream ended before reading the full 144-byte header.
    TruncatedHeader,
    /// File magic does not match `IOSTORE_TOC_MAGIC`.
    InvalidMagic,
    /// Header size field does not equal 144 bytes.
    InvalidHeaderSize(u32),
    /// Compressed block entry size field does not equal 12 bytes.
    InvalidBlockEntrySize(u32),
    /// Arithmetic sanity check failed (e.g. integer overflow or declared tables exceed file length).
    SanityCheckFailed(&'static str),
}

/// Parses and structurally validates the 144-byte `FIoStoreTocHeader` and table layout from an in-memory slice.
pub fn parse_iostore_toc_summary_bytes(
    bytes: &[u8],
    file_len: u64,
) -> Result<IoStoreTocSummary, IoStoreTocError> {
    let mut cursor = io::Cursor::new(bytes);
    parse_iostore_toc_summary(&mut cursor, file_len)
}

/// Reads and structurally validates the 144-byte `FIoStoreTocHeader` and table bounds from a seekable reader.
pub fn parse_iostore_toc_summary<R: Read + Seek>(
    reader: &mut R,
    file_len: u64,
) -> Result<IoStoreTocSummary, IoStoreTocError> {
    if file_len < IOSTORE_TOC_HEADER_SIZE as u64 {
        return Err(IoStoreTocError::TruncatedHeader);
    }

    let mut bytes = [0u8; IOSTORE_TOC_HEADER_SIZE];
    reader
        .seek(SeekFrom::Start(0))
        .map_err(|e| IoStoreTocError::Io {
            kind: e.kind(),
            message: e.to_string(),
        })?;
    reader.read_exact(&mut bytes).map_err(|e| match e.kind() {
        io::ErrorKind::UnexpectedEof => IoStoreTocError::TruncatedHeader,
        _ => IoStoreTocError::Io {
            kind: e.kind(),
            message: e.to_string(),
        },
    })?;

    // 0x00..0x10: Magic
    if bytes[0..16] != IOSTORE_TOC_MAGIC {
        return Err(IoStoreTocError::InvalidMagic);
    }

    // 0x10: Version
    let toc_version = bytes[16];

    // 0x14..0x18: TocHeaderSize (must be 144)
    let toc_header_size = u32::from_le_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
    if toc_header_size != IOSTORE_TOC_HEADER_SIZE as u32 {
        return Err(IoStoreTocError::InvalidHeaderSize(toc_header_size));
    }

    // 0x18..0x1C: TocEntryCount
    let entry_count = u32::from_le_bytes([bytes[24], bytes[25], bytes[26], bytes[27]]);

    // 0x1C..0x20: TocCompressedBlockEntryCount
    let compressed_block_entry_count =
        u32::from_le_bytes([bytes[28], bytes[29], bytes[30], bytes[31]]);

    // 0x20..0x24: TocCompressedBlockEntrySize (must be 12)
    let compressed_block_entry_size =
        u32::from_le_bytes([bytes[32], bytes[33], bytes[34], bytes[35]]);
    if compressed_block_entry_size != IOSTORE_COMPRESSED_BLOCK_ENTRY_SIZE {
        return Err(IoStoreTocError::InvalidBlockEntrySize(
            compressed_block_entry_size,
        ));
    }

    // 0x24..0x28: CompressionMethodNameCount
    let compression_method_name_count =
        u32::from_le_bytes([bytes[36], bytes[37], bytes[38], bytes[39]]);

    // 0x28..0x2C: CompressionMethodNameLength
    let compression_method_name_length =
        u32::from_le_bytes([bytes[40], bytes[41], bytes[42], bytes[43]]);

    // 0x2C..0x30: CompressionBlockSize
    let compression_block_size = u32::from_le_bytes([bytes[44], bytes[45], bytes[46], bytes[47]]);

    // 0x30..0x34: DirectoryIndexSize
    let directory_index_size = u32::from_le_bytes([bytes[48], bytes[49], bytes[50], bytes[51]]);

    // 0x34..0x38: PartitionCount
    let partition_count = u32::from_le_bytes([bytes[52], bytes[53], bytes[54], bytes[55]]);

    // 0x38..0x40: ContainerId
    let container_id = u64::from_le_bytes([
        bytes[56], bytes[57], bytes[58], bytes[59], bytes[60], bytes[61], bytes[62], bytes[63],
    ]);

    // 0x50: ContainerFlags
    let container_flags = bytes[80];

    // 0x54..0x58: TocChunkPerfectHashSeedsCount (UE 4.27+ / versions >= 4)
    let toc_chunk_perfect_hash_seeds_count =
        u32::from_le_bytes([bytes[84], bytes[85], bytes[86], bytes[87]]);

    // 0x60..0x64: TocChunksWithoutPerfectHashCount (UE 4.27+ / versions >= 4)
    let toc_chunks_without_perfect_hash_count =
        u32::from_le_bytes([bytes[96], bytes[97], bytes[98], bytes[99]]);

    // Bounded cumulative table size verification:
    // Sequential UE IoStore .utoc layout:
    // 1. Fixed header: 144 bytes
    // 2. Chunk ID table: entry_count * 12 bytes
    // 3. Chunk offset/length table: entry_count * 10 bytes (`FIoOffsetAndLength` is always 10 bytes: 5 offset + 5 length)
    // 4. Perfect hash seed tables (in v >= 4):
    //      seeds: toc_chunk_perfect_hash_seeds_count * 4 bytes (i32)
    //      overflow chunk indices: toc_chunks_without_perfect_hash_count * 4 bytes (i32)
    // 5. Compressed block entry table: compressed_block_entry_count * 12 bytes
    // 6. Compression method names table: compression_method_name_count * compression_method_name_length
    // 7. Signatures section (if container_flags has Signed bit 0x04):
    //      - u32 signature_size (4 bytes)
    //      - toc_signature: signature_size bytes
    //      - block_signature: signature_size bytes
    //      - chunk_block_signatures: compressed_block_entry_count * 20 bytes (SHA-1 per block)
    // 8. Directory index table: directory_index_size bytes
    // 9. Chunk metadata table (`TocChunkMetas` / `FIoStoreTocEntryMeta`):
    //      entry_count * meta_entry_size bytes:
    //      - in v >= 8 (ReplaceIoChunkHashWithIoHash): 24 bytes (20-byte IoHash + 1-byte flags + 3-byte pad)
    //      - in v < 8: 33 bytes (32-byte IoChunkHash + 1-byte flags)
    let chunk_id_table_size =
        (entry_count as u64)
            .checked_mul(12)
            .ok_or(IoStoreTocError::SanityCheckFailed(
                "chunk_id table size arithmetic overflow",
            ))?;

    let chunk_offset_table_size =
        (entry_count as u64)
            .checked_mul(10)
            .ok_or(IoStoreTocError::SanityCheckFailed(
                "chunk_offset table size arithmetic overflow",
            ))?;

    let perfect_hash_seeds_size =
        if toc_version >= 4 {
            let seeds_size = (toc_chunk_perfect_hash_seeds_count as u64)
                .checked_mul(4)
                .ok_or(IoStoreTocError::SanityCheckFailed(
                    "perfect hash seeds table size overflow",
                ))?;
            let overflow_chunks_size = (toc_chunks_without_perfect_hash_count as u64)
                .checked_mul(4)
                .ok_or(IoStoreTocError::SanityCheckFailed(
                    "overflow chunk_id table size overflow",
                ))?;
            seeds_size.checked_add(overflow_chunks_size).ok_or(
                IoStoreTocError::SanityCheckFailed("perfect hash tables cumulative overflow"),
            )?
        } else {
            0
        };

    let block_table_size = (compressed_block_entry_count as u64)
        .checked_mul(compressed_block_entry_size as u64)
        .ok_or(IoStoreTocError::SanityCheckFailed(
            "compressed block table size arithmetic overflow",
        ))?;

    let compression_methods_table_size = (compression_method_name_count as u64)
        .checked_mul(compression_method_name_length as u64)
        .ok_or(IoStoreTocError::SanityCheckFailed(
            "compression_methods table size arithmetic overflow",
        ))?;

    let sig_offset = (IOSTORE_TOC_HEADER_SIZE as u64)
        .checked_add(chunk_id_table_size)
        .and_then(|acc| acc.checked_add(chunk_offset_table_size))
        .and_then(|acc| acc.checked_add(perfect_hash_seeds_size))
        .and_then(|acc| acc.checked_add(block_table_size))
        .and_then(|acc| acc.checked_add(compression_methods_table_size))
        .ok_or(IoStoreTocError::SanityCheckFailed(
            "cumulative table bounds arithmetic overflow",
        ))?;

    let signatures_size = if (container_flags & 0x04) != 0 {
        let sig_offset_plus_4 =
            sig_offset
                .checked_add(4)
                .ok_or(IoStoreTocError::SanityCheckFailed(
                    "cumulative table bounds arithmetic overflow",
                ))?;
        if file_len < sig_offset_plus_4 {
            return Err(IoStoreTocError::SanityCheckFailed(
                "declared tables exceed file length",
            ));
        }

        reader
            .seek(SeekFrom::Start(sig_offset))
            .map_err(|e| IoStoreTocError::Io {
                kind: e.kind(),
                message: e.to_string(),
            })?;
        let mut sig_size_buf = [0u8; 4];
        reader
            .read_exact(&mut sig_size_buf)
            .map_err(|e| match e.kind() {
                io::ErrorKind::UnexpectedEof => {
                    IoStoreTocError::SanityCheckFailed("declared tables exceed file length")
                }
                _ => IoStoreTocError::Io {
                    kind: e.kind(),
                    message: e.to_string(),
                },
            })?;
        let signature_size = u32::from_le_bytes(sig_size_buf) as u64;
        let two_sig_blobs =
            signature_size
                .checked_mul(2)
                .ok_or(IoStoreTocError::SanityCheckFailed(
                    "signatures table size overflow",
                ))?;
        let block_hashes = (compressed_block_entry_count as u64)
            .checked_mul(20)
            .ok_or(IoStoreTocError::SanityCheckFailed(
                "signatures table size overflow",
            ))?;
        (4u64)
            .checked_add(two_sig_blobs)
            .and_then(|s| s.checked_add(block_hashes))
            .ok_or(IoStoreTocError::SanityCheckFailed(
                "signatures table size overflow",
            ))?
    } else {
        0
    };

    let directory_index_table_size = directory_index_size as u64;

    let meta_entry_size: u64 = if toc_version >= 8 { 24 } else { 33 };
    let chunk_metas_table_size = (entry_count as u64).checked_mul(meta_entry_size).ok_or(
        IoStoreTocError::SanityCheckFailed("chunk_metas table size overflow"),
    )?;

    let min_required_file_len = sig_offset
        .checked_add(signatures_size)
        .and_then(|acc| acc.checked_add(directory_index_table_size))
        .and_then(|acc| acc.checked_add(chunk_metas_table_size))
        .ok_or(IoStoreTocError::SanityCheckFailed(
            "cumulative table bounds arithmetic overflow",
        ))?;

    if file_len < min_required_file_len {
        return Err(IoStoreTocError::SanityCheckFailed(
            "declared tables exceed file length",
        ));
    }

    Ok(IoStoreTocSummary {
        toc_version,
        entry_count,
        compressed_block_entry_count,
        container_flags,
        compression_block_size,
        directory_index_size,
        partition_count,
        container_id,
    })
}

/// Fast predicate testing whether a reader stream represents a structurally valid IoStore TOC container.
pub fn probe_iostore_toc<R: Read + Seek>(reader: &mut R, file_len: u64) -> bool {
    parse_iostore_toc_summary(reader, file_len).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn build_iostore_header(
        magic: [u8; 16],
        version: u8,
        header_size: u32,
        entry_count: u32,
        block_count: u32,
        block_entry_size: u32,
        total_file_len: usize,
    ) -> Vec<u8> {
        let mut buf = vec![0u8; total_file_len.max(144)];
        buf[0..16].copy_from_slice(&magic);
        buf[16] = version;
        buf[20..24].copy_from_slice(&header_size.to_le_bytes());
        buf[24..28].copy_from_slice(&entry_count.to_le_bytes());
        buf[28..32].copy_from_slice(&block_count.to_le_bytes());
        buf[32..36].copy_from_slice(&block_entry_size.to_le_bytes());
        buf
    }

    #[test]
    fn test_valid_iostore_header_version_6() {
        // entry_count 10 (120 + 100 = 220 bytes), block_count 5 (60 bytes) -> min 144 + 280 = 424 bytes
        let data = build_iostore_header(IOSTORE_TOC_MAGIC, 6, 144, 10, 5, 12, 1024);
        let mut cursor = Cursor::new(&data);
        let summary = parse_iostore_toc_summary(&mut cursor, data.len() as u64).unwrap();
        assert_eq!(summary.toc_version, 6);
        assert_eq!(summary.entry_count, 10);
        assert_eq!(summary.compressed_block_entry_count, 5);
        assert!(probe_iostore_toc(
            &mut Cursor::new(&data),
            data.len() as u64
        ));
    }

    #[test]
    fn test_valid_iostore_header_version_8() {
        let data = build_iostore_header(IOSTORE_TOC_MAGIC, 8, 144, 500, 100, 12, 100_000);
        let mut cursor = Cursor::new(&data);
        let summary = parse_iostore_toc_summary(&mut cursor, data.len() as u64).unwrap();
        assert_eq!(summary.toc_version, 8);
        assert_eq!(summary.entry_count, 500);
        assert_eq!(summary.compressed_block_entry_count, 100);
    }

    #[test]
    fn test_rejects_invalid_magic() {
        let data = build_iostore_header(*b"INVALID_MAGIC_!!", 6, 144, 10, 5, 12, 1024);
        let mut cursor = Cursor::new(&data);
        assert_eq!(
            parse_iostore_toc_summary(&mut cursor, data.len() as u64),
            Err(IoStoreTocError::InvalidMagic)
        );
    }

    #[test]
    fn test_rejects_invalid_header_size() {
        let data = build_iostore_header(IOSTORE_TOC_MAGIC, 6, 128, 10, 5, 12, 1024);
        let mut cursor = Cursor::new(&data);
        assert_eq!(
            parse_iostore_toc_summary(&mut cursor, data.len() as u64),
            Err(IoStoreTocError::InvalidHeaderSize(128))
        );
    }

    #[test]
    fn test_rejects_invalid_block_entry_size() {
        let data = build_iostore_header(IOSTORE_TOC_MAGIC, 6, 144, 10, 5, 16, 1024);
        let mut cursor = Cursor::new(&data);
        assert_eq!(
            parse_iostore_toc_summary(&mut cursor, data.len() as u64),
            Err(IoStoreTocError::InvalidBlockEntrySize(16))
        );
    }

    #[test]
    fn test_rejects_truncated_file() {
        let data = vec![0u8; 100];
        let mut cursor = Cursor::new(&data);
        assert_eq!(
            parse_iostore_toc_summary(&mut cursor, data.len() as u64),
            Err(IoStoreTocError::TruncatedHeader)
        );
    }

    #[test]
    fn test_rejects_header_declaring_tables_beyond_eof() {
        // Physically 144 bytes header, but declares non-zero tables (min required: 144 + 120 + 100 + 60 = 424)
        let data = build_iostore_header(IOSTORE_TOC_MAGIC, 6, 144, 10, 5, 12, 144);
        let mut cursor = Cursor::new(&data);
        assert_eq!(
            parse_iostore_toc_summary(&mut cursor, data.len() as u64),
            Err(IoStoreTocError::SanityCheckFailed(
                "declared tables exceed file length"
            ))
        );
    }

    #[test]
    fn test_rejects_header_with_oversized_directory_index_or_compression_methods() {
        // Zero chunks/blocks, but directory_index_size = 5000 in a 144-byte file
        let mut data = build_iostore_header(IOSTORE_TOC_MAGIC, 6, 144, 0, 0, 12, 144);
        data[48..52].copy_from_slice(&5000u32.to_le_bytes());
        let mut cursor = Cursor::new(&data);
        assert_eq!(
            parse_iostore_toc_summary(&mut cursor, data.len() as u64),
            Err(IoStoreTocError::SanityCheckFailed(
                "declared tables exceed file length"
            ))
        );

        // Zero chunks/blocks, but compression_method_name_count = 10, length = 32 (320 bytes) in 144-byte file
        let mut data2 = build_iostore_header(IOSTORE_TOC_MAGIC, 6, 144, 0, 0, 12, 144);
        data2[36..40].copy_from_slice(&10u32.to_le_bytes());
        data2[40..44].copy_from_slice(&32u32.to_le_bytes());
        let mut cursor2 = Cursor::new(&data2);
        assert_eq!(
            parse_iostore_toc_summary(&mut cursor2, data2.len() as u64),
            Err(IoStoreTocError::SanityCheckFailed(
                "declared tables exceed file length"
            ))
        );

        // Version 6 with perfect hash seeds declaring 100 seeds (400 bytes) in 144-byte file
        let mut data3 = build_iostore_header(IOSTORE_TOC_MAGIC, 6, 144, 0, 0, 12, 144);
        data3[84..88].copy_from_slice(&100u32.to_le_bytes());
        let mut cursor3 = Cursor::new(&data3);
        assert_eq!(
            parse_iostore_toc_summary(&mut cursor3, data3.len() as u64),
            Err(IoStoreTocError::SanityCheckFailed(
                "declared tables exceed file length"
            ))
        );
    }

    #[test]
    fn test_rejects_table_size_exceeding_file_len() {
        // Entry count 10_000 * 22 = 220_000 > 500
        let data = build_iostore_header(IOSTORE_TOC_MAGIC, 6, 144, 10_000, 5, 12, 500);
        let mut cursor = Cursor::new(&data);
        assert_eq!(
            parse_iostore_toc_summary(&mut cursor, data.len() as u64),
            Err(IoStoreTocError::SanityCheckFailed(
                "declared tables exceed file length"
            ))
        );
    }

    #[test]
    fn test_large_counts_exceed_file_bounds() {
        let data = build_iostore_header(IOSTORE_TOC_MAGIC, 6, 144, u32::MAX, u32::MAX, 12, 1024);
        let mut cursor = Cursor::new(&data);
        assert_eq!(
            parse_iostore_toc_summary(&mut cursor, data.len() as u64),
            Err(IoStoreTocError::SanityCheckFailed(
                "declared tables exceed file length"
            ))
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
        let res = parse_iostore_toc_summary(&mut failing, 1000);
        match res {
            Err(IoStoreTocError::Io { kind, message }) => {
                assert_eq!(kind, io::ErrorKind::PermissionDenied);
                assert!(message.contains("permission denied"));
            }
            other => panic!("expected IoStoreTocError::Io, got {:?}", other),
        }
    }

    #[test]
    fn test_rejects_header_missing_chunk_metas_table() {
        // Declares 10 entries (120 + 100 = 220 bytes) and 5 blocks (60 bytes).
        // Without chunk metadata (10 * 33 = 330 bytes), header + IDs + offsets + blocks = 424 bytes.
        // A file of size 424 bytes physically contains all tables up to blocks, but lacks chunk metadata.
        let data = build_iostore_header(IOSTORE_TOC_MAGIC, 6, 144, 10, 5, 12, 424);
        let mut cursor = Cursor::new(&data);
        assert_eq!(
            parse_iostore_toc_summary(&mut cursor, data.len() as u64),
            Err(IoStoreTocError::SanityCheckFailed(
                "declared tables exceed file length"
            ))
        );

        // Once the file length accommodates chunk metadata (144 + 120 + 100 + 60 + 330 = 754 bytes), it passes:
        let valid_data = build_iostore_header(IOSTORE_TOC_MAGIC, 6, 144, 10, 5, 12, 754);
        let mut valid_cursor = Cursor::new(&valid_data);
        assert!(parse_iostore_toc_summary(&mut valid_cursor, valid_data.len() as u64).is_ok());
    }

    fn build_signed_iostore_toc(
        version: u8,
        entry_count: u32,
        block_count: u32,
        signature_size: u32,
    ) -> Vec<u8> {
        let chunk_id_size = (entry_count as usize) * 12;
        let chunk_offset_size = (entry_count as usize) * 10;
        let block_table_size = (block_count as usize) * 12;
        let sig_offset = 144 + chunk_id_size + chunk_offset_size + block_table_size;
        let signed_section_size = 4 + 2 * (signature_size as usize) + (block_count as usize) * 20;
        let meta_size = (if version >= 8 { 24 } else { 33 }) * (entry_count as usize);
        let total_size = sig_offset + signed_section_size + meta_size;

        let mut buf = vec![0u8; total_size];
        buf[0..16].copy_from_slice(&IOSTORE_TOC_MAGIC);
        buf[16] = version;
        buf[20..24].copy_from_slice(&144u32.to_le_bytes());
        buf[24..28].copy_from_slice(&entry_count.to_le_bytes());
        buf[28..32].copy_from_slice(&block_count.to_le_bytes());
        buf[32..36].copy_from_slice(&12u32.to_le_bytes());
        buf[80] = 0x04; // ContainerFlags: Signed
        buf[sig_offset..sig_offset + 4].copy_from_slice(&signature_size.to_le_bytes());
        buf
    }

    #[test]
    fn test_signed_iostore_toc_cuts_rejected_and_full_accepted() {
        let entry_count = 2u32;
        let block_count = 5u32;
        let signature_size = 64u32;
        let full_data = build_signed_iostore_toc(8, entry_count, block_count, signature_size);
        let total_len = full_data.len();
        let sig_offset = 144
            + (entry_count as usize) * 12
            + (entry_count as usize) * 10
            + (block_count as usize) * 12;

        // Cut 1: Immediately after regular tables (before signatures section)
        let cut1_len = sig_offset;
        let res1 = parse_iostore_toc_summary_bytes(&full_data[..cut1_len], cut1_len as u64);
        assert_eq!(
            res1,
            Err(IoStoreTocError::SanityCheckFailed(
                "declared tables exceed file length"
            )),
            "Cut right after regular tables must be rejected"
        );

        // Cut 2: After block_count * 20, but missing the two signature blobs
        let cut2_len = sig_offset + 4 + (block_count as usize) * 20;
        let res2 = parse_iostore_toc_summary_bytes(&full_data[..cut2_len], cut2_len as u64);
        assert_eq!(
            res2,
            Err(IoStoreTocError::SanityCheckFailed(
                "declared tables exceed file length"
            )),
            "Cut after block_count * 20 missing signature blobs must be rejected"
        );

        // Cut 3: Exactly 1 byte before full min_required_file_len
        let cut3_len = total_len - 1;
        let res3 = parse_iostore_toc_summary_bytes(&full_data[..cut3_len], cut3_len as u64);
        assert_eq!(
            res3,
            Err(IoStoreTocError::SanityCheckFailed(
                "declared tables exceed file length"
            )),
            "Cut 1 byte before min_required_file_len must be rejected"
        );

        // Valid: Full signed TOC passes validation
        let res_full = parse_iostore_toc_summary_bytes(&full_data, total_len as u64);
        assert!(res_full.is_ok(), "Full signed TOC must pass validation");
        let summary = res_full.unwrap();
        assert_eq!(summary.container_flags, 0x04);
        assert_eq!(summary.entry_count, entry_count);
        assert_eq!(summary.compressed_block_entry_count, block_count);
    }
}
