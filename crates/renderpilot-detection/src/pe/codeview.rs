//! CodeView (RSDS) Debug Directory parser with multi-entry extraction and fail-closed bounds.

use std::io::{self, Read, Seek, SeekFrom};

use super::section_locator::{PeHeaderInfo, PeSectionHeader};

/// Maximum allowed debug directory size (64 KiB).
pub const MAX_DEBUG_DIRECTORY_BYTES: usize = 64 * 1024; // 64 KiB
/// Maximum allowed debug directory entries.
pub const MAX_DEBUG_ENTRIES: usize = 100;
/// Maximum allowed PDB path bytes.
pub const MAX_PDB_PATH_BYTES: usize = 1024;
const IMAGE_DIRECTORY_ENTRY_DEBUG: usize = 6;

/// Precise and safe conversion of [RVA, RVA + len) to file offset.
///
/// INVARIANT: Entire range must lie strictly within `size_of_raw_data`.
/// SAFETY INVARIANT: If the range matches multiple sections in a malformed PE,
/// returns `None` to prevent mapping ambiguity.
pub fn rva_to_file_range(
    rva: u32,
    len: usize,
    sections: &[PeSectionHeader],
    file_len: u64,
) -> Option<u64> {
    let len_u64 = len as u64;
    let mut matched_offset: Option<u64> = None;

    for sec in sections {
        if rva < sec.virtual_address {
            continue;
        }
        let delta = (rva - sec.virtual_address) as u64;
        let within_raw = delta
            .checked_add(len_u64)
            .is_some_and(|needed_raw| needed_raw <= sec.size_of_raw_data as u64);
        if within_raw {
            let file_offset = (sec.pointer_to_raw_data as u64).checked_add(delta)?;
            if file_offset
                .checked_add(len_u64)
                .is_some_and(|end| end <= file_len)
            {
                if matched_offset.is_some() {
                    // Ambiguity: range overlaps more than one section. Fail-closed.
                    return None;
                }
                matched_offset = Some(file_offset);
            }
        }
    }
    matched_offset
}

/// Extracted CodeView PDB path and file offset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeViewPdbResult {
    /// Sanitized basename of the PDB.
    pub pdb_path: String,
    /// File offset where the PDB record starts.
    pub file_offset: u64,
}

/// Extraction status of CodeView debug directories.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodeViewExtractionStatus {
    /// Extraction succeeded.
    Success,
    /// Debug directory is malformed.
    MalformedDirectory {
        /// Malformed directory size.
        size: usize,
    },
    /// Too many debug entries in directory.
    TooManyDebugEntries {
        /// Actual number of entries found.
        actual_entries: usize,
        /// Maximum allowed entries.
        max_entries: usize,
    },
    /// Debug directory size exceeds limit.
    DebugDirectoryExceedsLimit {
        /// Actual byte size of directory.
        actual_bytes: usize,
        /// Maximum allowed byte size.
        max_bytes: usize,
    },
    /// RVA cannot be mapped to physical file range.
    UnmappableRva {
        /// Unmappable RVA.
        rva: u32,
        /// Size of the data.
        size: u32,
    },
    /// No debug directory present in PE.
    NoDebugDirectory,
    /// No CodeView (RSDS) entries found in debug directory.
    NoCodeViewEntries,
}

/// Safely extracts the PDB basename without leaking build machine directory structures.
///
/// Supports both forward (`/`) and backward (`\`) slashes.
/// If extraction fails or the result contains slashes, returns `None` (fail-closed).
pub fn extract_pdb_basename(raw_path: &str) -> Option<&str> {
    let trimmed = raw_path.trim();
    if trimmed.is_empty() {
        return None;
    }
    let last_sep = trimmed.rfind(['/', '\\']);
    let basename = match last_sep {
        Some(pos) => &trimmed[pos + 1..],
        None => trimmed,
    };
    if basename.is_empty() || basename.contains('/') || basename.contains('\\') {
        return None;
    }
    Some(basename)
}

/// Multi-entry Debug Directory parser.
///
/// Extracts all valid CodeView (RSDS) entries within limits.
/// SAFETY INVARIANT: If `debug_size % 28 != 0`, exceeds 64 KiB, or total entries > 100,
/// the parser completely refuses extraction with an error status to prevent hiding conflicts.
pub fn extract_codeview_pdbs<R: Read + Seek>(
    reader: &mut R,
    header: &PeHeaderInfo,
) -> io::Result<(Vec<CodeViewPdbResult>, CodeViewExtractionStatus)> {
    if header.number_of_rva_and_sizes <= IMAGE_DIRECTORY_ENTRY_DEBUG as u32 {
        return Ok((Vec::new(), CodeViewExtractionStatus::NoDebugDirectory));
    }

    let file_len = reader.seek(SeekFrom::End(0))?;
    let data_dirs_offset = if header.is_64bit {
        header.optional_header_offset + 112
    } else {
        header.optional_header_offset + 96
    };

    let debug_dir_entry_offset = data_dirs_offset + (IMAGE_DIRECTORY_ENTRY_DEBUG as u64 * 8);
    if debug_dir_entry_offset + 8
        > header.optional_header_offset + header.optional_header_size as u64
    {
        return Ok((Vec::new(), CodeViewExtractionStatus::NoDebugDirectory));
    }

    let mut dir_buf = [0u8; 8];
    reader.seek(SeekFrom::Start(debug_dir_entry_offset))?;
    reader.read_exact(&mut dir_buf)?;

    let debug_va = u32::from_le_bytes(dir_buf[0..4].try_into().unwrap());
    let debug_size = u32::from_le_bytes(dir_buf[4..8].try_into().unwrap()) as usize;
    if debug_va == 0 || debug_size == 0 {
        return Ok((Vec::new(), CodeViewExtractionStatus::NoDebugDirectory));
    }

    if !debug_size.is_multiple_of(28) {
        return Ok((
            Vec::new(),
            CodeViewExtractionStatus::MalformedDirectory { size: debug_size },
        ));
    }

    if debug_size > MAX_DEBUG_DIRECTORY_BYTES {
        return Ok((
            Vec::new(),
            CodeViewExtractionStatus::DebugDirectoryExceedsLimit {
                actual_bytes: debug_size,
                max_bytes: MAX_DEBUG_DIRECTORY_BYTES,
            },
        ));
    }

    let total_entries = debug_size / 28;
    if total_entries > MAX_DEBUG_ENTRIES {
        return Ok((
            Vec::new(),
            CodeViewExtractionStatus::TooManyDebugEntries {
                actual_entries: total_entries,
                max_entries: MAX_DEBUG_ENTRIES,
            },
        ));
    }

    let debug_file_offset =
        match rva_to_file_range(debug_va, debug_size, &header.sections, file_len) {
            Some(offset) => offset,
            None => {
                return Ok((
                    Vec::new(),
                    CodeViewExtractionStatus::UnmappableRva {
                        rva: debug_va,
                        size: debug_size as u32,
                    },
                ));
            }
        };

    let mut entries_buf = vec![0u8; debug_size];
    reader.seek(SeekFrom::Start(debug_file_offset))?;
    reader.read_exact(&mut entries_buf)?;

    let mut results = Vec::with_capacity(total_entries);
    let mut cv_buf = Vec::with_capacity(MAX_PDB_PATH_BYTES);

    for entry in entries_buf.as_chunks::<28>().0 {
        let entry_type = u32::from_le_bytes(entry[12..16].try_into().unwrap());
        let size_of_data = u32::from_le_bytes(entry[16..20].try_into().unwrap());
        let address_of_raw_data = u32::from_le_bytes(entry[20..24].try_into().unwrap());
        let pointer_to_raw = u32::from_le_bytes(entry[24..28].try_into().unwrap());

        // Type 2 = IMAGE_DEBUG_TYPE_CODEVIEW
        if entry_type == 2 {
            if size_of_data < 24 || (size_of_data as usize) > MAX_DEBUG_DIRECTORY_BYTES {
                return Ok((
                    Vec::new(),
                    CodeViewExtractionStatus::MalformedDirectory {
                        size: size_of_data as usize,
                    },
                ));
            }
            if pointer_to_raw == 0 {
                return Ok((
                    Vec::new(),
                    CodeViewExtractionStatus::MalformedDirectory { size: 0 },
                ));
            }
            let raw_offset = pointer_to_raw as u64;
            if raw_offset
                .checked_add(size_of_data as u64)
                .is_none_or(|end| end > file_len)
            {
                return Ok((
                    Vec::new(),
                    CodeViewExtractionStatus::MalformedDirectory {
                        size: size_of_data as usize,
                    },
                ));
            }
            if address_of_raw_data != 0 {
                match rva_to_file_range(
                    address_of_raw_data,
                    size_of_data as usize,
                    &header.sections,
                    file_len,
                ) {
                    Some(mapped_offset) => {
                        if mapped_offset != raw_offset {
                            return Ok((
                                Vec::new(),
                                CodeViewExtractionStatus::UnmappableRva {
                                    rva: address_of_raw_data,
                                    size: size_of_data,
                                },
                            ));
                        }
                    }
                    None => {
                        return Ok((
                            Vec::new(),
                            CodeViewExtractionStatus::UnmappableRva {
                                rva: address_of_raw_data,
                                size: size_of_data,
                            },
                        ));
                    }
                }
            }

            let read_len = (size_of_data as usize).min(MAX_PDB_PATH_BYTES);
            cv_buf.resize(read_len, 0);
            reader.seek(SeekFrom::Start(raw_offset))?;
            reader.read_exact(&mut cv_buf)?;

            // Magic RSDS = 0x53445352
            if cv_buf.len() < 24 || &cv_buf[0..4] != b"RSDS" {
                continue;
            }

            let path_bytes = &cv_buf[24..];
            let nul_pos = match path_bytes.iter().position(|&b| b == 0) {
                Some(pos) => pos,
                None => continue,
            };

            let full_pdb = String::from_utf8_lossy(&path_bytes[..nul_pos]);
            if let Some(sanitized_basename) = extract_pdb_basename(&full_pdb) {
                results.push(CodeViewPdbResult {
                    pdb_path: sanitized_basename.to_string(),
                    file_offset: raw_offset,
                });
            }
        }
    }

    let status = if results.is_empty() {
        CodeViewExtractionStatus::NoCodeViewEntries
    } else {
        CodeViewExtractionStatus::Success
    };

    Ok((results, status))
}
