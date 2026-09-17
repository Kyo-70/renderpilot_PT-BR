//! Streaming PE section scanner with chunk overlap and byte budget management.

use std::io::{self, Read, Seek, SeekFrom};

use super::section_locator::{IMAGE_SCN_MEM_EXECUTE, IMAGE_SCN_MEM_READ, PeSectionHeader};

/// Stream buffer chunk size (64 KiB).
pub const STREAM_CHUNK_SIZE: usize = 64 * 1024; // 64 KiB
/// Overlap buffer size between chunks (1 KiB).
pub const OVERLAP_SIZE: usize = 1024; // 1 KiB

/// Checks for excluded sections: `.rsrc` (resources) and `.reloc` (relocations).
/// Scanning `.rsrc` is prohibited by Non-Negotiable #8 (generic VERSIONINFO excluded),
/// and `.reloc` contains integer relocation fixup tables without strings.
pub fn is_excluded_version_section(name: &str) -> bool {
    name.eq_ignore_ascii_case(".rsrc") || name.eq_ignore_ascii_case(".reloc")
}

/// Deterministic section selection for standard scan (readable initialized data, non-executable, excluding .rsrc/.reloc).
pub fn normal_version_sections(sections: &[PeSectionHeader]) -> Vec<&PeSectionHeader> {
    let mut prioritized = Vec::new();
    // Tier 1: .rdata/.rodata read-only
    for sec in sections {
        if is_excluded_version_section(&sec.name) {
            continue;
        }
        let is_rdata_or_rodata =
            sec.name.eq_ignore_ascii_case(".rdata") || sec.name.eq_ignore_ascii_case(".rodata");
        if is_rdata_or_rodata && sec.is_readonly_initialized_data() {
            prioritized.push(sec);
        }
    }
    // Tier 2: other read-only initialized data (excluding .rdata/.rodata, .data, and .rsrc/.reloc)
    for sec in sections {
        if is_excluded_version_section(&sec.name) {
            continue;
        }
        let is_rdata_or_rodata =
            sec.name.eq_ignore_ascii_case(".rdata") || sec.name.eq_ignore_ascii_case(".rodata");
        if !is_rdata_or_rodata && sec.is_readonly_initialized_data() {
            prioritized.push(sec);
        }
    }
    // Tier 3: .data and writable initialized data (excluding .rsrc/.reloc)
    for sec in sections {
        if is_excluded_version_section(&sec.name) {
            continue;
        }
        if sec.is_writable_initialized_data() {
            prioritized.push(sec);
        }
    }
    prioritized
}

/// Fallback readable non-executable sections (strictly excluding .rsrc and .reloc).
pub fn fallback_version_sections(sections: &[PeSectionHeader]) -> Vec<&PeSectionHeader> {
    sections
        .iter()
        .filter(|sec| {
            !is_excluded_version_section(&sec.name)
                && (sec.characteristics & IMAGE_SCN_MEM_READ != 0)
                && (sec.characteristics & IMAGE_SCN_MEM_EXECUTE == 0)
                && !sec.is_readonly_initialized_data()
                && !sec.is_writable_initialized_data()
        })
        .collect()
}

/// Iterates executable fallback sections (.text etc.) excluding .rsrc and .reloc.
pub fn executable_fallback_sections(
    sections: &[PeSectionHeader],
) -> impl Iterator<Item = &PeSectionHeader> {
    sections.iter().filter(|sec| {
        !is_excluded_version_section(&sec.name)
            && (sec.characteristics & IMAGE_SCN_MEM_READ != 0)
            && (sec.characteristics & IMAGE_SCN_MEM_EXECUTE != 0)
    })
}

/// Unified deterministic list of all candidate sections.
///
/// INVARIANT: When presence proof is established, executable fallback sections (.text)
/// are included alongside data sections to uncover hidden cross-section conflicts.
pub fn prioritized_candidate_sections(
    sections: &[PeSectionHeader],
    has_presence_proof: bool,
) -> Vec<&PeSectionHeader> {
    let mut all = normal_version_sections(sections);
    all.extend(fallback_version_sections(sections));
    if has_presence_proof {
        all.extend(executable_fallback_sections(sections));
    }
    all
}

/// Terminal condition of section streaming.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanTerminalCondition {
    /// Section was scanned to EOF.
    Completed,
    /// Byte budget was exhausted before EOF.
    BudgetExhausted,
    /// Consumer stopped early.
    ConsumerStopped,
    /// File was truncated before section end.
    TruncatedFile,
}

/// Statistics and coverage of a section scan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionScanCoverage {
    /// Name of the section scanned.
    pub section_name: String,
    /// Total raw size of section on disk.
    pub total_raw_size: u64,
    /// Actual bytes scanned within budget.
    pub actual_scanned_bytes: u64,
    /// Terminal condition of the scan.
    pub terminal_condition: ScanTerminalCondition,
}

/// Universal streaming section scanner.
pub fn scan_section_streaming<R: Read + Seek, F>(
    reader: &mut R,
    section: &PeSectionHeader,
    bytes_budget: &mut u64,
    mut consumer: F,
) -> io::Result<SectionScanCoverage>
where
    F: FnMut(&[u8], u64, bool) -> bool,
{
    let file_len = reader.seek(SeekFrom::End(0))?;
    let sec_offset = section.pointer_to_raw_data as u64;
    let sec_size = section.size_of_raw_data as u64;

    if sec_size == 0 {
        return Ok(SectionScanCoverage {
            section_name: section.name.clone(),
            total_raw_size: 0,
            actual_scanned_bytes: 0,
            terminal_condition: ScanTerminalCondition::Completed,
        });
    }

    if sec_offset >= file_len {
        return Ok(SectionScanCoverage {
            section_name: section.name.clone(),
            total_raw_size: sec_size,
            actual_scanned_bytes: 0,
            terminal_condition: ScanTerminalCondition::TruncatedFile,
        });
    }

    let available_in_file = sec_size.min(file_len - sec_offset);
    let mut scanned: u64 = 0;
    let mut buffer = vec![0u8; OVERLAP_SIZE + STREAM_CHUNK_SIZE];
    let mut overlap_len: usize = 0;
    let mut term = ScanTerminalCondition::Completed;

    reader.seek(SeekFrom::Start(sec_offset))?;

    while scanned < available_in_file {
        let to_read = ((available_in_file - scanned) as usize).min(STREAM_CHUNK_SIZE);
        let allowed = (to_read as u64).min(*bytes_budget) as usize;
        if allowed == 0 {
            term = ScanTerminalCondition::BudgetExhausted;
            break;
        }

        let read_slice = &mut buffer[overlap_len..overlap_len + allowed];
        reader.read_exact(read_slice)?;
        *bytes_budget -= allowed as u64;

        let current_chunk_file_offset = sec_offset + scanned;
        scanned += allowed as u64;
        // Invariant: If file is truncated, this is NOT a final chunk.
        let is_final_chunk = (available_in_file == sec_size) && (scanned >= sec_size);

        let total_valid = overlap_len + allowed;
        let window_offset = current_chunk_file_offset - (overlap_len as u64);
        if !consumer(&buffer[..total_valid], window_offset, is_final_chunk) {
            term = ScanTerminalCondition::ConsumerStopped;
            break;
        }

        let tail_len = total_valid.min(OVERLAP_SIZE);
        let tail_start = total_valid - tail_len;
        buffer.copy_within(tail_start..total_valid, 0);
        overlap_len = tail_len;
    }

    if scanned < sec_size
        && term == ScanTerminalCondition::Completed
        && available_in_file < sec_size
    {
        term = ScanTerminalCondition::TruncatedFile;
    }

    Ok(SectionScanCoverage {
        section_name: section.name.clone(),
        total_raw_size: sec_size,
        actual_scanned_bytes: scanned,
        terminal_condition: term,
    })
}

/// Specialized wrapper for version scanning protected by type system.
///
/// Consumer callback returns `()`, making early-stop impossible at compile time.
pub fn scan_version_section_all<R: Read + Seek, F>(
    reader: &mut R,
    section: &PeSectionHeader,
    bytes_budget: &mut u64,
    mut consumer: F,
) -> io::Result<SectionScanCoverage>
where
    F: FnMut(&[u8], u64, bool),
{
    scan_section_streaming(
        reader,
        section,
        bytes_budget,
        |chunk, offset, is_final_chunk| {
            consumer(chunk, offset, is_final_chunk);
            true // Always continue marker collection until end of section or budget exhaustion
        },
    )
}
