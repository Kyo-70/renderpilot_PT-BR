//! Bounded, exact-member 7z validation and in-memory staging.

use std::collections::{HashMap, HashSet};
use std::io::{Cursor, Read};
use std::path::{Component, Path};

use sevenz_rust2::{Archive, BlockDecoder, Password};
use sha2::{Digest, Sha256};

use crate::{ServiceError, failed};

use super::types::{OptiScalerArchiveMember, OptiScalerRelease};

const MAX_ARCHIVE_BYTES: u64 = 128 * 1024 * 1024;
const MAX_MEMBER_BYTES: u64 = 128 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 384 * 1024 * 1024;
const MAX_MEMBERS: usize = 256;

#[derive(Debug)]
pub(crate) struct PreparedArchive {
    files: HashMap<String, Vec<u8>>,
}

impl PreparedArchive {
    #[cfg(test)]
    pub(crate) fn from_files(files: HashMap<String, Vec<u8>>) -> Self {
        Self {
            files: files
                .into_iter()
                .map(|(path, bytes)| (normalized_archive_key(&path), bytes))
                .collect(),
        }
    }

    pub(crate) fn bytes(&self, archive_path: &str) -> Result<&[u8], ServiceError> {
        self.files
            .get(&normalized_archive_key(archive_path))
            .map(Vec::as_slice)
            .ok_or_else(|| failed(format!("staged archive member is missing: {archive_path}")))
    }
}

pub(crate) fn validate_and_stage(
    archive: &[u8],
    release: &OptiScalerRelease,
    modules: &HashSet<String>,
) -> Result<PreparedArchive, ServiceError> {
    validate_archive_identity(archive, release)?;
    if release.members.len() > MAX_MEMBERS {
        return Err(failed("OptiScaler archive declares too many members"));
    }

    let expected: HashMap<_, _> = release
        .members
        .iter()
        .map(|member| (normalized_archive_key(&member.archive_path), member))
        .collect();
    let selected: HashSet<_> = expected
        .iter()
        .filter(|(_, member)| modules.contains(&member.module))
        .map(|(path, _)| path.clone())
        .collect();
    let password = Password::empty();
    let mut source = Cursor::new(archive);
    let metadata = Archive::read(&mut source, &password)
        .map_err(|error| failed(format!("invalid OptiScaler 7z archive: {error}")))?;
    let mut files = HashMap::new();
    let mut names = HashSet::new();
    let mut seen_expected = HashSet::new();
    let mut declared_total = 0_u64;
    let mut staged_total = 0_u64;

    // Validate the archive table before decoding any content. Non-payload
    // entries are deliberately absent from the manifest: the complete
    // archive digest already authenticates them, and RenderPilot never
    // extracts them. Keeping them out of the install contract avoids useless
    // per-file hashing and staging while retaining path/bomb defenses.
    if metadata.files.len() > MAX_MEMBERS {
        return Err(failed("OptiScaler archive contains too many members"));
    }
    for entry in &metadata.files {
        let name = entry.name().replace('\\', "/");
        let key = normalized_archive_key(&name);
        if !safe_archive_path(&name) || !names.insert(key.clone()) {
            return Err(failed(format!(
                "unsafe, duplicate, or case-colliding 7z member `{name}`"
            )));
        }
        // `windows_attributes` also carries the Unix file-kind bits in
        // archives created on Unix. Do not gate the check on
        // `has_windows_attributes`, otherwise a symlink can be mistaken for
        // an ordinary file.
        if entry.is_anti_item() || is_link_or_reparse(entry.windows_attributes) {
            return Err(failed(format!(
                "links, anti-items, and reparse points are forbidden: `{name}`"
            )));
        }
        declared_total = declared_total.saturating_add(entry.size);
        if entry.size > MAX_MEMBER_BYTES || declared_total > MAX_TOTAL_BYTES {
            return Err(failed(format!(
                "oversized OptiScaler archive contents at `{name}`"
            )));
        }
        let Some(member) = expected.get(&key) else {
            continue;
        };
        if entry.is_directory() || !entry.has_stream() || entry.size != member.size {
            return Err(failed(format!(
                "invalid size or file kind for archive member `{name}`"
            )));
        }
        seen_expected.insert(key);
    }

    let missing_payload = release
        .members
        .iter()
        .find(|member| !seen_expected.contains(&normalized_archive_key(&member.archive_path)));
    if let Some(member) = missing_payload {
        return Err(failed(format!(
            "OptiScaler archive is missing `{}`",
            member.archive_path
        )));
    }

    for block_index in 0..metadata.blocks.len() {
        let decoder = BlockDecoder::new(1, block_index, &metadata, &password, &mut source);
        let mut remaining_selected = decoder
            .entries()
            .iter()
            .filter(|entry| selected.contains(&normalized_archive_key(entry.name())))
            .count();
        if remaining_selected == 0 {
            // A folder containing only README/license/setup or unselected
            // modules does not need to be inflated at all.
            continue;
        }

        let mut validation_error = None;
        decoder
            .for_each_entries(&mut |entry, contents| {
                let name = entry.name().replace('\\', "/");
                let key = normalized_archive_key(&name);
                let Some(member) = expected.get(&key) else {
                    // Solid blocks cannot seek past preceding entries. Drain
                    // only what is necessary to reach a selected payload and
                    // never retain or hash these non-payload bytes.
                    if let Err(error) = std::io::copy(contents, &mut std::io::sink()) {
                        validation_error = Some(failed(format!(
                            "failed to advance past non-payload archive member `{name}`: {error}"
                        )));
                        return Ok(false);
                    }
                    return Ok(true);
                };
                if !selected.contains(&key) {
                    if let Err(error) = std::io::copy(contents, &mut std::io::sink()) {
                        validation_error = Some(failed(format!(
                            "failed to advance past unselected archive member `{name}`: {error}"
                        )));
                        return Ok(false);
                    }
                    return Ok(true);
                }

                let capacity = usize::try_from(member.size).unwrap_or(0);
                let mut bytes = Vec::with_capacity(capacity);
                if let Err(error) = contents.take(member.size + 1).read_to_end(&mut bytes) {
                    validation_error = Some(failed(format!("failed to read `{name}`: {error}")));
                    return Ok(false);
                }
                staged_total = staged_total.saturating_add(bytes.len() as u64);
                if staged_total > MAX_TOTAL_BYTES
                    || bytes.len() as u64 != member.size
                    || sha256_hex(&bytes) != member.sha256
                {
                    validation_error = Some(failed(format!(
                        "size or SHA-256 mismatch for archive member `{name}`"
                    )));
                    return Ok(false);
                }
                if member.pe_x64 && !is_x64_pe(&bytes) {
                    validation_error = Some(failed(format!(
                        "archive member `{name}` is not an x64 PE image"
                    )));
                    return Ok(false);
                }
                files.insert(key, bytes);
                remaining_selected -= 1;
                Ok(remaining_selected != 0)
            })
            .map_err(|error| failed(format!("failed to decode OptiScaler archive: {error}")))?;
        if let Some(error) = validation_error {
            return Err(error);
        }
        if remaining_selected != 0 {
            return Err(failed(
                "selected OptiScaler archive payload was not decoded",
            ));
        }
    }

    if selected.iter().any(|path| !files.contains_key(path)) {
        return Err(failed("selected OptiScaler archive payload is missing"));
    }
    Ok(PreparedArchive { files })
}

/// Reads one verified member from an already pinned immutable archive without
/// inflating the unrelated solid DLL block. This is used only for the previous
/// release's INI base during a version transition; all members of the
/// release being installed still go through [`validate_and_stage`].
pub(crate) fn read_verified_member(
    archive: &[u8],
    release: &OptiScalerRelease,
    archive_path: &str,
) -> Result<Vec<u8>, ServiceError> {
    validate_archive_identity(archive, release)?;
    let key = normalized_archive_key(archive_path);
    let member = release
        .members
        .iter()
        .find(|member| normalized_archive_key(&member.archive_path) == key)
        .ok_or_else(|| {
            failed(format!(
                "release {} has no member {archive_path}",
                release.id
            ))
        })?;
    if member.size > MAX_MEMBER_BYTES {
        return Err(failed(format!("oversized archive member `{archive_path}`")));
    }

    let password = Password::empty();
    let mut source = Cursor::new(archive);
    let metadata = Archive::read(&mut source, &password)
        .map_err(|error| failed(format!("invalid OptiScaler 7z archive: {error}")))?;
    let mut found = None;
    let mut validation_error = None;
    for block_index in 0..metadata.blocks.len() {
        let decoder = BlockDecoder::new(1, block_index, &metadata, &password, &mut source);
        if !decoder
            .entries()
            .iter()
            .any(|entry| normalized_archive_key(entry.name()) == key)
        {
            continue;
        }
        decoder
            .for_each_entries(&mut |entry, contents| {
                let name = entry.name().replace('\\', "/");
                if !safe_archive_path(&name)
                    || entry.is_anti_item()
                    || is_link_or_reparse(entry.windows_attributes)
                {
                    validation_error = Some(failed(format!(
                        "unsafe member encountered while reading `{archive_path}`: `{name}`"
                    )));
                    std::io::copy(contents, &mut std::io::sink())?;
                    return Ok(true);
                }
                if normalized_archive_key(&name) != key {
                    std::io::copy(contents, &mut std::io::sink())?;
                    return Ok(true);
                }
                if entry.size != member.size {
                    validation_error = Some(failed(format!(
                        "size mismatch for archive member `{archive_path}`"
                    )));
                    std::io::copy(contents, &mut std::io::sink())?;
                    return Ok(true);
                }
                let capacity = usize::try_from(member.size).unwrap_or(0);
                let mut bytes = Vec::with_capacity(capacity);
                if let Err(error) = contents.read_to_end(&mut bytes) {
                    validation_error = Some(failed(format!(
                        "failed to read archive member `{archive_path}`: {error}"
                    )));
                    return Ok(true);
                }
                if bytes.len() as u64 != member.size
                    || sha256_hex(&bytes) != member.sha256
                    || (member.pe_x64 && !is_x64_pe(&bytes))
                {
                    validation_error = Some(failed(format!(
                        "identity mismatch for archive member `{archive_path}`"
                    )));
                    return Ok(true);
                }
                found = Some(bytes);
                Ok(true)
            })
            .map_err(|error| failed(format!("failed to decode OptiScaler archive: {error}")))?;
        break;
    }
    if let Some(error) = validation_error {
        return Err(error);
    }
    found.ok_or_else(|| failed(format!("archive is missing `{archive_path}`")))
}

fn validate_archive_identity(
    archive: &[u8],
    release: &OptiScalerRelease,
) -> Result<(), ServiceError> {
    if archive.len() as u64 != release.archive_size
        || archive.len() as u64 > MAX_ARCHIVE_BYTES
        || sha256_hex(archive) != release.archive_sha256
    {
        return Err(failed(format!(
            "OptiScaler archive identity mismatch for {}",
            release.id
        )));
    }
    Ok(())
}

fn safe_archive_path(value: &str) -> bool {
    let path = Path::new(value);
    !value.is_empty()
        && !value.contains('\0')
        && !value.contains(':')
        && !path.is_absolute()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

const fn is_link_or_reparse(attributes: u32) -> bool {
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    let unix_kind = (attributes >> 16) & 0xf000;
    attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 || unix_kind == 0xa000
}

pub(crate) fn is_x64_pe(bytes: &[u8]) -> bool {
    if bytes.len() < 0x40 || &bytes[..2] != b"MZ" {
        return false;
    }
    let pe_offset =
        u32::from_le_bytes([bytes[0x3c], bytes[0x3d], bytes[0x3e], bytes[0x3f]]) as usize;
    pe_offset
        .checked_add(26)
        .is_some_and(|end| end <= bytes.len())
        && &bytes[pe_offset..pe_offset + 4] == b"PE\0\0"
        && u16::from_le_bytes([bytes[pe_offset + 4], bytes[pe_offset + 5]]) == 0x8664
        && u16::from_le_bytes([bytes[pe_offset + 24], bytes[pe_offset + 25]]) == 0x20b
}

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn normalized_archive_key(value: &str) -> String {
    value.replace('\\', "/").to_ascii_lowercase()
}

pub(crate) fn selected_members<'a>(
    release: &'a OptiScalerRelease,
    modules: &HashSet<String>,
) -> impl Iterator<Item = &'a OptiScalerArchiveMember> {
    release
        .members
        .iter()
        .filter(move |member| modules.contains(&member.module))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selectively_reads_a_verified_member_from_a_solid_archive() {
        let temp = tempfile::tempdir().expect("tempdir");
        let source = temp.path().join("source");
        std::fs::create_dir(&source).expect("source dir");
        let config = b"[FSR]\nFsr4Preset=auto\n";
        let payload = vec![0x5a; 64 * 1024];
        std::fs::write(source.join("OptiScaler.ini"), config).expect("config");
        std::fs::write(source.join("payload.dll"), &payload).expect("payload");
        let archive_path = temp.path().join("fixture.7z");
        sevenz_rust2::compress_to_path(&source, &archive_path).expect("compress fixture");
        let archive = std::fs::read(&archive_path).expect("archive bytes");
        let release = OptiScalerRelease {
            id: "fixture".to_owned(),
            source: super::super::types::OptiScalerReleaseSource {
                provider: super::super::types::OptiScalerReleaseProvider::GithubRelease,
                repository: "optiscaler/OptiScaler".to_owned(),
                tag: "fixture".to_owned(),
                asset: "fixture.7z".to_owned(),
            },
            archive_sha256: sha256_hex(&archive),
            archive_size: archive.len() as u64,
            config_schema: 1,
            members: vec![
                OptiScalerArchiveMember {
                    archive_path: "OptiScaler.ini".to_owned(),
                    target: "OptiScaler.ini".to_owned(),
                    sha256: sha256_hex(config),
                    size: config.len() as u64,
                    module: "core".to_owned(),
                    pe_x64: false,
                },
                OptiScalerArchiveMember {
                    archive_path: "payload.dll".to_owned(),
                    target: "payload.dll".to_owned(),
                    sha256: sha256_hex(&payload),
                    size: payload.len() as u64,
                    module: "core".to_owned(),
                    pe_x64: false,
                },
            ],
        };

        assert_eq!(
            read_verified_member(&archive, &release, "OptiScaler.ini").expect("verified config"),
            config
        );
    }

    #[test]
    fn stages_selected_payload_and_discards_non_payload_members() {
        let temp = tempfile::tempdir().expect("tempdir");
        let source = temp.path().join("source");
        std::fs::create_dir(&source).expect("source dir");
        let payload = b"payload";
        let optional = b"optional";
        std::fs::write(source.join("payload.dll"), payload).expect("payload");
        std::fs::write(source.join("optional.dll"), optional).expect("optional");
        std::fs::write(source.join("LICENSE.txt"), b"license text").expect("license");
        let archive_path = temp.path().join("fixture.7z");
        let mut writer = sevenz_rust2::ArchiveWriter::create(&archive_path).expect("create writer");
        writer
            .push_source_path_non_solid(&source, |_| true)
            .expect("pack fixture");
        writer.finish().expect("finish fixture");
        let archive = std::fs::read(&archive_path).expect("archive bytes");
        let release = OptiScalerRelease {
            id: "fixture-selected".to_owned(),
            source: super::super::types::OptiScalerReleaseSource {
                provider: super::super::types::OptiScalerReleaseProvider::GithubRelease,
                repository: "optiscaler/OptiScaler".to_owned(),
                tag: "fixture-selected".to_owned(),
                asset: "fixture-selected.7z".to_owned(),
            },
            archive_sha256: sha256_hex(&archive),
            archive_size: archive.len() as u64,
            config_schema: 1,
            members: vec![
                OptiScalerArchiveMember {
                    archive_path: "payload.dll".to_owned(),
                    target: "payload.dll".to_owned(),
                    sha256: sha256_hex(payload),
                    size: payload.len() as u64,
                    module: "core".to_owned(),
                    pe_x64: false,
                },
                OptiScalerArchiveMember {
                    archive_path: "optional.dll".to_owned(),
                    target: "optional.dll".to_owned(),
                    sha256: sha256_hex(optional),
                    size: optional.len() as u64,
                    module: "optional".to_owned(),
                    pe_x64: false,
                },
            ],
        };
        let modules = HashSet::from(["core".to_owned()]);
        let staged = validate_and_stage(&archive, &release, &modules).expect("stage archive");

        assert_eq!(staged.bytes("payload.dll").expect("payload"), payload);
        assert!(staged.bytes("optional.dll").is_err());
        assert!(staged.bytes("LICENSE.txt").is_err());
    }

    #[test]
    fn rejects_parent_absolute_ads_and_empty_paths() {
        for path in ["", "../evil.dll", "/evil.dll", "C:/evil.dll", "safe:file"] {
            assert!(!safe_archive_path(path), "{path}");
        }
        assert!(safe_archive_path("OptiScaler/libxess.dll"));
    }
}
