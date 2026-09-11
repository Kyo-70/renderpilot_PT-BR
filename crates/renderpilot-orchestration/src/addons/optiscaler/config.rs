//! Exact final projection for the OptiScaler ReShade-chain setting.
//!
//! This reducer is intentionally narrower than the later OptiScaler lifecycle:
//! it changes one semantic key and preserves the input document's lines,
//! comments, key spelling, duplicate entries, section order, newline style,
//! and trailing-newline convention.  It is equivalent to the final projection
//! of the full merger when old, current, and new documents are the same.

#[derive(Debug, Clone)]
struct IniLine {
    raw: String,
    section: String,
    key: Option<String>,
}

/// Changes only `Plugins.LoadReshade` while preserving unrelated bytes and
/// the exact final-projection semantics used by the full OptiScaler merger.
pub(crate) fn set_reshade_chain_enabled(bytes: &[u8], enabled: bool) -> Vec<u8> {
    let text = String::from_utf8_lossy(bytes);
    let newline = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let final_newline = text.ends_with('\n');
    let mut lines = parse(&text);
    let value = if enabled { "true" } else { "false" };
    let semantic_key = key("Plugins", "LoadReshade");

    if let Some(index) = index_of(&lines, &semantic_key) {
        set_value(&mut lines[index], value);
    } else {
        insert_value(&mut lines, "Plugins", "LoadReshade", value);
    }

    let mut output = lines
        .into_iter()
        .map(|line| line.raw)
        .collect::<Vec<_>>()
        .join(newline);
    if final_newline {
        output.push_str(newline);
    }
    output.into_bytes()
}

fn parse(text: &str) -> Vec<IniLine> {
    let mut section = String::new();
    text.lines()
        .map(|raw| {
            let trimmed = raw.trim();
            if let Some(name) = trimmed.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
                name.trim().clone_into(&mut section);
                return IniLine {
                    raw: raw.to_owned(),
                    section: section.clone(),
                    key: None,
                };
            }
            let parsed = (!trimmed.starts_with(';') && !trimmed.starts_with('#'))
                .then(|| raw.split_once('='))
                .flatten();
            IniLine {
                raw: raw.to_owned(),
                section: section.clone(),
                key: parsed.map(|(key, _)| key.trim().to_owned()),
            }
        })
        .collect()
}

fn key(section: &str, setting: &str) -> String {
    format!(
        "{}\0{}",
        section.trim().to_ascii_lowercase(),
        setting.trim().to_ascii_lowercase()
    )
}

fn index_of(lines: &[IniLine], semantic_key: &str) -> Option<usize> {
    let (expected_section, expected_setting) = semantic_key.split_once('\0')?;
    lines.iter().rposition(|line| {
        line.key.as_deref().is_some_and(|setting| {
            line.section.trim().eq_ignore_ascii_case(expected_section)
                && setting.trim().eq_ignore_ascii_case(expected_setting)
        })
    })
}

fn set_value(line: &mut IniLine, value: &str) {
    let setting = line.key.as_deref().unwrap_or_default();
    line.raw = format!("{setting}={value}");
}

fn insert_value(lines: &mut Vec<IniLine>, section: &str, setting: &str, value: &str) {
    let header = format!("[{section}]");
    let section_index = lines
        .iter()
        .position(|line| line.key.is_none() && line.raw.trim().eq_ignore_ascii_case(&header));
    if section_index.is_none() {
        if lines.last().is_some_and(|line| !line.raw.is_empty()) {
            lines.push(IniLine {
                raw: String::new(),
                section: String::new(),
                key: None,
            });
        }
        lines.push(IniLine {
            raw: header,
            section: section.to_owned(),
            key: None,
        });
    }
    let start = section_index.unwrap_or(lines.len().saturating_sub(1));
    let insert_at = lines
        .iter()
        .enumerate()
        .skip(start + 1)
        .find(|(_, line)| line.key.is_none() && line.raw.trim().starts_with('['))
        .map_or(lines.len(), |(index, _)| index);
    lines.insert(
        insert_at,
        IniLine {
            raw: format!("{setting}={value}"),
            section: section.to_owned(),
            key: Some(setting.to_owned()),
        },
    );
}

#[cfg(test)]
mod tests {
    use super::set_reshade_chain_enabled;

    #[test]
    fn updates_existing_key_and_preserves_crlf_foreign_content() {
        let before = b"; keep\r\n[Plugins]\r\nOther=unchanged\r\nLoadReshade=false\r\n";
        assert_eq!(
            set_reshade_chain_enabled(before, true),
            b"; keep\r\n[Plugins]\r\nOther=unchanged\r\nLoadReshade=true\r\n"
        );
    }

    #[test]
    fn inserts_missing_key_into_existing_section_before_next_header() {
        let before = b"[Plugins]\nOther=unchanged\n[Other]\nValue=1\n";
        assert_eq!(
            set_reshade_chain_enabled(before, true),
            b"[Plugins]\nOther=unchanged\nLoadReshade=true\n[Other]\nValue=1\n"
        );
    }

    #[test]
    fn inserts_missing_section_with_document_newline_semantics() {
        assert_eq!(
            set_reshade_chain_enabled(b"[Other]\nValue=1\n", false),
            b"[Other]\nValue=1\n\n[Plugins]\nLoadReshade=false\n"
        );
        assert_eq!(
            set_reshade_chain_enabled(b"", true),
            b"[Plugins]\nLoadReshade=true"
        );
    }

    #[test]
    fn changes_only_the_last_case_insensitive_duplicate() {
        let before = b"[plugins]\nLoadReshade=false\nLoadReshade=custom\n";
        assert_eq!(
            set_reshade_chain_enabled(before, true),
            b"[plugins]\nLoadReshade=false\nLoadReshade=true\n"
        );
    }

    #[test]
    fn preserves_key_spelling_but_normalizes_assignment_spacing() {
        let before = b"[Plugins]\n  loadreshade = false ; user suffix\n";
        assert_eq!(
            set_reshade_chain_enabled(before, true),
            b"[Plugins]\nloadreshade=true\n"
        );
    }

    #[test]
    fn preserves_invalid_utf8_as_the_lossy_projection() {
        assert_eq!(
            set_reshade_chain_enabled(b"[Plugins]\n\xff", true),
            "[Plugins]\n�\nLoadReshade=true".as_bytes()
        );
    }
}
