//! RenoDX's `ReShade.ini` uninstall/DLSS-Fix removal transforms.
//!
//! The `[ADDON]`/`[INSTALL]` schema constants and the additive
//! `ini_merge_strategy` write transform are shared at
//! [`crate::addons::reshade::ini_schema`]. This module owns only the
//! RenoDX-shaped *removal* strategies used on uninstall.

use std::fmt;

use renderpilot_domain::{
    NormalizedPathRelation, PathRef, RenoDxConfigReceipt, RenoDxSetPathBaseline,
    RenoDxSetPathValue, normalized_path_relation,
};

use crate::addons::engine::{IniSectionRemoval, MergeStrategy};
use crate::addons::reshade::ini_schema::{
    ADDON_PATH_KEY, ADDON_SECTION, DISABLED_ADDONS_KEY, DLSS_FIX_SECTION, LOAD_FROM_DLL_MAIN_KEY,
};

pub(crate) const RENODX_SECTION: &str = "renodx";
pub(crate) const SET_PATH_KEY: &str = "Set_Path";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RenoDxSetPathError {
    NonUtf8,
    Nul,
    Ambiguous(&'static str),
    InvalidReceipt,
}

impl fmt::Display for RenoDxSetPathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonUtf8 => f.write_str("ReShade.ini is not valid UTF-8"),
            Self::Nul => f.write_str("ReShade.ini contains NUL bytes"),
            Self::Ambiguous(reason) => write!(f, "ambiguous ReShade.ini Set_Path syntax: {reason}"),
            Self::InvalidReceipt => f.write_str("invalid RenoDX Set_Path receipt"),
        }
    }
}

impl std::error::Error for RenoDxSetPathError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RenoDxSetPathMutation {
    pub(crate) after: Vec<u8>,
    pub(crate) receipt: RenoDxConfigReceipt,
    pub(crate) changed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RenoDxSetPathRemoval {
    pub(crate) after: Option<Vec<u8>>,
    pub(crate) changed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RenoDxSetPathReconcile {
    pub(crate) after: Option<Vec<u8>>,
    pub(crate) receipt: Option<RenoDxConfigReceipt>,
    pub(crate) changed: bool,
}

#[derive(Debug, Clone)]
struct ParsedLine {
    raw: String,
    body: String,
    ending: &'static str,
}

fn parse_lines(bytes: &[u8]) -> Result<(bool, Vec<ParsedLine>), RenoDxSetPathError> {
    if bytes.contains(&0) {
        return Err(RenoDxSetPathError::Nul);
    }
    let (has_bom, bytes) = if let Some(stripped) = bytes.strip_prefix(crate::addons::UTF8_BOM) {
        (true, stripped)
    } else {
        (false, bytes)
    };
    let text = std::str::from_utf8(bytes).map_err(|_| RenoDxSetPathError::NonUtf8)?;
    let lines = text
        .split_inclusive('\n')
        .map(|raw| {
            let (body, ending) = if let Some(body) = raw.strip_suffix('\n') {
                if let Some(body) = body.strip_suffix('\r') {
                    (body, "\r\n")
                } else {
                    (body, "\n")
                }
            } else {
                (raw, "")
            };
            ParsedLine {
                raw: raw.to_owned(),
                body: body.to_owned(),
                ending,
            }
        })
        .collect();
    Ok((has_bom, lines))
}

fn section_header(body: &str) -> Option<&str> {
    let trimmed = body.trim();
    trimmed
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
        .map(str::trim)
}

/// A malformed section-looking line is opaque unless it could be the target
/// RenoDX section.  Rejecting every malformed header would let an unrelated
/// user line outside `[renodx]` block an otherwise safe byte-preserving edit.
fn malformed_target_header(body: &str) -> bool {
    let candidate = body.trim_start().strip_prefix('[').map(str::trim_start);
    let Some(candidate) = candidate else {
        return false;
    };
    if section_header(body).is_some() {
        return false;
    }
    let Some(rest) = candidate.get(..RENODX_SECTION.len()) else {
        return false;
    };
    rest.eq_ignore_ascii_case(RENODX_SECTION)
        && candidate
            .get(RENODX_SECTION.len()..)
            .is_some_and(|rest| rest.is_empty() || !rest.as_bytes()[0].is_ascii_alphanumeric())
}

fn malformed_section_header(body: &str) -> bool {
    body.trim_start().starts_with('[') && section_header(body).is_none()
}

fn is_target_section(body: &str) -> bool {
    section_header(body).is_some_and(|name| name.eq_ignore_ascii_case(RENODX_SECTION))
}

fn is_target_key(body: &str) -> Option<(&str, &str)> {
    let trimmed = body.trim_start();
    let (key, rhs) = trimmed.split_once('=')?;
    key.trim()
        .eq_ignore_ascii_case(SET_PATH_KEY)
        .then_some((key, rhs))
}

fn malformed_target_key(body: &str) -> bool {
    let trimmed = body.trim();
    if trimmed.is_empty()
        || trimmed.starts_with(';')
        || trimmed.starts_with('#')
        || trimmed.contains('=')
    {
        return false;
    }
    let Some(rest) = trimmed.get(..SET_PATH_KEY.len()) else {
        return false;
    };
    rest.eq_ignore_ascii_case(SET_PATH_KEY)
        && trimmed
            .as_bytes()
            .get(SET_PATH_KEY.len())
            .is_none_or(|byte| !byte.is_ascii_alphanumeric() && *byte != b'_')
}

fn target_range(lines: &[ParsedLine]) -> Result<Option<(usize, usize)>, RenoDxSetPathError> {
    let mut start = None;
    let mut end = lines.len();
    for (index, line) in lines.iter().enumerate() {
        if start.is_none() {
            if malformed_target_header(&line.body) {
                return Err(RenoDxSetPathError::Ambiguous("malformed section header"));
            }
            if is_target_section(&line.body) {
                start = Some(index);
            }
            continue;
        }

        if end == lines.len() {
            if malformed_section_header(&line.body) {
                return Err(RenoDxSetPathError::Ambiguous("malformed section header"));
            }
            if malformed_target_key(&line.body) {
                return Err(RenoDxSetPathError::Ambiguous(
                    "Set_Path has no assignment delimiter",
                ));
            }
            if section_header(&line.body).is_some() {
                if is_target_section(&line.body) {
                    return Err(RenoDxSetPathError::Ambiguous("multiple [renodx] sections"));
                }
                end = index;
            }
        } else {
            if malformed_target_header(&line.body) {
                return Err(RenoDxSetPathError::Ambiguous("malformed section header"));
            }
            if is_target_section(&line.body) {
                return Err(RenoDxSetPathError::Ambiguous("multiple [renodx] sections"));
            }
        }
    }
    Ok(start.map(|start| (start, end)))
}

fn serialize_lines(lines: &[ParsedLine], has_bom: bool) -> Vec<u8> {
    let bom_len = if has_bom {
        crate::addons::UTF8_BOM.len()
    } else {
        0
    };
    let total_len: usize = bom_len + lines.iter().map(|line| line.raw.len()).sum::<usize>();
    let mut out = Vec::with_capacity(total_len);
    if has_bom {
        out.extend_from_slice(crate::addons::UTF8_BOM);
    }
    for line in lines {
        out.extend_from_slice(line.raw.as_bytes());
    }
    out
}

fn replace_assignment_value(body: &str, value: &str) -> Result<String, RenoDxSetPathError> {
    let equal = body.find('=').ok_or(RenoDxSetPathError::Ambiguous(
        "Set_Path has no assignment delimiter",
    ))?;
    let rhs = &body[equal + 1..];
    let leading = rhs.len() - rhs.trim_start().len();
    let trailing = rhs.trim_end().len();
    Ok(format!(
        "{}{}{}{}",
        &body[..=equal],
        &rhs[..leading],
        value,
        &rhs[trailing..]
    ))
}

pub(crate) fn current_set_path_value(before: &[u8]) -> Result<Option<String>, RenoDxSetPathError> {
    let (_has_bom, lines) = parse_lines(before)?;
    let Some((section_start, section_end)) = target_range(&lines)? else {
        return Ok(None);
    };
    let mut value = None;
    for line in lines.iter().take(section_end).skip(section_start + 1) {
        if let Some((_key, rhs)) = is_target_key(&line.body) {
            if value.is_some() {
                return Err(RenoDxSetPathError::Ambiguous("duplicate Set_Path keys"));
            }
            let rhs = rhs.trim();
            value = Some(rhs.to_owned());
        }
    }
    Ok(value)
}

/// Reconciles the narrow RenoDX Set_Path ownership contract. The caller owns
/// the durable file/record transaction; this function only interprets exact
/// supplied bytes and returns the corresponding postimage/receipt transition.
pub(crate) fn plan_set_path_reconcile(
    ini_path: PathRef,
    before: Option<&[u8]>,
    desired: Option<RenoDxSetPathValue>,
    receipt: Option<&RenoDxConfigReceipt>,
) -> Result<RenoDxSetPathReconcile, RenoDxSetPathError> {
    if let Some(receipt) = receipt
        && (!receipt.is_supported()
            || !matches!(
                normalized_path_relation(receipt.ini_path.as_str(), ini_path.as_str()),
                NormalizedPathRelation::Equal
            ))
    {
        return Err(RenoDxSetPathError::InvalidReceipt);
    }
    let Some(desired) = desired else {
        let Some(receipt) = receipt else {
            return Ok(RenoDxSetPathReconcile {
                after: before.map(ToOwned::to_owned),
                receipt: None,
                changed: false,
            });
        };
        let Some(before) = before else {
            return Ok(RenoDxSetPathReconcile {
                after: None,
                receipt: None,
                changed: false,
            });
        };
        let current = current_set_path_value(before)?;
        if current.as_deref() != Some(receipt.last_written.as_str()) {
            return Ok(RenoDxSetPathReconcile {
                after: Some(before.to_vec()),
                receipt: None,
                changed: false,
            });
        }
        let removal = plan_set_path_removal(before, receipt)?;
        return Ok(RenoDxSetPathReconcile {
            after: removal.after,
            receipt: None,
            changed: removal.changed,
        });
    };

    let before_bytes = before.unwrap_or_default();
    let Some(receipt) = receipt else {
        let planned = plan_set_path(ini_path, before_bytes, desired)?;
        return Ok(RenoDxSetPathReconcile {
            after: Some(planned.after),
            receipt: Some(planned.receipt),
            changed: planned.changed || before.is_none(),
        });
    };
    let Some(before) = before else {
        return Err(RenoDxSetPathError::Ambiguous(
            "receipt-owned ReShade.ini is missing",
        ));
    };
    let current = current_set_path_value(before)?;
    let Some(current) = current else {
        return Err(RenoDxSetPathError::Ambiguous(
            "receipt-owned Set_Path is missing",
        ));
    };
    if current != receipt.last_written.as_str() {
        return Err(RenoDxSetPathError::Ambiguous(
            "receipt-owned Set_Path was edited outside RenderPilot",
        ));
    }
    if desired == receipt.last_written {
        return Ok(RenoDxSetPathReconcile {
            after: Some(before.to_vec()),
            receipt: Some(receipt.clone()),
            changed: false,
        });
    }
    let planned = plan_set_path(ini_path, before, desired)?;
    let next_receipt = RenoDxConfigReceipt::new(
        planned.receipt.ini_path,
        receipt.baseline.clone(),
        receipt.section_preexisted,
        desired,
    )
    .with_newline_anchor(receipt.newline_anchor.clone());
    Ok(RenoDxSetPathReconcile {
        after: Some(planned.after),
        receipt: Some(next_receipt),
        changed: true,
    })
}

/// Purely plans one typed RenoDX Set_Path mutation. It never performs IO and
/// never interprets guidance text or arbitrary manifest keys.
pub(crate) fn plan_set_path(
    ini_path: PathRef,
    before: &[u8],
    desired: RenoDxSetPathValue,
) -> Result<RenoDxSetPathMutation, RenoDxSetPathError> {
    let (has_bom, mut lines) = parse_lines(before)?;
    let range = target_range(&lines)?;
    let (section_start, section_end) = range.unwrap_or((lines.len(), lines.len()));
    let mut key_index = None;
    let mut baseline = RenoDxSetPathBaseline::Absent;
    if range.is_some() {
        for (index, line) in lines
            .iter()
            .enumerate()
            .take(section_end)
            .skip(section_start + 1)
        {
            if let Some((_key, rhs)) = is_target_key(&line.body) {
                if key_index.replace(index).is_some() {
                    return Err(RenoDxSetPathError::Ambiguous("duplicate Set_Path keys"));
                }
                let value = rhs.trim();
                baseline = RenoDxSetPathBaseline::Present {
                    value: value.to_owned(),
                };
            }
        }
    }
    let mut changed = false;
    let mut newline_anchor = None;
    if let Some(index) = key_index {
        let line = &mut lines[index];
        let replacement = replace_assignment_value(&line.body, desired.as_str())?;
        if replacement != line.body {
            changed = true;
            line.body = replacement;
            line.raw = format!("{}{}", line.body, line.ending);
        }
    } else {
        changed = true;
        let newline = lines
            .iter()
            .find_map(|line| (!line.ending.is_empty()).then_some(line.ending))
            .unwrap_or("\r\n");
        if range.is_some() {
            let insert_at = section_end;
            if insert_at > 0 && lines[insert_at - 1].ending.is_empty() {
                newline_anchor = Some(lines[insert_at - 1].body.clone());
                lines[insert_at - 1].ending = newline;
                lines[insert_at - 1].raw = format!("{}{}", lines[insert_at - 1].body, newline);
            }
            lines.insert(
                insert_at,
                ParsedLine {
                    raw: format!("Set_Path={}{}", desired.as_str(), newline),
                    body: format!("Set_Path={}", desired.as_str()),
                    ending: newline,
                },
            );
        } else {
            if let Some(last) = lines.last_mut().filter(|line| line.ending.is_empty()) {
                newline_anchor = Some(last.body.clone());
                last.ending = newline;
                last.raw = format!("{}{}", last.body, newline);
            }
            lines.push(ParsedLine {
                raw: format!("[{}]{}", RENODX_SECTION, newline),
                body: format!("[{}]", RENODX_SECTION),
                ending: newline,
            });
            lines.push(ParsedLine {
                raw: format!("Set_Path={}{}", desired.as_str(), newline),
                body: format!("Set_Path={}", desired.as_str()),
                ending: newline,
            });
        }
    }
    let after = serialize_lines(&lines, has_bom);
    Ok(RenoDxSetPathMutation {
        after,
        receipt: RenoDxConfigReceipt::new(ini_path, baseline, range.is_some(), desired)
            .with_newline_anchor(newline_anchor),
        changed,
    })
}

/// Plans uninstall/reconciliation of a receipt-owned Set_Path key using a CAS:
/// only the exact typed value last written by RenderPilot can be changed.
pub(crate) fn plan_set_path_removal(
    before: &[u8],
    receipt: &RenoDxConfigReceipt,
) -> Result<RenoDxSetPathRemoval, RenoDxSetPathError> {
    if !receipt.is_supported() {
        return Err(RenoDxSetPathError::InvalidReceipt);
    }
    let (has_bom, mut lines) = parse_lines(before)?;
    let Some((section_start, section_end)) = target_range(&lines)? else {
        return Ok(RenoDxSetPathRemoval {
            after: None,
            changed: false,
        });
    };
    let mut key_index = None;
    let mut current = None;
    for (index, line) in lines
        .iter()
        .enumerate()
        .take(section_end)
        .skip(section_start + 1)
    {
        if let Some((_key, rhs)) = is_target_key(&line.body) {
            if key_index.replace(index).is_some() {
                return Err(RenoDxSetPathError::Ambiguous("duplicate Set_Path keys"));
            }
            let value = rhs.trim();
            current = Some(value);
        }
    }
    let Some(index) = key_index else {
        return Ok(RenoDxSetPathRemoval {
            after: Some(before.to_vec()),
            changed: false,
        });
    };
    if current != Some(receipt.last_written.as_str()) {
        return Ok(RenoDxSetPathRemoval {
            after: Some(before.to_vec()),
            changed: false,
        });
    }
    match &receipt.baseline {
        RenoDxSetPathBaseline::Present { value } => {
            let line = &mut lines[index];
            line.body = replace_assignment_value(&line.body, value)?;
            line.raw = format!("{}{}", line.body, line.ending);
        }
        RenoDxSetPathBaseline::Absent => {
            lines.remove(index);
            let adjusted_end = lines
                .iter()
                .enumerate()
                .skip(section_start + 1)
                .find(|(_, line)| section_header(&line.body).is_some())
                .map_or(lines.len(), |(index, _)| index);
            if !receipt.section_preexisted
                && lines[section_start + 1..adjusted_end]
                    .iter()
                    .all(|line| line.body.trim().is_empty())
            {
                lines.drain(section_start..adjusted_end);
            }
            if let Some(anchor) = &receipt.newline_anchor {
                // The anchor is deliberately content-based rather than positional.
                // User edits may shift line indices; rollback removes the injected EOF
                // newline only when the original terminal line is still recognizable.
                let target_line = if !receipt.section_preexisted {
                    if section_start > 0 {
                        lines.get_mut(section_start - 1)
                    } else {
                        None
                    }
                } else if index > 0 {
                    lines.get_mut(index - 1)
                } else {
                    None
                };
                if let Some(target) = target_line
                    && target.body == *anchor
                {
                    target.ending = "";
                    target.raw = target.body.clone();
                }
            }
        }
    }
    let after = serialize_lines(&lines, has_bom);
    Ok(RenoDxSetPathRemoval {
        after: Some(after),
        changed: true,
    })
}

/// Builds the merge strategy to remove DLSS-Fix keys from `ReShade.ini`.
#[must_use]
pub(crate) fn ini_remove_dlss_fix_strategy() -> MergeStrategy {
    MergeStrategy::IniRemoveKeys {
        sections: vec![
            IniSectionRemoval {
                name: ADDON_SECTION.to_owned(),
                keys: vec![LOAD_FROM_DLL_MAIN_KEY.to_owned()],
            },
            IniSectionRemoval {
                name: DLSS_FIX_SECTION.to_owned(),
                keys: Vec::new(),
            },
        ],
    }
}

/// Builds the merge strategy an uninstall applies to a `ReShade.ini` RenoDX did
/// not create from scratch (so it is never blanket-deleted): removes exactly the
/// keys/sections RenoDX itself ever writes there — `[ADDON]` `DisabledAddons`,
/// `AddonPath`, and (when a DLSS-Fix companion was installed) `LoadFromDllMain`
/// plus the whole `[RENODX-DLSSFIX]` section — leaving every other key, section,
/// comment, and blank line (including the user's own settings) untouched.
#[must_use]
pub(crate) fn ini_remove_renodx_strategy() -> MergeStrategy {
    MergeStrategy::IniRemoveKeys {
        sections: vec![
            IniSectionRemoval {
                name: ADDON_SECTION.to_owned(),
                keys: vec![
                    DISABLED_ADDONS_KEY.to_owned(),
                    ADDON_PATH_KEY.to_owned(),
                    LOAD_FROM_DLL_MAIN_KEY.to_owned(),
                ],
            },
            IniSectionRemoval {
                name: DLSS_FIX_SECTION.to_owned(),
                keys: Vec::new(),
            },
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::addons::renodx::types::renodx_ini_defaults;
    use crate::addons::reshade::ini_schema::ini_merge_strategy;
    use crate::addons::reshade::types::{DlssFixIniTweaks, ReshadeIniTweaks};

    #[test]
    fn ini_merge_strategy_carries_only_the_set_keys() {
        let MergeStrategy::IniSetKeys { sections } = ini_merge_strategy(&renodx_ini_defaults())
        else {
            panic!("expected IniSetKeys")
        };
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].name, "ADDON");
        assert_eq!(
            sections[0].keys,
            vec![(
                "DisabledAddons".to_owned(),
                "Generic Depth,Effect Runtime Sync".to_owned()
            )]
        );
    }

    #[test]
    fn ini_merge_strategy_adds_dlss_fix_sections_when_present() {
        let tweaks = ReshadeIniTweaks {
            disabled_addons: vec!["Generic Depth".to_owned()],
            addon_path: None,
            dlss_fix: Some(DlssFixIniTweaks {
                addon_file_name: "renodx-dlssfix.addon64".to_owned(),
                dlss_path: r"C:\Game\nvngx_dlss.dll".to_owned(),
                streamline_path: r"C:\Game\sl.interposer.dll".to_owned(),
            }),
        };
        let MergeStrategy::IniSetKeys { sections } = ini_merge_strategy(&tweaks) else {
            panic!("expected IniSetKeys")
        };
        assert_eq!(sections.len(), 2);
        assert_eq!(sections[0].name, "ADDON");
        assert!(
            sections[0]
                .keys
                .iter()
                .any(|(k, v)| k == "LoadFromDllMain" && v == "renodx-dlssfix.addon64")
        );
        assert_eq!(sections[1].name, "RENODX-DLSSFIX");
    }

    #[test]
    fn ini_remove_dlss_fix_strategy_strips_loadfromdllmain_and_section() {
        let strategy = ini_remove_dlss_fix_strategy();
        let base = "[ADDON]\r\nAddonPath=.\r\nLoadFromDllMain=renodx-dlssfix.addon64\r\n\
                    [RENODX-DLSSFIX]\r\nDLSSPath=C:\\d.dll\r\nStreamlinePath=C:\\s.dll\r\n";
        let merged = strategy.apply(base);
        assert!(merged.contains("AddonPath=."));
        assert!(!merged.contains("LoadFromDllMain"));
        assert!(!merged.contains("RENODX-DLSSFIX"));
    }

    #[test]
    fn ini_remove_renodx_strategy_strips_only_renodx_keys() {
        let strategy = ini_remove_renodx_strategy();
        let base = "; user comment\r\n\
                    [GENERAL]\r\nPreset=mine.ini\r\n\r\n\
                    [ADDON]\r\nDisabledAddons=Generic Depth,Effect Runtime Sync\r\n\
                    AddonPath=.\r\nLoadFromDllMain=renodx-dlssfix.addon64\r\n\
                    UserAddonKey=keep-me\r\n\r\n\
                    [RENODX-DLSSFIX]\r\nDLSSPath=C:\\d.dll\r\nStreamlinePath=C:\\s.dll\r\n";
        let merged = strategy.apply(base);

        // User settings outside RenoDX's own keys/section survive untouched.
        assert!(merged.contains("; user comment"));
        assert!(merged.contains("[GENERAL]"));
        assert!(merged.contains("Preset=mine.ini"));
        assert!(merged.contains("UserAddonKey=keep-me"));
        // RenoDX's own keys and section are gone.
        assert!(!merged.contains("DisabledAddons"));
        assert!(!merged.contains("AddonPath"));
        assert!(!merged.contains("LoadFromDllMain"));
        assert!(!merged.contains("RENODX-DLSSFIX"));
    }

    #[test]
    fn ini_remove_renodx_strategy_is_a_no_op_on_a_foreign_config() {
        let strategy = ini_remove_renodx_strategy();
        let base = "[GENERAL]\r\nPreset=mine.ini\r\n";
        assert_eq!(strategy.apply(base), base);
    }

    fn ini_path() -> PathRef {
        PathRef::new("C:/Game/ReShade.ini").expect("path")
    }

    #[test]
    fn set_path_planner_captures_absent_and_existing_baselines() {
        let absent = plan_set_path(ini_path(), b"[General]\r\n", RenoDxSetPathValue::One)
            .expect("absent key");
        assert_eq!(absent.receipt.baseline, RenoDxSetPathBaseline::Absent);
        assert_eq!(
            String::from_utf8(absent.after).expect("utf8"),
            "[General]\r\n[renodx]\r\nSet_Path=1\r\n"
        );

        let existing = plan_set_path(
            ini_path(),
            b"[renodx]\nSet_Path = arbitrary\nOther=keep\n",
            RenoDxSetPathValue::Zero,
        )
        .expect("existing key");
        assert_eq!(
            existing.receipt.baseline,
            RenoDxSetPathBaseline::Present {
                value: "arbitrary".to_owned()
            }
        );
        assert_eq!(
            String::from_utf8(existing.after).expect("utf8"),
            "[renodx]\nSet_Path = 0\nOther=keep\n"
        );

        for original in [
            b"[renodx]\nSet_Path=\n".as_slice(),
            b"[renodx]\nSet_Path=a=b\n".as_slice(),
        ] {
            let planned = plan_set_path(ini_path(), original, RenoDxSetPathValue::Zero)
                .expect("opaque baseline RHS");
            let expected = if original.ends_with(b"=\n") {
                ""
            } else {
                "a=b"
            };
            assert_eq!(
                planned.receipt.baseline,
                RenoDxSetPathBaseline::Present {
                    value: expected.to_owned()
                }
            );
        }
    }

    #[test]
    fn set_path_planner_rejects_ambiguous_or_unsafe_input() {
        assert!(matches!(
            plan_set_path(
                ini_path(),
                b"[renodx]\nSet_Path=0\nSet_Path=1\n",
                RenoDxSetPathValue::One
            ),
            Err(RenoDxSetPathError::Ambiguous("duplicate Set_Path keys"))
        ));
        assert!(matches!(
            plan_set_path(
                ini_path(),
                b"[renodx\nSet_Path=0\n",
                RenoDxSetPathValue::One
            ),
            Err(RenoDxSetPathError::Ambiguous("malformed section header"))
        ));
        assert!(matches!(
            plan_set_path(
                ini_path(),
                b"[renodx]\n[broken\nSet_Path=0\n",
                RenoDxSetPathValue::One
            ),
            Err(RenoDxSetPathError::Ambiguous("malformed section header"))
        ));
        assert!(matches!(
            plan_set_path(
                ini_path(),
                b"[renodx]\nSet_Path=0\n[General]\nX=1\n[renodx\nSet_Path=1\n",
                RenoDxSetPathValue::One
            ),
            Err(RenoDxSetPathError::Ambiguous("malformed section header"))
        ));
        for malformed_key in [b"Set_Path\n".as_slice(), b"Set_Path; comment\n".as_slice()] {
            let input = [&b"[renodx]\n"[..], malformed_key].concat();
            assert!(matches!(
                plan_set_path(ini_path(), &input, RenoDxSetPathValue::One),
                Err(RenoDxSetPathError::Ambiguous(
                    "Set_Path has no assignment delimiter"
                ))
            ));
        }
        let commented = plan_set_path(
            ini_path(),
            b"[renodx]\n; Set_Path\n# [broken\nSet_Path=0\n",
            RenoDxSetPathValue::One,
        )
        .expect("comments are opaque");
        assert!(
            String::from_utf8(commented.after)
                .expect("utf8")
                .contains("; Set_Path\n# [broken\nSet_Path=1\n")
        );
        let foreign_malformed = b"[broken\nOpaque=keep\n[General]\nValue=1\n";
        let preserved = plan_set_path(ini_path(), foreign_malformed, RenoDxSetPathValue::One)
            .expect("unrelated malformed section is opaque");
        assert!(
            String::from_utf8(preserved.after)
                .expect("utf8")
                .starts_with("[broken\nOpaque=keep\n[General]\nValue=1\n")
        );
        let foreign_after_target = b"[renodx]\nSet_Path=0\n[General]\nX=1\n[broken\nOpaque=keep\n";
        let preserved_after_target =
            plan_set_path(ini_path(), foreign_after_target, RenoDxSetPathValue::One)
                .expect("unrelated malformed section after target is opaque");
        assert!(
            String::from_utf8(preserved_after_target.after)
                .expect("utf8")
                .contains("[broken\nOpaque=keep\n")
        );
        assert!(matches!(
            plan_set_path(ini_path(), &[0xff], RenoDxSetPathValue::One),
            Err(RenoDxSetPathError::NonUtf8)
        ));
    }

    #[test]
    fn reconcile_obeys_receipt_cas_and_restores_original_value() {
        let path = ini_path();
        let receipt = RenoDxConfigReceipt::new(
            path.clone(),
            RenoDxSetPathBaseline::Present {
                value: "custom".to_owned(),
            },
            true,
            RenoDxSetPathValue::One,
        );
        let mismatch = plan_set_path_reconcile(
            path.clone(),
            Some(b"[renodx]\nSet_Path=custom-edit\n"),
            Some(RenoDxSetPathValue::Zero),
            Some(&receipt),
        );
        assert!(matches!(
            mismatch,
            Err(RenoDxSetPathError::Ambiguous(
                "receipt-owned Set_Path was edited outside RenderPilot"
            ))
        ));

        let restored =
            plan_set_path_reconcile(path, Some(b"[renodx]\nSet_Path=1\n"), None, Some(&receipt))
                .expect("restore");
        assert_eq!(restored.receipt, None);
        assert_eq!(
            String::from_utf8(restored.after.expect("after")).expect("utf8"),
            "[renodx]\nSet_Path=custom\n"
        );
    }

    #[test]
    fn reconcile_captures_absent_file_and_keeps_user_bytes_when_value_is_unchanged() {
        let path = ini_path();
        let created =
            plan_set_path_reconcile(path.clone(), None, Some(RenoDxSetPathValue::Zero), None)
                .expect("create config");
        assert_eq!(
            created.after.as_deref(),
            Some(b"[renodx]\r\nSet_Path=0\r\n".as_slice())
        );
        assert_eq!(
            created.receipt.as_ref().expect("receipt").baseline,
            RenoDxSetPathBaseline::Absent
        );

        let receipt = created.receipt.expect("receipt");
        let user_bytes = b"[renodx]\nSet_Path=0\n; user edit\n";
        let no_op = plan_set_path_reconcile(
            path,
            Some(user_bytes),
            Some(RenoDxSetPathValue::Zero),
            Some(&receipt),
        )
        .expect("same policy");
        assert!(!no_op.changed);
        assert_eq!(no_op.after.as_deref(), Some(user_bytes.as_slice()));
    }

    #[test]
    fn opaque_rhs_round_trips_through_reconcile_and_uninstall() {
        for original_value in ["", "a=b"] {
            let path = ini_path();
            let before = format!("[renodx]\nSet_Path={original_value}\n");
            let installed = plan_set_path(path.clone(), before.as_bytes(), RenoDxSetPathValue::One)
                .expect("install opaque RHS");
            let reconciled = plan_set_path_reconcile(
                path.clone(),
                Some(&installed.after),
                Some(RenoDxSetPathValue::Zero),
                Some(&installed.receipt),
            )
            .expect("reconcile opaque RHS");
            let receipt = reconciled.receipt.expect("updated receipt");
            let removed =
                plan_set_path_reconcile(path, reconciled.after.as_deref(), None, Some(&receipt))
                    .expect("uninstall opaque RHS");
            assert_eq!(removed.after.as_deref(), Some(before.as_bytes()));
            assert!(removed.receipt.is_none());
        }
    }

    #[test]
    fn reconcile_unmanaged_preserves_user_edit_and_clears_receipt() {
        let path = ini_path();
        let receipt = RenoDxConfigReceipt::new(
            path.clone(),
            RenoDxSetPathBaseline::Absent,
            false,
            RenoDxSetPathValue::One,
        );
        let user_bytes = b"[renodx]\nSet_Path=custom\n";
        let relinquished = plan_set_path_reconcile(path, Some(user_bytes), None, Some(&receipt))
            .expect("relinquish user edit");
        assert_eq!(relinquished.receipt, None);
        assert!(!relinquished.changed);
        assert_eq!(relinquished.after.as_deref(), Some(user_bytes.as_slice()));
    }

    #[test]
    fn removal_of_absent_baseline_drops_only_the_created_empty_section() {
        let path = ini_path();
        let receipt = RenoDxConfigReceipt::new(
            path,
            RenoDxSetPathBaseline::Absent,
            false,
            RenoDxSetPathValue::One,
        );
        let removed = plan_set_path_removal(
            b"[General]\r\nKeep=1\r\n[renodx]\r\nSet_Path=1\r\n",
            &receipt,
        )
        .expect("remove set path");
        assert_eq!(
            String::from_utf8(removed.after.expect("after")).expect("utf8"),
            "[General]\r\nKeep=1\r\n"
        );
    }

    #[test]
    fn utf8_bom_is_preserved_and_does_not_mask_first_renodx_section() {
        let path = ini_path();
        let mut before = crate::addons::UTF8_BOM.to_vec();
        before.extend_from_slice(b"[renodx]\r\nSet_Path=0\r\n");

        // Value inquiry recognizes [renodx] even with BOM at line 1.
        assert_eq!(
            current_set_path_value(&before).expect("query"),
            Some("0".to_owned())
        );

        // Planning mutation modifies existing section instead of appending duplicate [renodx],
        // and preserves the BOM in the output.
        let planned = plan_set_path(path, &before, RenoDxSetPathValue::One).expect("plan");
        assert!(planned.changed);
        assert!(planned.after.starts_with(crate::addons::UTF8_BOM));
        let text =
            std::str::from_utf8(&planned.after[crate::addons::UTF8_BOM.len()..]).expect("utf8");
        assert_eq!(text, "[renodx]\r\nSet_Path=1\r\n");

        // Uninstall preserves BOM on roundtrip.
        let removed = plan_set_path_removal(&planned.after, &planned.receipt).expect("remove");
        assert!(removed.changed);
        let removed_bytes = removed.after.expect("removed bytes");
        assert_eq!(removed_bytes, before);
    }

    #[test]
    fn utf8_bom_without_eof_newline_is_preserved() {
        let path = ini_path();
        let mut before = crate::addons::UTF8_BOM.to_vec();
        before.extend_from_slice(b"[General]\r\nfoo=bar");

        let planned = plan_set_path(path, &before, RenoDxSetPathValue::Zero).expect("plan");
        assert!(planned.after.starts_with(crate::addons::UTF8_BOM));

        let removed = plan_set_path_removal(&planned.after, &planned.receipt).expect("remove");
        assert_eq!(removed.after.as_deref(), Some(before.as_slice()));
    }

    #[test]
    fn byte_reversibility_restores_eof_newline_only_when_anchor_is_unchanged() {
        let path = ini_path();
        let before_no_newline = b"[General]\nfoo=bar";
        let installed =
            plan_set_path(path, before_no_newline, RenoDxSetPathValue::Zero).expect("install");
        assert_eq!(installed.receipt.newline_anchor.as_deref(), Some("foo=bar"));

        // Exact uninstall: anchor matches, trailing newline is stripped, exact byte match.
        let uninstalled =
            plan_set_path_removal(&installed.after, &installed.receipt).expect("uninstall");
        assert_eq!(
            uninstalled.after.as_deref(),
            Some(before_no_newline.as_slice())
        );

        // User inserted a setting after install:
        let user_modified = b"[General]\nfoo=bar\nUserSetting=123\n[renodx]\nSet_Path=0\n".to_vec();
        let removed_user = plan_set_path_removal(&user_modified, &installed.receipt)
            .expect("uninstall with user line");
        // UserSetting=123 must NOT lose its newline because it does not match anchor "foo=bar".
        assert_eq!(
            removed_user.after.as_deref(),
            Some(b"[General]\nfoo=bar\nUserSetting=123\n".as_slice())
        );
    }

    #[test]
    fn install_then_update_then_uninstall_preserves_anchor_and_original_bytes() {
        let path = ini_path();
        let original = b"[General]\nfoo=bar";

        // Step 1: Install RenoDX with Set_Path=1 into file without trailing newline
        let install_step = plan_set_path_reconcile(
            path.clone(),
            Some(original),
            Some(RenoDxSetPathValue::One),
            None,
        )
        .expect("install reconcile succeeds");
        assert!(install_step.changed);
        let install_receipt = install_step.receipt.expect("install receipt");
        assert_eq!(install_receipt.newline_anchor.as_deref(), Some("foo=bar"));
        let after_install = install_step.after.expect("after install");

        // Step 2: Update desired to Set_Path=0
        let update_step = plan_set_path_reconcile(
            path.clone(),
            Some(&after_install),
            Some(RenoDxSetPathValue::Zero),
            Some(&install_receipt),
        )
        .expect("update reconcile succeeds");
        assert!(update_step.changed);
        let update_receipt = update_step.receipt.expect("update receipt");
        // Anchor MUST NOT be lost upon update!
        assert_eq!(update_receipt.newline_anchor.as_deref(), Some("foo=bar"));
        let after_update = update_step.after.expect("after update");

        // Step 3: Uninstall RenoDX (desired = None)
        let uninstall_step =
            plan_set_path_reconcile(path, Some(&after_update), None, Some(&update_receipt))
                .expect("uninstall reconcile succeeds");
        assert!(uninstall_step.changed);
        let after_uninstall = uninstall_step.after.expect("after uninstall");

        // Step 4: Verify exact byte-for-byte equality to original
        assert_eq!(after_uninstall.as_slice(), original);
    }

    #[test]
    fn reconcile_evaluates_four_cas_states_strictly() {
        let path = ini_path();
        let receipt = RenoDxConfigReceipt::new(
            path.clone(),
            RenoDxSetPathBaseline::Absent,
            false,
            RenoDxSetPathValue::Zero,
        );

        // State 1: File absent with existing receipt -> Ambiguous (missing file)
        let err = plan_set_path_reconcile(
            path.clone(),
            None,
            Some(RenoDxSetPathValue::Zero),
            Some(&receipt),
        )
        .expect_err("missing file must fail");
        assert_eq!(
            err,
            RenoDxSetPathError::Ambiguous("receipt-owned ReShade.ini is missing")
        );

        // State 2: Key absent from file -> Ambiguous (missing key)
        let empty_file = b"[renodx]\n";
        let err = plan_set_path_reconcile(
            path.clone(),
            Some(empty_file),
            Some(RenoDxSetPathValue::Zero),
            Some(&receipt),
        )
        .expect_err("missing key must fail");
        assert_eq!(
            err,
            RenoDxSetPathError::Ambiguous("receipt-owned Set_Path is missing")
        );

        // State 3: Value mismatch -> Ambiguous (edited outside RenderPilot)
        let user_edit = b"[renodx]\nSet_Path=999\n";
        let err = plan_set_path_reconcile(
            path.clone(),
            Some(user_edit),
            Some(RenoDxSetPathValue::Zero),
            Some(&receipt),
        )
        .expect_err("edited value must fail even on no-op path");
        assert_eq!(
            err,
            RenoDxSetPathError::Ambiguous("receipt-owned Set_Path was edited outside RenderPilot")
        );

        // Duplicate sections/keys fail-closed
        let duplicate_sections = b"[renodx]\nSet_Path=0\n[renodx]\nSet_Path=0\n";
        let err = plan_set_path_reconcile(
            path.clone(),
            Some(duplicate_sections),
            Some(RenoDxSetPathValue::Zero),
            Some(&receipt),
        )
        .expect_err("duplicate sections must fail");
        assert_eq!(
            err,
            RenoDxSetPathError::Ambiguous("multiple [renodx] sections")
        );

        // State 4: Value matches receipt and desired matches -> Clean no-op
        let matching = b"[renodx]\nSet_Path=0\n";
        let no_op = plan_set_path_reconcile(
            path,
            Some(matching),
            Some(RenoDxSetPathValue::Zero),
            Some(&receipt),
        )
        .expect("matching state succeeds");
        assert!(!no_op.changed);
        assert_eq!(no_op.after.as_deref(), Some(matching.as_slice()));
    }

    #[test]
    fn reconcile_uses_normalized_path_relation_for_windows_compatibility() {
        let win_path = PathRef::new(r"C:\Games\Cyberpunk\ReShade.ini").expect("win path");
        let unix_path = PathRef::new("c:/games/cyberpunk/reshade.ini").expect("unix path");

        let receipt = RenoDxConfigReceipt::new(
            win_path,
            RenoDxSetPathBaseline::Absent,
            false,
            RenoDxSetPathValue::Zero,
        );

        let file = b"[renodx]\nSet_Path=0\n";
        // Passing unix_path (different case/separators) to win_path receipt must compare equal.
        let reconciled = plan_set_path_reconcile(
            unix_path,
            Some(file),
            Some(RenoDxSetPathValue::Zero),
            Some(&receipt),
        )
        .expect("normalized path comparison must succeed");
        assert!(!reconciled.changed);
    }
}
