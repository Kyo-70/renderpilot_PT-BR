//! CodeView PDB path parser and scanner.

use std::io;

use renderpilot_detection::pe::{CodeViewExtractionStatus, extract_codeview_pdbs};

use crate::addons::game_analysis::parsers::tokens::{HelperRole, ParsedPdbPath, PrimaryRole};
use crate::addons::game_analysis::topology::executable::{
    BoundEngineHelper, BoundPrimaryExecutable,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeViewScanResult<T> {
    pub tokens: Vec<T>,
    pub status: CodeViewExtractionStatus,
}

/// Parses a sanitized PDB basename according to §5.3.2.
///
/// Matches pattern `UE_([45])\.([0-9]{1,2})` with strict left and right delimiters.
pub fn parse_pdb_version(basename: &str) -> Option<(u32, u32, String)> {
    let bytes = basename.as_bytes();
    let mut i = 0;
    while i + 5 <= bytes.len() {
        if &bytes[i..i + 3] == b"UE_" {
            // Left boundary: start of string or preceded by '_', '-', '.'
            let valid_left =
                i == 0 || bytes[i - 1] == b'_' || bytes[i - 1] == b'-' || bytes[i - 1] == b'.';
            if valid_left {
                let gen_char = bytes[i + 3];
                if (gen_char == b'4' || gen_char == b'5') && bytes.get(i + 4) == Some(&b'.') {
                    let major = if gen_char == b'4' { 4 } else { 5 };
                    let mut j = i + 5;
                    let minor_start = j;
                    while j < bytes.len() && bytes[j].is_ascii_digit() {
                        j += 1;
                    }
                    let minor_digits = j - minor_start;
                    if (1..=2).contains(&minor_digits) {
                        let valid_right = j == bytes.len()
                            || bytes[j] == b'.'
                            || bytes[j] == b'_'
                            || bytes[j] == b'-';
                        let minor_opt = if valid_right {
                            std::str::from_utf8(&bytes[minor_start..j])
                                .ok()
                                .and_then(|s| s.parse::<u32>().ok())
                        } else {
                            None
                        };
                        if let Some(minor) = minor_opt {
                            let fragment = format!("UE_{major}.{minor}");
                            return Some((major, minor, fragment));
                        }
                    }
                }
            }
        }
        i += 1;
    }
    None
}

/// Scans CodeView debug directory of the Primary executable.
pub fn scan_primary_codeview<'game>(
    source: &mut BoundPrimaryExecutable<'game>,
) -> io::Result<CodeViewScanResult<ParsedPdbPath<'game, PrimaryRole>>> {
    let (entries, status) = {
        let (file, header, _, _) = source.scan_parts();
        extract_codeview_pdbs(file, header)?
    };

    let mut tokens = Vec::new();
    for entry in entries {
        if let Some((major, minor, fragment)) = parse_pdb_version(&entry.pdb_path) {
            tokens.push(ParsedPdbPath::from_primary(
                source,
                entry.file_offset,
                major,
                minor,
                fragment,
            ));
        }
    }

    Ok(CodeViewScanResult { tokens, status })
}

/// Scans CodeView debug directory of an Engine Helper executable.
pub fn scan_helper_codeview<'game>(
    source: &mut BoundEngineHelper<'game>,
) -> io::Result<CodeViewScanResult<ParsedPdbPath<'game, HelperRole>>> {
    let (entries, status) = {
        let (file, header, _, _) = source.scan_parts();
        extract_codeview_pdbs(file, header)?
    };

    let mut tokens = Vec::new();
    for entry in entries {
        if let Some((major, minor, fragment)) = parse_pdb_version(&entry.pdb_path) {
            tokens.push(ParsedPdbPath::from_helper(
                source,
                entry.file_offset,
                major,
                minor,
                fragment,
            ));
        }
    }

    Ok(CodeViewScanResult { tokens, status })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_pdb_names() {
        assert_eq!(
            parse_pdb_version("ShooterGame-UE_5.4.pdb"),
            Some((5, 4, "UE_5.4".to_string()))
        );
        assert_eq!(
            parse_pdb_version("UE_4.27.pdb"),
            Some((4, 27, "UE_4.27".to_string()))
        );
        assert_eq!(
            parse_pdb_version("Game_UE_4.26_Win64.pdb"),
            Some((4, 26, "UE_4.26".to_string()))
        );
        assert_eq!(
            parse_pdb_version("UE_5.4"),
            Some((5, 4, "UE_5.4".to_string()))
        );
        assert_eq!(
            parse_pdb_version("UE_5.04"),
            Some((5, 4, "UE_5.4".to_string()))
        );
        assert_eq!(
            parse_pdb_version("UE_5.27"),
            Some((5, 27, "UE_5.27".to_string()))
        );
        assert_eq!(
            parse_pdb_version("Foo_UE_5.4.pdb"),
            Some((5, 4, "UE_5.4".to_string()))
        );
        assert_eq!(
            parse_pdb_version("UE_4.26-Win64"),
            Some((4, 26, "UE_4.26".to_string()))
        );
        assert_eq!(
            parse_pdb_version("UE_5.4_extra"),
            Some((5, 4, "UE_5.4".to_string()))
        );
    }

    #[test]
    fn test_rejects_invalid_pdb_names() {
        assert_eq!(parse_pdb_version("UE4.pdb"), None);
        assert_eq!(parse_pdb_version("UE5.pdb"), None);
        assert_eq!(parse_pdb_version("TRUE_5.4.pdb"), None);
        assert_eq!(parse_pdb_version("UE_5x4.pdb"), None);
        assert_eq!(parse_pdb_version("UE_6.0.pdb"), None);
        assert_eq!(parse_pdb_version("UE_3.0.pdb"), None);
        assert_eq!(parse_pdb_version("UE_5.123"), None);
        assert_eq!(parse_pdb_version("FooUE_5.4.pdb"), None);
        assert_eq!(parse_pdb_version("UE_5.4x"), None);
    }
}
