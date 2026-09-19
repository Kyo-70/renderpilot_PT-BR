//! Strict parsers for `Build.version` and `.uproject` metadata files.

use serde::Deserialize;

use crate::addons::game_analysis::parsers::tokens::{
    ParsedBuildVersion, ParsedProjectDescriptor, ParsedTargetVersion,
};
use crate::addons::game_analysis::topology::metadata::{
    BoundEngineMetadata, BoundProjectMetadata, BoundTargetMetadata, MetadataReadError,
};

/// Strictly parses `<game_root>/Engine/Build/Build.version` according to §5.3.3.
pub fn parse_build_version<'game>(
    source: &mut BoundEngineMetadata<'game>,
) -> Result<Option<ParsedBuildVersion<'game>>, MetadataReadError> {
    let manifest = source.read_manifest()?;
    let text = manifest.as_str();

    let map = match serde_json::from_str::<serde_json::Value>(text) {
        Ok(serde_json::Value::Object(m)) => m,
        _ => return Ok(None),
    };

    // MajorVersion must be integer u32 in {4, 5}
    let major = match map.get("MajorVersion").and_then(|v| v.as_u64()) {
        Some(m @ 4..=5) => m as u32,
        _ => return Ok(None),
    };

    // MinorVersion must be integer u32 in 0..=999
    let minor = match map.get("MinorVersion").and_then(|v| v.as_u64()) {
        Some(m @ 0..=999) => m as u32,
        _ => return Ok(None),
    };

    // PatchVersion is optional, but if present must be integer u32 (explicit null is REJECTED)
    let patch = match map.get("PatchVersion") {
        None => None,
        Some(val) => match val.as_u64() {
            Some(p) if p <= u32::MAX as u64 => Some(p as u32),
            _ => return Ok(None),
        },
    };

    // Changelist is optional, but if present must be integer u32 (explicit null is REJECTED)
    if map
        .get("Changelist")
        .is_some_and(|val| val.as_u64().is_none_or(|c| c > u32::MAX as u64))
    {
        return Ok(None);
    }

    // BranchName is optional, but if present must be string (explicit null is REJECTED)
    if map
        .get("BranchName")
        .is_some_and(|val| val.as_str().is_none())
    {
        return Ok(None);
    }

    Ok(Some(ParsedBuildVersion::from_source(
        &manifest, major, minor, patch,
    )))
}

/// Strictly parses bound Primary `<target>.version` companion file according to the full UBT contract.
///
/// Requires standard Unreal Build Tool fields:
/// - `MajorVersion` in {4, 5}
/// - `MinorVersion` in 0..=999
/// - `PatchVersion` (u32)
/// - `Changelist` (u32)
/// - `CompatibleChangelist` (u32)
/// - `IsLicenseeVersion` (0/1 or boolean)
/// - `IsPromotedBuild` (0/1 or boolean)
/// - `BranchName` (non-empty string)
pub fn parse_target_version<'game>(
    source: &mut BoundTargetMetadata<'game>,
) -> Result<Option<ParsedTargetVersion<'game>>, MetadataReadError> {
    let manifest = source.read_manifest()?;
    let text = manifest.as_str();

    let map = match serde_json::from_str::<serde_json::Value>(text) {
        Ok(serde_json::Value::Object(m)) => m,
        _ => return Ok(None),
    };

    // 1. MajorVersion: integer u32 in {4, 5}
    let major = match map.get("MajorVersion").and_then(|v| v.as_u64()) {
        Some(m @ 4..=5) => m as u32,
        _ => return Ok(None),
    };

    // 2. MinorVersion: integer u32 in 0..=999
    let minor = match map.get("MinorVersion").and_then(|v| v.as_u64()) {
        Some(m @ 0..=999) => m as u32,
        _ => return Ok(None),
    };

    // 3. PatchVersion: mandatory u32
    let patch = match map.get("PatchVersion").and_then(|v| v.as_u64()) {
        Some(p) if p <= u32::MAX as u64 => p as u32,
        _ => return Ok(None),
    };

    // 4. Changelist: mandatory u32
    if !map
        .get("Changelist")
        .and_then(|v| v.as_u64())
        .is_some_and(|c| c <= u32::MAX as u64)
    {
        return Ok(None);
    }

    // 5. CompatibleChangelist: mandatory u32
    if !map
        .get("CompatibleChangelist")
        .and_then(|v| v.as_u64())
        .is_some_and(|c| c <= u32::MAX as u64)
    {
        return Ok(None);
    }

    // 6. IsLicenseeVersion: mandatory 0/1 integer or boolean
    let is_licensee_valid = match map.get("IsLicenseeVersion") {
        Some(serde_json::Value::Number(n)) => n.as_u64().is_some_and(|v| v <= 1),
        Some(serde_json::Value::Bool(_)) => true,
        _ => false,
    };
    if !is_licensee_valid {
        return Ok(None);
    }

    // 7. IsPromotedBuild: mandatory 0/1 integer or boolean
    let is_promoted_valid = match map.get("IsPromotedBuild") {
        Some(serde_json::Value::Number(n)) => n.as_u64().is_some_and(|v| v <= 1),
        Some(serde_json::Value::Bool(_)) => true,
        _ => false,
    };
    if !is_promoted_valid {
        return Ok(None);
    }

    // 8. BranchName: mandatory non-empty string
    let branch_valid = matches!(
        map.get("BranchName").and_then(|v| v.as_str()),
        Some(s) if !s.trim().is_empty()
    );
    if !branch_valid {
        return Ok(None);
    }

    // Optional fields if present must have valid types
    if map
        .get("BuildId")
        .is_some_and(|v| v.as_str().is_none() && v.as_u64().is_none())
    {
        return Ok(None);
    }
    if map.get("GameVersion").is_some_and(|v| v.as_str().is_none()) {
        return Ok(None);
    }

    Ok(Some(ParsedTargetVersion::from_source(
        &manifest, major, minor, patch,
    )))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct UProjectRaw {
    engine_association: Option<serde_json::Value>,
}

/// Result of parsing a `.uproject` descriptor file (§5.3.4).
#[derive(Debug, Clone)]
pub struct ProjectDescriptorParseOutcome<'game> {
    /// True if the descriptor is a valid Unreal project file (has valid string EngineAssociation).
    pub is_valid_descriptor: bool,
    /// Parsed version token if EngineAssociation matches stock release grammar (e.g. "5.4").
    pub token: Option<ParsedProjectDescriptor<'game>>,
}

/// Strictly parses `.uproject` project descriptor according to §5.3.4.
pub fn parse_project_descriptor<'game>(
    source: &mut BoundProjectMetadata<'game>,
) -> Result<ProjectDescriptorParseOutcome<'game>, MetadataReadError> {
    let manifest = source.read_manifest()?;
    let text = manifest.as_str();

    let raw: UProjectRaw = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(_) => {
            return Ok(ProjectDescriptorParseOutcome {
                is_valid_descriptor: false,
                token: None,
            });
        }
    };

    let association = match raw.engine_association {
        Some(serde_json::Value::String(s)) => s,
        _ => {
            return Ok(ProjectDescriptorParseOutcome {
                is_valid_descriptor: false,
                token: None,
            });
        }
    };

    let trimmed = association.trim();
    if trimmed.is_empty() {
        return Ok(ProjectDescriptorParseOutcome {
            is_valid_descriptor: false,
            token: None,
        });
    }

    // Source builds / GUIDs / custom branch names: valid descriptor, but no stock release version token
    if trimmed.starts_with('{') || trimmed.contains('-') {
        return Ok(ProjectDescriptorParseOutcome {
            is_valid_descriptor: true,
            token: None,
        });
    }

    // Parse "<Major>.<Minor>" or "<Major>"
    if let Some((major_str, minor_str)) = trimmed.split_once('.') {
        let major: u32 = match major_str.parse() {
            Ok(m @ 4..=5) => m,
            _ => {
                return Ok(ProjectDescriptorParseOutcome {
                    is_valid_descriptor: false,
                    token: None,
                });
            }
        };
        let minor: u32 = match minor_str.parse() {
            Ok(m @ 0..=999) => m,
            _ => {
                return Ok(ProjectDescriptorParseOutcome {
                    is_valid_descriptor: false,
                    token: None,
                });
            }
        };
        Ok(ProjectDescriptorParseOutcome {
            is_valid_descriptor: true,
            token: Some(ParsedProjectDescriptor::from_source(
                &manifest,
                major,
                Some(minor),
            )),
        })
    } else if let Ok(major) = trimmed.parse::<u32>() {
        if major == 4 || major == 5 {
            Ok(ProjectDescriptorParseOutcome {
                is_valid_descriptor: true,
                token: Some(ParsedProjectDescriptor::from_source(&manifest, major, None)),
            })
        } else {
            Ok(ProjectDescriptorParseOutcome {
                is_valid_descriptor: false,
                token: None,
            })
        }
    } else {
        // Any custom named engine association
        Ok(ProjectDescriptorParseOutcome {
            is_valid_descriptor: true,
            token: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_project_association_variants() {
        use crate::addons::game_analysis::context::GameInstallationContext;
        use crate::addons::game_analysis::topology::metadata::BoundProjectMetadata;

        let temp = tempfile::tempdir().unwrap();
        let context = GameInstallationContext::new(temp.path()).unwrap();
        let uproject_path = temp.path().join("Game.uproject");

        let cases = [
            ("5.4", true, Some((5, Some(4)))),
            ("4.27", true, Some((4, Some(27)))),
            ("5", true, Some((5, None))),
            ("4", true, Some((4, None))),
            ("{12345678-ABCD-EF01-2345-6789ABCDEF01}", true, None),
            ("custom-branch", true, None),
            ("3.0", false, None),
            ("6.0", false, None),
        ];

        for (assoc, expected_valid, expected_ver) in cases {
            let json = format!(r#"{{"EngineAssociation": "{assoc}"}}"#);
            std::fs::write(&uproject_path, json).unwrap();
            let mut bound = BoundProjectMetadata::open(&context, &uproject_path).unwrap();
            let outcome = parse_project_descriptor(&mut bound).unwrap();
            assert_eq!(
                outcome.is_valid_descriptor, expected_valid,
                "association '{assoc}' validity mismatch"
            );
            let ver = outcome.token.map(|t| {
                let (_, _, claim) = t.into_parts();
                (claim.major(), claim.minor())
            });
            assert_eq!(ver, expected_ver, "association '{assoc}' version mismatch");
        }
    }

    #[test]
    fn test_parse_target_version_schema_rejections() {
        use crate::addons::game_analysis::context::GameInstallationContext;
        use crate::addons::game_analysis::topology::executable::BoundPrimaryExecutable;
        use crate::addons::game_analysis::topology::metadata::BoundTargetMetadata;
        use crate::game_executable::{ExeSource, ResolvedExecutable};
        use renderpilot_domain::{Architecture, ExeGraphicsInfo, PathRef};

        let temp = tempfile::tempdir().unwrap();
        let context = GameInstallationContext::new(temp.path()).unwrap();
        let primary_path = temp.path().join("Game.exe");
        let version_path = temp.path().join("Game.version");

        // Write a minimal PE for Primary
        let mut pe_bytes = vec![0u8; 512];
        pe_bytes[0] = b'M';
        pe_bytes[1] = b'Z';
        let pe_offset: u32 = 0x80;
        pe_bytes[0x3c..0x40].copy_from_slice(&pe_offset.to_le_bytes());
        let off = pe_offset as usize;
        pe_bytes[off..off + 4].copy_from_slice(b"PE\0\0");
        let machine: u16 = 0x8664;
        pe_bytes[off + 4..off + 6].copy_from_slice(&machine.to_le_bytes());
        let opt_hdr_size: u16 = 0xF0;
        pe_bytes[off + 20..off + 22].copy_from_slice(&opt_hdr_size.to_le_bytes());
        let opt_off = off + 24;
        let magic: u16 = 0x020B;
        pe_bytes[opt_off..opt_off + 2].copy_from_slice(&magic.to_le_bytes());
        let rva_sizes_offset = opt_off + 108;
        pe_bytes[rva_sizes_offset..rva_sizes_offset + 4].copy_from_slice(&16u32.to_le_bytes());
        std::fs::write(&primary_path, pe_bytes).unwrap();

        let resolved = ResolvedExecutable {
            path: PathRef::new(&*primary_path.to_string_lossy()).unwrap(),
            file_name: "Game.exe".to_string(),
            graphics: ExeGraphicsInfo::new(vec![], Some(Architecture::X64)),
            source: ExeSource::Auto,
        };
        let primary = BoundPrimaryExecutable::from_resolved(&context, &resolved).unwrap();

        let invalid_cases = [
            r#"{"MajorVersion": 5, "MinorVersion": 6}"#,
            r#"{"MajorVersion": 5, "MinorVersion": 6, "PatchVersion": 1}"#,
            r#"{"MajorVersion": null, "MinorVersion": 6}"#,
            r#"{"MajorVersion": "5", "MinorVersion": 6}"#,
            r#"{"MajorVersion": 5, "MinorVersion": null}"#,
            r#"{"MajorVersion": 5, "MinorVersion": 6, "PatchVersion": null}"#,
            r#"{"MajorVersion": 5, "MinorVersion": 6, "Changelist": 100, "CompatibleChangelist": 0, "IsLicenseeVersion": 1, "IsPromotedBuild": 1, "BranchName": "test"}"#,
            r#"{"MajorVersion": 5, "MinorVersion": 6, "PatchVersion": 1, "CompatibleChangelist": 0, "IsLicenseeVersion": 1, "IsPromotedBuild": 1, "BranchName": "test"}"#,
            r#"{"MajorVersion": 5, "MinorVersion": 6, "PatchVersion": 1, "Changelist": 100, "IsLicenseeVersion": 1, "IsPromotedBuild": 1, "BranchName": "test"}"#,
            r#"{"MajorVersion": 5, "MinorVersion": 6, "PatchVersion": 1, "Changelist": 100, "CompatibleChangelist": 0, "IsPromotedBuild": 1, "BranchName": "test"}"#,
            r#"{"MajorVersion": 5, "MinorVersion": 6, "PatchVersion": 1, "Changelist": 100, "CompatibleChangelist": 0, "IsLicenseeVersion": 1, "BranchName": "test"}"#,
            r#"{"MajorVersion": 5, "MinorVersion": 6, "PatchVersion": 1, "Changelist": 100, "CompatibleChangelist": 0, "IsLicenseeVersion": 1, "IsPromotedBuild": 1}"#,
            r#"{"MajorVersion": 5, "MinorVersion": 6, "PatchVersion": 1, "Changelist": 100, "CompatibleChangelist": 0, "IsLicenseeVersion": 1, "IsPromotedBuild": 1, "BranchName": "   "}"#,
            r#"{"MajorVersion": 3, "MinorVersion": 6, "PatchVersion": 1, "Changelist": 100, "CompatibleChangelist": 0, "IsLicenseeVersion": 1, "IsPromotedBuild": 1, "BranchName": "test"}"#,
            r#"{"MajorVersion": 5, "MinorVersion": 6, "PatchVersion": 1, "Changelist": 100, "CompatibleChangelist": 0, "IsLicenseeVersion": 2, "IsPromotedBuild": 1, "BranchName": "test"}"#,
        ];

        for invalid_json in invalid_cases {
            std::fs::write(&version_path, invalid_json).unwrap();
            let mut bound = BoundTargetMetadata::from_primary(&primary)
                .unwrap()
                .expect("must open");
            let token = parse_target_version(&mut bound).unwrap();
            assert!(
                token.is_none(),
                "Invalid json payload '{invalid_json}' must return None"
            );
        }
    }
}
