//! Canonical release marker parser (`++UE4+Release-` / `++UE5+Release-`) and installation-wide atomic scanner.

use std::io;
use std::path::PathBuf;

use renderpilot_detection::pe::{
    PeSectionHeader, ScanTerminalCondition, SectionScanCoverage, executable_fallback_sections,
    fallback_version_sections, normal_version_sections, scan_version_section_all,
};

use crate::addons::game_analysis::budget::AnalysisBudget;
use crate::addons::game_analysis::context::GameInstallationContext;
use crate::addons::game_analysis::evidence::ValidatedEvidence;
use crate::addons::game_analysis::parsers::tokens::{
    BoundedMarker, MAX_MARKER_LEN, ParsedCanonicalReleaseMarker,
};
use crate::addons::game_analysis::topology::executable::{
    BoundEngineHelper, BoundPrimaryExecutable,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoundaryCheckResult {
    Valid,
    SkipAlreadyScanned,
    InvalidLeftBoundary,
    InvalidRightBoundary,
    DeferredToNextChunk,
}

pub fn is_left_word_boundary(byte: u8) -> bool {
    !byte.is_ascii_alphanumeric() && byte != b'_'
}

pub fn is_right_word_boundary(byte: u8) -> bool {
    byte == 0
        || byte == b'\r'
        || byte == b'\n'
        || byte == b' '
        || byte == b'"'
        || byte == b'\''
        || byte == b','
        || byte == b';'
}

/// Validates word boundaries of a candidate marker in a streaming chunk.
pub fn validate_marker_boundaries(
    slice: &[u8],
    match_pos: usize,
    marker_len: usize,
    chunk_offset: u64,
    section_start: u64,
    is_final_chunk: bool,
) -> BoundaryCheckResult {
    // 1. Left boundary check
    if match_pos == 0 {
        if chunk_offset == section_start {
            // Absolute start of section in file - valid left boundary
        } else {
            // Overlap buffer start of subsequent chunk: candidate was fully checked in previous chunk
            return BoundaryCheckResult::SkipAlreadyScanned;
        }
    } else if !is_left_word_boundary(slice[match_pos - 1]) {
        return BoundaryCheckResult::InvalidLeftBoundary;
    }

    // 2. Right boundary check
    let end_pos = match_pos + marker_len;
    if end_pos < slice.len() {
        if !is_right_word_boundary(slice[end_pos]) {
            return BoundaryCheckResult::InvalidRightBoundary;
        }
    } else if is_final_chunk {
        // Physical EOF of section is a valid terminator
    } else {
        // Marker hits chunk boundary in non-final buffer: deferred to next chunk
        return BoundaryCheckResult::DeferredToNextChunk;
    }

    BoundaryCheckResult::Valid
}

/// Parsed release marker payload before role wrapping.
#[derive(Debug, Clone)]
pub struct ParsedReleaseMarkerPayload {
    pub file_offset: u64,
    pub major: u32,
    pub minor: u32,
    pub patch: Option<u32>,
    pub raw: BoundedMarker,
}

const UE4_PREFIX: &[u8] = b"++UE4+Release-";
const UE5_PREFIX: &[u8] = b"++UE5+Release-";
const RELEASE_PREFIX_LEN: usize = UE4_PREFIX.len();

const UE4_PREFIX_WIDE: &[u8] = b"+\0+\0U\0E\x004\0+\0R\0e\0l\0e\0a\0s\0e\0-\0";
const UE5_PREFIX_WIDE: &[u8] = b"+\0+\0U\0E\x005\0+\0R\0e\0l\0e\0a\0s\0e\0-\0";

/// Validates word boundaries of a UTF-16LE candidate marker in a streaming chunk.
pub fn validate_wide_marker_boundaries(
    slice: &[u8],
    match_pos: usize,
    wide_marker_len: usize,
    chunk_offset: u64,
    section_start: u64,
    is_final_chunk: bool,
) -> BoundaryCheckResult {
    // 1. Left boundary check
    if match_pos < 2 {
        if chunk_offset == section_start {
            if match_pos == 1 {
                return BoundaryCheckResult::InvalidLeftBoundary;
            }
            // Absolute start of section in file - valid left boundary
        } else {
            // Overlap buffer start of subsequent chunk: candidate was fully checked in previous chunk
            return BoundaryCheckResult::SkipAlreadyScanned;
        }
    } else {
        let low = slice[match_pos - 2];
        let high = slice[match_pos - 1];
        if high != 0 || !is_left_word_boundary(low) {
            return BoundaryCheckResult::InvalidLeftBoundary;
        }
    }

    // 2. Right boundary check
    let end_pos = match_pos + wide_marker_len;
    if end_pos + 1 < slice.len() {
        let low = slice[end_pos];
        let high = slice[end_pos + 1];
        if high != 0 || !is_right_word_boundary(low) {
            return BoundaryCheckResult::InvalidRightBoundary;
        }
    } else if end_pos == slice.len() {
        if !is_final_chunk {
            return BoundaryCheckResult::DeferredToNextChunk;
        }
    } else if !is_final_chunk {
        return BoundaryCheckResult::DeferredToNextChunk;
    } else {
        return BoundaryCheckResult::InvalidRightBoundary;
    }

    BoundaryCheckResult::Valid
}

/// Parsed version suffix following a canonical release marker prefix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParsedVersionSuffix {
    /// Number of ASCII bytes consumed by the version suffix.
    pub parsed_len: usize,
    pub major: u32,
    pub minor: u32,
    pub patch: Option<u32>,
}

/// Parses `<Major>.<Minor>[.<Patch>][-CL-<Changelist>]` from an ASCII byte slice.
pub fn parse_marker_version_suffix(slice: &[u8], expected_gen: u32) -> Option<ParsedVersionSuffix> {
    let mut idx = 0;

    // Parse <Major>
    let major_start = idx;
    while idx < slice.len() && slice[idx].is_ascii_digit() {
        idx += 1;
    }
    if idx == major_start {
        return None;
    }
    let major_str = std::str::from_utf8(&slice[major_start..idx]).ok()?;
    let major: u32 = major_str.parse().ok()?;
    if major != expected_gen {
        // Major must strictly equal prefix generation
        return None;
    }

    // Expect '.'
    if idx >= slice.len() || slice[idx] != b'.' {
        return None;
    }
    idx += 1;

    // Parse <Minor> (1 to 3 digits)
    let minor_start = idx;
    while idx < slice.len() && slice[idx].is_ascii_digit() {
        idx += 1;
    }
    let minor_digits = idx - minor_start;
    if minor_digits == 0 || minor_digits > 3 {
        return None;
    }
    let minor_str = std::str::from_utf8(&slice[minor_start..idx]).ok()?;
    let minor: u32 = minor_str.parse().ok()?;
    if minor > 999 {
        return None;
    }

    // Optional .<Patch> (1 to 2 digits, 0..=99)
    let mut patch: Option<u32> = None;
    if idx < slice.len() && slice[idx] == b'.' {
        idx += 1; // skip '.'
        let patch_start = idx;
        while idx < slice.len() && slice[idx].is_ascii_digit() {
            idx += 1;
        }
        let patch_digits = idx - patch_start;
        if !(1..=2).contains(&patch_digits) {
            // If '.' is present after minor, it MUST be followed by exactly 1 to 2 digits.
            // 0 digits or >= 3 digits violates the release marker grammar.
            return None;
        }
        let patch_str = std::str::from_utf8(&slice[patch_start..idx]).ok()?;
        let p: u32 = patch_str.parse().ok()?;
        if p > 99 {
            return None;
        }
        patch = Some(p);
    }

    // Optional -CL-<Changelist> (at least 1 digit)
    if idx + 4 <= slice.len() && &slice[idx..idx + 4] == b"-CL-" {
        idx += 4;
        let cl_start = idx;
        while idx < slice.len() && slice[idx].is_ascii_digit() {
            idx += 1;
        }
        let cl_digits = idx - cl_start;
        if cl_digits == 0 {
            // -CL- must be followed by at least 1 digit.
            return None;
        }
    }

    Some(ParsedVersionSuffix {
        parsed_len: idx,
        major,
        minor,
        patch,
    })
}

/// Attempts to match and parse an ASCII canonical release marker at `match_pos`.
fn try_match_ascii_marker(
    slice: &[u8],
    match_pos: usize,
    chunk_offset: u64,
    section_start: u64,
    is_final_chunk: bool,
) -> Option<(ParsedReleaseMarkerPayload, usize)> {
    let remaining = &slice[match_pos..];
    let (expected_gen, prefix_len) = if remaining.starts_with(UE4_PREFIX) {
        (4u32, UE4_PREFIX.len())
    } else if remaining.starts_with(UE5_PREFIX) {
        (5u32, UE5_PREFIX.len())
    } else {
        return None;
    };

    let after_prefix = &slice[match_pos + prefix_len..];
    let suffix = parse_marker_version_suffix(after_prefix, expected_gen)?;

    let marker_len = prefix_len + suffix.parsed_len;
    if marker_len > MAX_MARKER_LEN {
        return None;
    }

    if validate_marker_boundaries(
        slice,
        match_pos,
        marker_len,
        chunk_offset,
        section_start,
        is_final_chunk,
    ) != BoundaryCheckResult::Valid
    {
        return None;
    }

    let raw = BoundedMarker::try_from_bytes(&slice[match_pos..match_pos + marker_len])?;
    let file_offset = chunk_offset + match_pos as u64;
    Some((
        ParsedReleaseMarkerPayload {
            file_offset,
            major: suffix.major,
            minor: suffix.minor,
            patch: suffix.patch,
            raw,
        },
        marker_len,
    ))
}

/// Attempts to match and parse a UTF-16LE canonical release marker at `match_pos`.
fn try_match_wide_marker(
    slice: &[u8],
    match_pos: usize,
    chunk_offset: u64,
    section_start: u64,
    is_final_chunk: bool,
) -> Option<(ParsedReleaseMarkerPayload, usize)> {
    let remaining = &slice[match_pos..];
    let (expected_gen, prefix_len, ascii_prefix) = if remaining.starts_with(UE4_PREFIX_WIDE) {
        (4u32, UE4_PREFIX_WIDE.len(), UE4_PREFIX)
    } else if remaining.starts_with(UE5_PREFIX_WIDE) {
        (5u32, UE5_PREFIX_WIDE.len(), UE5_PREFIX)
    } else {
        return None;
    };

    let after_prefix_wide = &remaining[prefix_len..];

    // Decode candidate ASCII characters from UTF-16LE code units (up to MAX_MARKER_LEN - RELEASE_PREFIX_LEN)
    let mut ascii_buf = [0u8; MAX_MARKER_LEN - RELEASE_PREFIX_LEN];
    let mut decoded_chars = 0;
    while decoded_chars < ascii_buf.len() && (decoded_chars * 2 + 1) < after_prefix_wide.len() {
        let low = after_prefix_wide[decoded_chars * 2];
        let high = after_prefix_wide[decoded_chars * 2 + 1];
        if high != 0 || !low.is_ascii() {
            break;
        }
        ascii_buf[decoded_chars] = low;
        decoded_chars += 1;
    }

    let suffix = parse_marker_version_suffix(&ascii_buf[..decoded_chars], expected_gen)?;

    let wide_marker_len = prefix_len + suffix.parsed_len * 2;
    if validate_wide_marker_boundaries(
        slice,
        match_pos,
        wide_marker_len,
        chunk_offset,
        section_start,
        is_final_chunk,
    ) != BoundaryCheckResult::Valid
    {
        return None;
    }

    let mut raw_bytes = [0u8; MAX_MARKER_LEN];
    raw_bytes[..RELEASE_PREFIX_LEN].copy_from_slice(ascii_prefix);
    raw_bytes[RELEASE_PREFIX_LEN..RELEASE_PREFIX_LEN + suffix.parsed_len]
        .copy_from_slice(&ascii_buf[..suffix.parsed_len]);
    let raw_len = RELEASE_PREFIX_LEN + suffix.parsed_len;
    let raw = BoundedMarker::try_from_bytes(&raw_bytes[..raw_len])?;
    let file_offset = chunk_offset + match_pos as u64;

    Some((
        ParsedReleaseMarkerPayload {
            file_offset,
            major: suffix.major,
            minor: suffix.minor,
            patch: suffix.patch,
            raw,
        },
        wide_marker_len,
    ))
}

/// Extracts canonical release markers from a raw chunk slice according to §5.3.1.
pub fn scan_chunk_for_release_markers(
    slice: &[u8],
    chunk_offset: u64,
    section_start: u64,
    is_final_chunk: bool,
    results: &mut Vec<ParsedReleaseMarkerPayload>,
) {
    let mut pos = 0;
    while pos < slice.len() {
        if let Some((payload, matched_len)) =
            try_match_ascii_marker(slice, pos, chunk_offset, section_start, is_final_chunk)
        {
            results.push(payload);
            pos += matched_len;
            continue;
        }

        if let Some((payload, matched_len)) =
            try_match_wide_marker(slice, pos, chunk_offset, section_start, is_final_chunk)
        {
            results.push(payload);
            pos += matched_len;
            continue;
        }

        pos += 1;
    }
}

/// Reason for incomplete release marker scan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MarkerScanIncomplete {
    BudgetExhausted {
        scanned_bytes: u64,
    },
    TruncatedFile {
        path: PathBuf,
    },
    IoError {
        kind: io::ErrorKind,
        message: String,
    },
    ContextMismatch,
}

impl From<io::Error> for MarkerScanIncomplete {
    fn from(err: io::Error) -> Self {
        Self::IoError {
            kind: err.kind(),
            message: err.to_string(),
        }
    }
}

/// Scans readable non-executable data sections (normal + fallback) for release markers.
fn scan_data_sections<R: io::Read + io::Seek>(
    reader: &mut R,
    sections: &[PeSectionHeader],
    exe_path: &std::path::Path,
    budget: &mut AnalysisBudget,
    coverage: &mut Vec<SectionScanCoverage>,
) -> Result<Vec<ParsedReleaseMarkerPayload>, MarkerScanIncomplete> {
    let mut results = Vec::new();
    let mut phase1_sections: Vec<&PeSectionHeader> = normal_version_sections(sections);
    phase1_sections.extend(fallback_version_sections(sections));

    for sec in phase1_sections {
        let sec_offset = sec.pointer_to_raw_data as u64;
        let cov = scan_version_section_all(
            reader,
            sec,
            &mut budget.section_stream_bytes_remaining,
            |chunk, offset, is_final| {
                scan_chunk_for_release_markers(chunk, offset, sec_offset, is_final, &mut results);
            },
        )?;
        let term = cov.terminal_condition;
        coverage.push(cov);

        match term {
            ScanTerminalCondition::BudgetExhausted => {
                let scanned_bytes = coverage.iter().map(|c| c.actual_scanned_bytes).sum::<u64>();
                return Err(MarkerScanIncomplete::BudgetExhausted { scanned_bytes });
            }
            ScanTerminalCondition::TruncatedFile => {
                return Err(MarkerScanIncomplete::TruncatedFile {
                    path: exe_path.to_path_buf(),
                });
            }
            ScanTerminalCondition::Completed | ScanTerminalCondition::ConsumerStopped => {}
        }
    }

    Ok(results)
}

/// Scans executable fallback sections (.text).
///
/// SAFETY: No name-based deduplication is performed. Multiple candidate executable
/// sections (e.g. malformed or packed PEs with multiple `.text` sections) are all scanned
/// to ensure no cross-section conflicts are masked.
fn scan_executable_sections<R: io::Read + io::Seek>(
    reader: &mut R,
    sections: &[PeSectionHeader],
    exe_path: &std::path::Path,
    budget: &mut AnalysisBudget,
    coverage: &mut Vec<SectionScanCoverage>,
) -> Result<Vec<ParsedReleaseMarkerPayload>, MarkerScanIncomplete> {
    let mut results = Vec::new();

    for sec in executable_fallback_sections(sections) {
        let sec_offset = sec.pointer_to_raw_data as u64;
        let cov = scan_version_section_all(
            reader,
            sec,
            &mut budget.section_stream_bytes_remaining,
            |chunk, offset, is_final| {
                scan_chunk_for_release_markers(chunk, offset, sec_offset, is_final, &mut results);
            },
        )?;
        let term = cov.terminal_condition;
        coverage.push(cov);

        match term {
            ScanTerminalCondition::BudgetExhausted => {
                let scanned_bytes = coverage.iter().map(|c| c.actual_scanned_bytes).sum::<u64>();
                return Err(MarkerScanIncomplete::BudgetExhausted { scanned_bytes });
            }
            ScanTerminalCondition::TruncatedFile => {
                return Err(MarkerScanIncomplete::TruncatedFile {
                    path: exe_path.to_path_buf(),
                });
            }
            ScanTerminalCondition::Completed | ScanTerminalCondition::ConsumerStopped => {}
        }
    }

    Ok(results)
}

/// Installation-wide atomic release marker scanning via RAII Result semantics.
///
/// Data & Executable Section Scan:
/// - Data sections of Primary and all Helpers are scanned first.
///   Strictly fail-closed: if any data-section scan hits BudgetExhausted, TruncatedFile,
///   or IoError, immediately returns Err and no evidence is committed.
///   Upon completion of data sections, its verified evidence is committed.
/// - Supplemental Executable Sections: Executable sections (.text) of Primary and all Helpers
///   are scanned if initial presence proof was established OR any data-section marker was found.
///   Uses the remaining shared budget.
///   - If executable section scan completes successfully: results are merged with data-section evidence.
///   - If executable section scan encounters BudgetExhausted: only its partial results are discarded,
///     while confirmed data-section evidence is preserved.
///   - If executable section scan encounters TruncatedFile or IoError: returns Err (fail-closed).
pub fn scan_installation_release_markers<'game>(
    context: &'game GameInstallationContext,
    mut primary: Option<&mut BoundPrimaryExecutable<'game>>,
    helpers: &mut [BoundEngineHelper<'game>],
    budget: &mut AnalysisBudget,
    coverage: &mut Vec<SectionScanCoverage>,
    has_presence_proof: bool,
) -> Result<Vec<ValidatedEvidence<'game>>, MarkerScanIncomplete> {
    if primary
        .as_ref()
        .is_some_and(|p| p.context().id() != context.id())
    {
        return Err(MarkerScanIncomplete::ContextMismatch);
    }
    if helpers.iter().any(|h| h.context().id() != context.id()) {
        return Err(MarkerScanIncomplete::ContextMismatch);
    }

    // 1. Installation-wide scan of data sections across Primary and all Helpers
    // Strictly fail-closed: ? immediately returns Err on BudgetExhausted, TruncatedFile, or IoError.
    let mut primary_payloads = Vec::new();
    if let Some(ref mut primary_scan) = primary {
        let (file, _, sections, exe_path) = primary_scan.scan_parts();
        let payloads = scan_data_sections(file, sections, exe_path, budget, coverage)?;
        primary_payloads.extend(payloads);
    }

    let mut helper_payloads: Vec<Vec<ParsedReleaseMarkerPayload>> =
        Vec::with_capacity(helpers.len());
    for helper in helpers.iter_mut() {
        let (file, _, sections, exe_path) = helper.scan_parts();
        let payloads = scan_data_sections(file, sections, exe_path, budget, coverage)?;
        helper_payloads.push(payloads);
    }

    // 2. Supplemental scan of executable sections (.text)
    let has_data_section_marker =
        !primary_payloads.is_empty() || helper_payloads.iter().any(|m| !m.is_empty());
    let should_scan_executable_sections = has_presence_proof || has_data_section_marker;

    if should_scan_executable_sections {
        let mut primary_exec_payloads = Vec::new();
        let mut helper_exec_payloads: Vec<Vec<ParsedReleaseMarkerPayload>> =
            Vec::with_capacity(helpers.len());

        let exec_scan_result: Result<(), MarkerScanIncomplete> = (|| {
            if let Some(ref mut primary_scan) = primary {
                let (file, _, sections, exe_path) = primary_scan.scan_parts();
                let exec_payloads =
                    scan_executable_sections(file, sections, exe_path, budget, coverage)?;
                primary_exec_payloads.extend(exec_payloads);
            }

            for helper in helpers.iter_mut() {
                let (file, _, sections, exe_path) = helper.scan_parts();
                let exec_payloads =
                    scan_executable_sections(file, sections, exe_path, budget, coverage)?;
                helper_exec_payloads.push(exec_payloads);
            }
            Ok(())
        })();

        match exec_scan_result {
            Ok(()) => {
                primary_payloads.extend(primary_exec_payloads);
                for (committed, scanned) in helper_payloads.iter_mut().zip(helper_exec_payloads) {
                    committed.extend(scanned);
                }
            }
            Err(MarkerScanIncomplete::BudgetExhausted { .. }) => {
                // Executable-section scan is supplemental. On budget exhaustion, discard its partial results while preserving confirmed data-section evidence.
            }
            Err(
                e @ (MarkerScanIncomplete::TruncatedFile { .. }
                | MarkerScanIncomplete::IoError { .. }
                | MarkerScanIncomplete::ContextMismatch),
            ) => {
                return Err(e);
            }
        }
    }

    // Stage committed evidence into output buffer
    let mut staged = Vec::new();
    if let Some(primary_scan) = primary {
        for payload in primary_payloads {
            let marker = ParsedCanonicalReleaseMarker::from_primary(
                primary_scan,
                payload.file_offset,
                payload.major,
                payload.minor,
                payload.patch,
                payload.raw,
            );
            staged.push(ValidatedEvidence::from_primary_marker(marker));
        }
    }

    for (helper, payloads) in helpers.iter().zip(helper_payloads) {
        for payload in payloads {
            let marker = ParsedCanonicalReleaseMarker::from_helper(
                helper,
                payload.file_offset,
                payload.major,
                payload.minor,
                payload.patch,
                payload.raw,
            );
            staged.push(ValidatedEvidence::from_helper_marker(marker));
        }
    }

    if staged.iter().any(|e| e.installation_id() != context.id()) {
        return Err(MarkerScanIncomplete::ContextMismatch);
    }

    Ok(staged)
}
