//! Registered-package discovery for Xbox app and Microsoft Store games.
//!
//! A package root is accepted only when Windows reports it for the current
//! user. `MicrosoftGame.config` is the package's game-specific metadata; an
//! `AppxManifest.xml` is used solely as a fallback for its executable record.
//! Files carrying either name in an arbitrary folder are deliberately not
//! treated as Xbox evidence.

use std::{
    borrow::Cow,
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use quick_xml::{Reader, XmlVersion, events::Event};
use renderpilot_domain::Launcher;

use crate::{
    executable_detection::is_readable_windows_pe_executable,
    install_identity::InstallIdentityDetails, path_normalize::canonicalize_install_path,
};

use super::{DiscoveredInstall, DiscoveredSources};

const MICROSOFT_GAME_CONFIG: &str = "MicrosoftGame.config";
const APPX_MANIFEST: &str = "AppxManifest.xml";

/// Returns the launcher identity only when `root` is one of Windows' currently
/// registered package roots. A similarly shaped loose folder remains manual.
pub(crate) fn identity_for_registered_root(root: &Path) -> Option<InstallIdentityDetails> {
    registered_package_metadata_for_root_from_packages(root, current_user_packages())
        .map(|metadata| metadata.identity())
}

/// Returns the exact launcher executable for a registered package root.
pub(crate) fn launch_executable_for_registered_root(root: &Path) -> Option<String> {
    launch_executable_for_registered_root_from_packages(root, current_user_packages())
}

fn launch_executable_for_registered_root_from_packages(
    root: &Path,
    packages: impl IntoIterator<Item = RegisteredPackage>,
) -> Option<String> {
    registered_package_metadata_for_root_from_packages(root, packages).and_then(|metadata| {
        Path::new(metadata.executable.as_deref()?)
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_owned)
    })
}

/// Discovers game roots reported by the current user's registered packages.
///
/// Package-manager access and package-root reads are best effort. A denied or
/// malformed package is not transformed into an inferred path.
pub(super) fn discover_registered_packages() -> DiscoveredSources {
    discovered_sources_from_registered_packages(current_user_packages())
}

fn discovered_sources_from_registered_packages(
    packages: impl IntoIterator<Item = RegisteredPackage>,
) -> DiscoveredSources {
    // A package family can have resource siblings. Pick only after a root has
    // proven it is a game package, so a lexically earlier resource root cannot
    // suppress the actual executable-bearing package.
    let mut candidates = BTreeMap::<String, (RegisteredPackage, XboxPackageMetadata)>::new();
    for package in packages {
        let Some(metadata) = metadata_for_package_root(&package.root) else {
            continue;
        };
        if metadata.executable.is_none() {
            continue;
        }
        let key = package.family_name.to_ascii_lowercase();
        match candidates.get(&key) {
            Some((existing, _))
                if comparable_root(&existing.root) <= comparable_root(&package.root) => {}
            _ => {
                candidates.insert(key, (package, metadata));
            }
        }
    }
    let game_installs = candidates
        .into_values()
        .map(|(package, metadata)| {
            DiscoveredInstall::with_identity(package.root, metadata.identity())
        })
        .collect();

    DiscoveredSources {
        game_installs,
        ..DiscoveredSources::default()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RegisteredPackage {
    root: PathBuf,
    /// A package-family name is only an in-memory enumeration/dedup key. It is
    /// never persisted in `GameIdentity.external_id`, whose Xbox contract is a
    /// validated MicrosoftGame.config StoreId only.
    family_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct XboxPackageMetadata {
    store_id: Option<String>,
    executable: Option<String>,
}

impl XboxPackageMetadata {
    fn identity(&self) -> InstallIdentityDetails {
        InstallIdentityDetails {
            launcher: Launcher::Xbox,
            external_id: self.store_id.clone(),
            display_name: None,
        }
    }
}

fn registered_package_metadata_for_root_from_packages(
    root: &Path,
    packages: impl IntoIterator<Item = RegisteredPackage>,
) -> Option<XboxPackageMetadata> {
    let expected_root = canonicalize_install_path(root).ok()?;
    packages
        .into_iter()
        .find(|package| canonical_root_matches(&package.root, &expected_root))
        .and_then(|package| metadata_for_package_root(&package.root))
}

fn canonical_root_matches(package_root: &Path, canonical_expected: &Path) -> bool {
    canonicalize_install_path(package_root)
        .is_ok_and(|canonical_package| canonical_package == canonical_expected)
}

#[cfg(test)]
fn deduplicate_registered_packages(
    packages: impl IntoIterator<Item = RegisteredPackage>,
) -> Vec<RegisteredPackage> {
    let mut by_family = BTreeMap::<String, RegisteredPackage>::new();
    for package in packages {
        let family_key = package.family_name.to_ascii_lowercase();
        match by_family.get(&family_key) {
            Some(existing) if comparable_root(&existing.root) <= comparable_root(&package.root) => {
            }
            _ => {
                by_family.insert(family_key, package);
            }
        }
    }
    by_family.into_values().collect()
}

fn comparable_root(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "/")
        .to_ascii_lowercase()
}

fn metadata_for_package_root(root: &Path) -> Option<XboxPackageMetadata> {
    let config_path = root.join(MICROSOFT_GAME_CONFIG);
    let config = read_root_regular_text(&config_path)?;
    let config = parse_microsoft_game_config(&config).ok();

    let executable = match config.as_ref().map(|config| {
        resolve_declared_executable(
            root,
            &config.executables,
            config.target_device_family.as_deref(),
        )
    }) {
        Some(DeclaredExecutableResolution::One(executable)) => Some(executable),
        Some(DeclaredExecutableResolution::Ambiguous) => None,
        Some(DeclaredExecutableResolution::None) | None => appx_executable(root),
    };

    Some(XboxPackageMetadata {
        store_id: config.and_then(|config| config.store_id),
        executable,
    })
}

fn read_root_regular_text(path: &Path) -> Option<String> {
    let metadata = fs::symlink_metadata(path).ok()?;
    (metadata.is_file() && !metadata.file_type().is_symlink())
        .then(|| fs::read_to_string(path).ok())
        .flatten()
}

fn appx_executable(root: &Path) -> Option<String> {
    let manifest = read_root_regular_text(&root.join(APPX_MANIFEST))?;
    let manifest = parse_appx_manifest(&manifest).ok()?;
    resolve_declared_executable(
        root,
        &manifest.executables,
        manifest.target_device_family.as_deref(),
    )
    .one()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedPackageMetadata {
    store_id: Option<String>,
    executables: Vec<DeclaredExecutable>,
    target_device_family: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DeclaredExecutable {
    name: String,
    is_dev_only: bool,
    target_device_family: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum DeclaredExecutableResolution {
    None,
    One(String),
    Ambiguous,
}

impl DeclaredExecutableResolution {
    fn one(self) -> Option<String> {
        match self {
            Self::One(executable) => Some(executable),
            Self::None | Self::Ambiguous => None,
        }
    }
}

/// Parses MicrosoftGame.config without doing filesystem I/O.
fn parse_microsoft_game_config(content: &str) -> Result<ParsedPackageMetadata, ()> {
    let mut reader = Reader::from_str(content);
    reader.config_mut().trim_text(true);
    let mut buffer = Vec::new();
    let mut executable_list_depth = 0_u32;
    let mut document_depth = 0_u32;
    let mut game_root_seen = false;
    let mut list_target_device_family = None;
    let mut store_id_text_depth = None;
    let mut store_id = StoreIdObservation::Absent;
    let mut executables = Vec::new();

    loop {
        match reader.read_event_into(&mut buffer).map_err(|_| ())? {
            Event::Start(element) => {
                let element_name = element.name();
                let name = local_name(element_name.as_ref());
                document_depth = document_depth.checked_add(1).ok_or(())?;
                if document_depth == 1 && name.eq_ignore_ascii_case("Game") {
                    game_root_seen = true;
                }
                if name.eq_ignore_ascii_case("ExecutableList") {
                    executable_list_depth = executable_list_depth.checked_add(1).ok_or(())?;
                    if executable_list_depth == 1 {
                        list_target_device_family = attribute_value(&element, "TargetDeviceFamily");
                    }
                } else if name.eq_ignore_ascii_case("Executable") && executable_list_depth > 0 {
                    if let Some(executable) =
                        executable_from_element(&element, list_target_device_family.as_deref())
                    {
                        executables.push(executable);
                    }
                } else if name.eq_ignore_ascii_case("StoreId") {
                    store_id_text_depth = Some(executable_list_depth);
                }
                if let Some(value) = attribute_value(&element, "StoreId") {
                    record_store_id(&mut store_id, &value);
                }
            }
            Event::Empty(element) => {
                let element_name = element.name();
                let name = local_name(element_name.as_ref());
                if document_depth == 0 && name.eq_ignore_ascii_case("Game") {
                    game_root_seen = true;
                }
                if name.eq_ignore_ascii_case("Executable")
                    && executable_list_depth > 0
                    && let Some(executable) =
                        executable_from_element(&element, list_target_device_family.as_deref())
                {
                    executables.push(executable);
                }
                if let Some(value) = attribute_value(&element, "StoreId") {
                    record_store_id(&mut store_id, &value);
                }
            }
            Event::Text(text) if store_id_text_depth.is_some() => {
                record_store_id(&mut store_id, text.as_ref());
            }
            Event::End(element) => {
                let element_name = element.name();
                let name = local_name(element_name.as_ref());
                if name.eq_ignore_ascii_case("StoreId") {
                    store_id_text_depth = None;
                } else if name.eq_ignore_ascii_case("ExecutableList") {
                    executable_list_depth = executable_list_depth.checked_sub(1).ok_or(())?;
                    if executable_list_depth == 0 {
                        list_target_device_family = None;
                    }
                }
                document_depth = document_depth.checked_sub(1).ok_or(())?;
            }
            Event::Eof => {
                if document_depth != 0 || !game_root_seen {
                    return Err(());
                }
                break;
            }
            _ => {}
        }
        buffer.clear();
    }

    Ok(ParsedPackageMetadata {
        store_id: store_id.into_option(),
        executables,
        target_device_family: None,
    })
}

/// Parses AppxManifest executable metadata without doing filesystem I/O.
fn parse_appx_manifest(content: &str) -> Result<ParsedPackageMetadata, ()> {
    let mut reader = Reader::from_str(content);
    reader.config_mut().trim_text(true);
    let mut buffer = Vec::new();
    let mut executables = Vec::new();
    let mut has_target_device_family = false;
    let mut has_pc_family = false;
    let mut document_depth = 0_u32;
    let mut package_root_seen = false;

    loop {
        match reader.read_event_into(&mut buffer).map_err(|_| ())? {
            Event::Start(element) => {
                let element_name = element.name();
                let name = local_name(element_name.as_ref());
                document_depth = document_depth.checked_add(1).ok_or(())?;
                if document_depth == 1 && name.eq_ignore_ascii_case("Package") {
                    package_root_seen = true;
                }
                if name.eq_ignore_ascii_case("TargetDeviceFamily") {
                    if let Some(value) = attribute_value(&element, "Name") {
                        has_target_device_family = true;
                        if is_pc_target_device_family(&value) {
                            has_pc_family = true;
                        }
                    }
                } else if name.eq_ignore_ascii_case("Application")
                    && let Some(executable) = executable_from_element(&element, None)
                {
                    executables.push(executable);
                }
            }
            Event::Empty(element) => {
                let element_name = element.name();
                let name = local_name(element_name.as_ref());
                if document_depth == 0 && name.eq_ignore_ascii_case("Package") {
                    package_root_seen = true;
                }
                if name.eq_ignore_ascii_case("TargetDeviceFamily") {
                    if let Some(value) = attribute_value(&element, "Name") {
                        has_target_device_family = true;
                        if is_pc_target_device_family(&value) {
                            has_pc_family = true;
                        }
                    }
                } else if name.eq_ignore_ascii_case("Application")
                    && let Some(executable) = executable_from_element(&element, None)
                {
                    executables.push(executable);
                }
            }
            Event::End(_) => {
                document_depth = document_depth.checked_sub(1).ok_or(())?;
            }
            Event::Eof => {
                if document_depth != 0 || !package_root_seen {
                    return Err(());
                }
                break;
            }
            _ => {}
        }
        buffer.clear();
    }

    let target_device_family = if !has_target_device_family {
        None
    } else if has_pc_family {
        Some("PC".to_owned())
    } else {
        Some("unsupported".to_owned())
    };

    Ok(ParsedPackageMetadata {
        store_id: None,
        executables,
        target_device_family,
    })
}

fn executable_from_element(
    element: &quick_xml::events::BytesStart<'_>,
    inherited_target_device_family: Option<&str>,
) -> Option<DeclaredExecutable> {
    let name =
        attribute_value(element, "Name").or_else(|| attribute_value(element, "Executable"))?;
    Some(DeclaredExecutable {
        name,
        is_dev_only: attribute_value(element, "IsDevOnly")
            .is_some_and(|value| value.eq_ignore_ascii_case("true")),
        target_device_family: attribute_value(element, "TargetDeviceFamily")
            .or_else(|| inherited_target_device_family.map(str::to_owned)),
    })
}

fn local_name(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}

fn attribute_value(element: &quick_xml::events::BytesStart<'_>, expected: &str) -> Option<String> {
    element
        .attributes()
        .with_checks(false)
        .filter_map(Result::ok)
        .find(|attribute| local_name(attribute.key.as_ref()).eq_ignore_ascii_case(expected))
        .and_then(|attribute| {
            attribute
                .normalized_value(XmlVersion::Implicit1_0)
                .ok()
                .map(Cow::into_owned)
        })
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum StoreIdObservation {
    Absent,
    Exact(String),
    InvalidOrConflicting,
}

impl StoreIdObservation {
    fn into_option(self) -> Option<String> {
        match self {
            Self::Exact(value) => Some(value),
            Self::Absent | Self::InvalidOrConflicting => None,
        }
    }
}

fn record_store_id(store_id: &mut StoreIdObservation, value: &str) {
    let Some(value) = canonical_store_id(value) else {
        *store_id = StoreIdObservation::InvalidOrConflicting;
        return;
    };
    match store_id {
        StoreIdObservation::Absent => *store_id = StoreIdObservation::Exact(value),
        StoreIdObservation::Exact(existing) if existing == &value => {}
        StoreIdObservation::Exact(_) | StoreIdObservation::InvalidOrConflicting => {
            *store_id = StoreIdObservation::InvalidOrConflicting;
        }
    }
}

pub(crate) fn canonical_store_id(value: &str) -> Option<String> {
    let value = value.trim();
    (value.len() == 12 && value.bytes().all(|byte| byte.is_ascii_alphanumeric()))
        .then(|| value.to_ascii_uppercase())
}

fn resolve_declared_executable(
    root: &Path,
    declarations: &[DeclaredExecutable],
    inherited_target_device_family: Option<&str>,
) -> DeclaredExecutableResolution {
    let mut executable_paths = BTreeMap::<String, String>::new();
    for declaration in declarations {
        if declaration.is_dev_only
            || !is_pc_target_device_family(
                declaration
                    .target_device_family
                    .as_deref()
                    .or(inherited_target_device_family)
                    .unwrap_or_default(),
            )
        {
            continue;
        }
        let Some(relative) = normalized_relative_executable(&declaration.name) else {
            continue;
        };
        if declared_executable_is_regular_pe(root, &relative) {
            executable_paths
                .entry(relative.to_ascii_lowercase())
                .or_insert(relative);
        }
    }
    match executable_paths.len() {
        0 => DeclaredExecutableResolution::None,
        1 => DeclaredExecutableResolution::One(
            executable_paths
                .into_values()
                .next()
                .expect("one executable exists"),
        ),
        _ => DeclaredExecutableResolution::Ambiguous,
    }
}

fn is_pc_target_device_family(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "" | "pc" | "desktop" | "windows.pc" | "windows.desktop" | "windows.universal"
    )
}

fn normalized_relative_executable(value: &str) -> Option<String> {
    let trimmed = value.trim();
    let value = if trimmed.contains('\\') {
        trimmed.replace('\\', "/")
    } else {
        trimmed.to_owned()
    };
    if value.is_empty() || value.starts_with('/') || value.contains(':') {
        return None;
    }
    let mut has_segments = false;
    let mut last_segment = "";
    for segment in value.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." || segment.contains('\0') {
            return None;
        }
        has_segments = true;
        last_segment = segment;
    }
    if !has_segments {
        return None;
    }
    super::paths::has_extension_ignore_ascii_case(Path::new(last_segment), "exe").then_some(value)
}

fn declared_executable_is_regular_pe(root: &Path, relative: &str) -> bool {
    let mut path = root.to_path_buf();
    let mut segments = relative.split('/').peekable();
    while let Some(segment) = segments.next() {
        path.push(segment);
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            return false;
        };
        if metadata.file_type().is_symlink() {
            return false;
        }
        if segments.peek().is_none() {
            if !metadata.is_file() {
                return false;
            }
        } else if !metadata.is_dir() {
            return false;
        }
    }
    is_readable_windows_pe_executable(&path)
}

fn current_user_packages() -> Vec<RegisteredPackage> {
    use windows::{ApplicationModel::Package, Management::Deployment::PackageManager};

    let Ok(manager) = PackageManager::new() else {
        return Vec::new();
    };
    let Ok(packages) = manager.FindPackages() else {
        return Vec::new();
    };
    packages
        .into_iter()
        .filter_map(|package: Package| package_to_registered_package(&package))
        .collect()
}

fn package_to_registered_package(
    package: &windows::ApplicationModel::Package,
) -> Option<RegisteredPackage> {
    if package.IsFramework().ok()? || package.IsResourcePackage().ok()? {
        return None;
    }
    let family_name = package.Id().ok()?.FamilyName().ok()?.to_string();
    if family_name.trim().is_empty() {
        return None;
    }
    let root = package.InstalledLocation().ok()?.Path().ok()?.to_string();
    let root = canonicalize_install_path(Path::new(&root)).ok()?;
    Some(RegisteredPackage { root, family_name })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;

    fn pe_bytes() -> Vec<u8> {
        let mut bytes = vec![0_u8; 0x84];
        bytes[..2].copy_from_slice(b"MZ");
        bytes[0x3c..0x40].copy_from_slice(&0x80_u32.to_le_bytes());
        bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
        bytes
    }

    fn write_pe(root: &Path, relative: &str) {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().expect("parent")).expect("parent");
        fs::write(path, pe_bytes()).expect("PE");
    }

    fn config(store_id: &str, executables: &str) -> String {
        format!(
            r#"<Game><StoreId>{store_id}</StoreId><ExecutableList>{executables}</ExecutableList></Game>"#
        )
    }

    #[test]
    fn config_canonicalizes_a_valid_store_id_and_one_pc_executable() {
        let parsed = parse_microsoft_game_config(&config(
            "9mw53zkzh168",
            r#"<Executable Name="Bin/Game.exe" TargetDeviceFamily="Windows.Desktop" />"#,
        ))
        .expect("config");

        assert_eq!(parsed.store_id.as_deref(), Some("9MW53ZKZH168"));
        assert_eq!(parsed.executables.len(), 1);
        assert_eq!(parsed.executables[0].name, "Bin/Game.exe");
    }

    #[test]
    fn invalid_or_conflicting_store_id_never_becomes_an_external_identity() {
        let invalid = parse_microsoft_game_config(&config(
            "not-a-store-id",
            r#"<Executable Name="Game.exe" />"#,
        ))
        .expect("well-formed config");
        assert_eq!(invalid.store_id, None);

        let conflicting = parse_microsoft_game_config(
            r#"<Game><StoreId>9MW53ZKZH168</StoreId><StoreId>9NK8947Z28XL</StoreId><StoreId>9MW53ZKZH168</StoreId><ExecutableList><Executable Name="Game.exe" /></ExecutableList></Game>"#,
        )
        .expect("well-formed config");
        assert_eq!(conflicting.store_id, None);
    }

    #[test]
    fn config_excludes_dev_and_console_executables() {
        let parsed = parse_microsoft_game_config(&config(
            "9MW53ZKZH168",
            r#"
                <Executable Name="Dev.exe" IsDevOnly="true" />
                <Executable Name="Console.exe" TargetDeviceFamily="Xbox" />
                <Executable Name="Game.exe" />
            "#,
        ))
        .expect("config");
        let root = tempdir().expect("root");
        write_pe(root.path(), "Game.exe");
        assert_eq!(
            resolve_declared_executable(root.path(), &parsed.executables, None),
            DeclaredExecutableResolution::One("Game.exe".to_owned())
        );
    }

    #[test]
    fn malformed_xml_is_rejected() {
        assert!(parse_microsoft_game_config("<Game><Executable").is_err());
        assert!(parse_appx_manifest("<Package><Applications>").is_err());
    }

    #[test]
    fn package_metadata_uses_appx_when_config_has_no_eligible_executable() {
        let root = tempdir().expect("root");
        fs::write(
            root.path().join(MICROSOFT_GAME_CONFIG),
            config(
                "9MW53ZKZH168",
                r#"<Executable Name="Console.exe" TargetDeviceFamily="Xbox" />"#,
            ),
        )
        .expect("config");
        fs::write(
            root.path().join(APPX_MANIFEST),
            r#"<Package><Dependencies><TargetDeviceFamily Name="Windows.Desktop" /></Dependencies><Applications><Application Executable="Bin/Game.exe" /></Applications></Package>"#,
        )
        .expect("appx");
        write_pe(root.path(), "Bin/Game.exe");

        let metadata = metadata_for_package_root(root.path()).expect("metadata");
        assert_eq!(metadata.store_id.as_deref(), Some("9MW53ZKZH168"));
        assert_eq!(metadata.executable.as_deref(), Some("Bin/Game.exe"));
    }

    #[test]
    fn unsafe_non_pe_and_ambiguous_declarations_fail_closed() {
        let root = tempdir().expect("root");
        write_pe(root.path(), "Good.exe");
        fs::write(root.path().join("NotPe.exe"), b"not a PE").expect("non PE");
        let declarations = vec![
            DeclaredExecutable {
                name: "../escape.exe".to_owned(),
                is_dev_only: false,
                target_device_family: None,
            },
            DeclaredExecutable {
                name: r"C:\escape.exe".to_owned(),
                is_dev_only: false,
                target_device_family: None,
            },
            DeclaredExecutable {
                name: r"\\server\share\escape.exe".to_owned(),
                is_dev_only: false,
                target_device_family: None,
            },
            DeclaredExecutable {
                name: "NotPe.exe".to_owned(),
                is_dev_only: false,
                target_device_family: None,
            },
            DeclaredExecutable {
                name: "Good.exe".to_owned(),
                is_dev_only: false,
                target_device_family: None,
            },
        ];
        assert_eq!(
            resolve_declared_executable(root.path(), &declarations, None),
            DeclaredExecutableResolution::One("Good.exe".to_owned())
        );

        write_pe(root.path(), "Second.exe");
        let mut ambiguous = declarations;
        ambiguous.push(DeclaredExecutable {
            name: "Second.exe".to_owned(),
            is_dev_only: false,
            target_device_family: None,
        });
        assert_eq!(
            resolve_declared_executable(root.path(), &ambiguous, None),
            DeclaredExecutableResolution::Ambiguous
        );
    }

    #[cfg(windows)]
    #[test]
    fn symlinked_declared_executable_is_rejected_when_the_platform_allows_links() {
        let root = tempdir().expect("root");
        write_pe(root.path(), "Game.exe");
        let link = root.path().join("Linked.exe");
        match std::os::windows::fs::symlink_file(root.path().join("Game.exe"), &link) {
            Ok(()) => {}
            // Some locked-down CI hosts do not grant the symbolic-link right;
            // the production guard is still covered directly by the metadata
            // check above, while this platform probe remains non-flaky.
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => return,
            Err(error) => panic!("create symbolic-link fixture: {error}"),
        }
        let declarations = vec![DeclaredExecutable {
            name: "Linked.exe".to_owned(),
            is_dev_only: false,
            target_device_family: None,
        }];
        assert_eq!(
            resolve_declared_executable(root.path(), &declarations, None),
            DeclaredExecutableResolution::None
        );
    }

    #[test]
    fn loose_folder_metadata_does_not_establish_xbox_identity() {
        let root = tempdir().expect("root");
        fs::write(
            root.path().join(MICROSOFT_GAME_CONFIG),
            config("9MW53ZKZH168", r#"<Executable Name="Game.exe" />"#),
        )
        .expect("config");
        write_pe(root.path(), "Game.exe");

        assert!(identity_for_registered_root(root.path()).is_none());
    }

    #[test]
    fn exact_registered_root_is_xbox_and_exposes_its_declared_launch_executable() {
        let root = tempdir().expect("root");
        fs::write(
            root.path().join(MICROSOFT_GAME_CONFIG),
            config("9MW53ZKZH168", r#"<Executable Name="Bin/Game.exe" />"#),
        )
        .expect("config");
        write_pe(root.path(), "Bin/Game.exe");
        let packages = vec![RegisteredPackage {
            root: root.path().to_path_buf(),
            family_name: "package-family".to_owned(),
        }];

        let metadata = registered_package_metadata_for_root_from_packages(root.path(), packages)
            .expect("registered package metadata");
        assert_eq!(metadata.identity().launcher, Launcher::Xbox);
        assert_eq!(
            metadata.identity().external_id.as_deref(),
            Some("9MW53ZKZH168")
        );
        assert_eq!(
            Path::new(metadata.executable.as_deref().expect("executable"))
                .file_name()
                .and_then(|name| name.to_str()),
            Some("Game.exe")
        );
        assert_eq!(
            launch_executable_for_registered_root_from_packages(
                root.path(),
                vec![RegisteredPackage {
                    root: root.path().to_path_buf(),
                    family_name: "package-family".to_owned(),
                }],
            )
            .as_deref(),
            Some("Game.exe")
        );
    }

    #[test]
    fn registered_root_without_a_store_id_keeps_xbox_identity_without_an_external_id() {
        let root = tempdir().expect("root");
        fs::write(
            root.path().join(MICROSOFT_GAME_CONFIG),
            r#"<Game><ExecutableList><Executable Name="Game.exe" /></ExecutableList></Game>"#,
        )
        .expect("config");
        write_pe(root.path(), "Game.exe");

        let metadata = registered_package_metadata_for_root_from_packages(
            root.path(),
            vec![RegisteredPackage {
                root: root.path().to_path_buf(),
                family_name: "package-family".to_owned(),
            }],
        )
        .expect("registered package metadata");
        let identity = metadata.identity();
        assert_eq!(identity.launcher, Launcher::Xbox);
        assert_eq!(identity.external_id, None);
    }

    #[test]
    fn family_dedup_is_stable_and_discovery_keeps_only_game_packages() {
        let root = tempdir().expect("root");
        let first = root.path().join("first");
        let duplicate = root.path().join("duplicate");
        let second = root.path().join("second");
        for (directory, store_id) in [(&first, "9MW53ZKZH168"), (&second, "9NK8947Z28XL")] {
            fs::create_dir_all(directory).expect("dir");
            fs::write(
                directory.join(MICROSOFT_GAME_CONFIG),
                config(store_id, r#"<Executable Name="Game.exe" />"#),
            )
            .expect("config");
            write_pe(directory, "Game.exe");
        }
        fs::create_dir_all(&duplicate).expect("duplicate dir");

        let packages = vec![
            RegisteredPackage {
                root: duplicate,
                family_name: "family-a".to_owned(),
            },
            RegisteredPackage {
                root: first.clone(),
                family_name: "family-a".to_owned(),
            },
            RegisteredPackage {
                root: second.clone(),
                family_name: "family-b".to_owned(),
            },
        ];
        let deduplicated = deduplicate_registered_packages(packages.clone());
        assert_eq!(deduplicated.len(), 2);
        assert_eq!(deduplicated[0].family_name, "family-a");
        assert_eq!(deduplicated[1].family_name, "family-b");
        let discovered = discovered_sources_from_registered_packages(packages);
        assert_eq!(
            discovered.game_installs.len(),
            2,
            "missing executable rejects package"
        );
        assert!(
            discovered
                .game_installs
                .iter()
                .all(|install| install.identity.launcher == Launcher::Xbox)
        );
        assert_eq!(
            discovered
                .game_installs
                .iter()
                .find(|install| install.identity.external_id.as_deref() == Some("9MW53ZKZH168"))
                .map(|install| install.identity.external_id.as_deref()),
            Some(Some("9MW53ZKZH168")),
            "a resource sibling cannot suppress its exact executable-bearing package root",
        );
        assert_eq!(
            metadata_for_package_root(&first)
                .expect("first metadata")
                .identity()
                .external_id
                .as_deref(),
            Some("9MW53ZKZH168")
        );
        assert_eq!(
            metadata_for_package_root(&second)
                .expect("second metadata")
                .identity()
                .external_id
                .as_deref(),
            Some("9NK8947Z28XL")
        );
    }
}
