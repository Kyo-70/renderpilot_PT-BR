//! RenoDX phase-one filesystem and configuration authority.

use std::path::Path;

use renderpilot_domain::PathRef;

use crate::ServiceError;
use crate::addons::peer_lifecycle::PeerRoots;
use crate::addons::reshade::{proxy::HostKind, scan};

use super::model::{RenoDxConfigSourceSeal, RenoDxRootSeal};

/// Immutable authority for one active RenoDX install operation.
#[derive(Debug, Clone)]
pub(crate) struct RenoDxRootAuthority {
    seal: RenoDxRootSeal,
    content: scan::ReshadeContent,
}

impl RenoDxRootAuthority {
    /// Resolves and seals the game root, ReShade configuration, payload root,
    /// exact direct ReShade host, and registered executable.
    ///
    /// `requested_addon_path` is an immutable command-plan overlay.  For proxy
    /// installs it supersedes the current `[ADDON] AddonPath` only for the
    /// sealed effective payload root; Vulkan installs deliberately remain at
    /// the game root.  The parsed configuration itself is never reread.
    pub(crate) fn resolve(
        game_dir: &Path,
        host_kind: HostKind,
        requested_addon_path: Option<&str>,
        registered_exe_path: Option<&Path>,
    ) -> Result<Self, ServiceError> {
        let canonical_game_root = crate::paths::canonical_candidate(game_dir).map_err(|error| {
            crate::failed(format!("RenoDX game root is not reachable: {error}"))
        })?;
        let expected_proxy_host = canonical_game_root.join("ReShade64.dll");
        let config = scan::resolve_strict_snapshot(
            game_dir,
            matches!(host_kind, HostKind::Proxy).then_some(expected_proxy_host.as_path()),
        )
        .map_err(|error| {
            crate::failed(format!("RenoDX ReShade configuration is invalid: {error}"))
        })?;
        let content = config.assess_content(game_dir, &[]);

        let exact_ini_path = config.retained_ini().map_or_else(
            || canonical_game_root.join(scan::RESHADE_INI_FILE_NAME),
            |ini| ini.path().to_path_buf(),
        );
        let config_source = match config.retained_ini() {
            Some(ini) => RenoDxConfigSourceSeal::File {
                exact_ini_path: ini.path().to_path_buf(),
                identity: ini.identity().to_owned(),
                digest: ini.digest().clone(),
                length: ini.length(),
                owned_bytes: ini.bytes().to_vec(),
                raw_addon_path_token: ini.raw_addon_path_token().map(str::to_owned),
            },
            None => RenoDxConfigSourceSeal::Absent {
                exact_ini_path: exact_ini_path.clone(),
            },
        };

        let effective_addon_root = if host_kind == HostKind::Vulkan {
            canonical_game_root.clone()
        } else {
            config.effective_addon_path_with_overlay(requested_addon_path)
        };
        let effective_addon_root = crate::paths::canonical_candidate(&effective_addon_root)
            .map_err(|error| {
                crate::failed(format!(
                    "RenoDX effective ReShade AddonPath is not reachable: {error}"
                ))
            })?;
        let payload_root = (!crate::paths::same_path(&effective_addon_root, &canonical_game_root))
            .then(|| effective_addon_root.clone());
        let payload_root_ref = payload_root
            .as_deref()
            .map(|root| path_ref(root, "payload root"))
            .transpose()?;
        let roots = PeerRoots::new(canonical_game_root.clone(), payload_root.clone())?;
        let canonical_game_root_ref = path_ref(&canonical_game_root, "game root")?;

        let exact_proxy_host = if host_kind == HostKind::Proxy {
            Some(expected_proxy_host)
        } else {
            None
        };
        let canonical_registered_exe = registered_exe_path
            .map(crate::paths::canonical_candidate)
            .transpose()
            .map_err(|error| {
                crate::failed(format!(
                    "RenoDX registered executable is not reachable: {error}"
                ))
            })?;
        if let Some(exe) = canonical_registered_exe.as_ref() {
            let exe_ref = path_ref(exe, "registered executable")?;
            roots.require_game_path(&exe_ref).map_err(|error| {
                crate::failed(format!(
                    "RenoDX registered executable must stay under the game root: {error}"
                ))
            })?;
        }

        let seal = RenoDxRootSeal {
            canonical_game_root,
            canonical_game_root_ref,
            config_source,
            effective_addon_root,
            payload_root,
            payload_root_ref,
            exact_ini_path,
            exact_proxy_host,
            canonical_registered_exe,
            roots,
        };
        Ok(Self { seal, content })
    }

    pub(crate) fn seal(&self) -> &RenoDxRootSeal {
        &self.seal
    }

    pub(crate) fn content(&self) -> scan::ReshadeContent {
        self.content
    }

    pub(crate) fn canonical_game_root(&self) -> &Path {
        self.seal.canonical_game_root()
    }

    pub(crate) fn canonical_game_root_ref(&self) -> &PathRef {
        self.seal.canonical_game_root_ref()
    }

    pub(crate) fn effective_addon_root(&self) -> &Path {
        self.seal.effective_addon_root()
    }

    #[cfg(test)]
    pub(crate) fn payload_root(&self) -> Option<&Path> {
        self.seal.payload_root()
    }

    pub(crate) fn exact_ini_path(&self) -> &Path {
        self.seal.exact_ini_path()
    }

    pub(crate) fn exact_proxy_host(&self) -> Option<&Path> {
        self.seal.exact_proxy_host()
    }

    pub(crate) fn roots(&self) -> &PeerRoots {
        self.seal.roots()
    }

    /// Converts one already-sealed filesystem path without a lossy fallback.
    pub(crate) fn path_ref(&self, path: &Path, label: &str) -> Result<PathRef, ServiceError> {
        let value = path
            .to_str()
            .ok_or_else(|| crate::failed(format!("RenoDX {label} is not valid UTF-8")))?;
        PathRef::new(value.to_owned())
            .map_err(|error| crate::failed(format!("RenoDX {label} is invalid: {error}")))
    }

    /// Ensures an endpoint is a strict descendant of one of the sealed roots.
    pub(crate) fn authorized_root(&self, path: &PathRef) -> Result<&PathRef, ServiceError> {
        if self.seal.roots.require_game_path(path).is_ok() {
            return Ok(self.seal.canonical_game_root_ref());
        }
        if self.seal.roots.require_sealed_path(path).is_ok() {
            let payload = self.seal.payload_root_ref().ok_or_else(|| {
                crate::failed(format!(
                    "RenoDX path is outside the sealed game root: {}",
                    path.as_str()
                ))
            })?;
            return Ok(payload);
        }
        Err(crate::failed(format!(
            "RenoDX path is outside the sealed roots: {}",
            path.as_str()
        )))
    }
}

fn path_ref(path: &Path, label: &str) -> Result<PathRef, ServiceError> {
    let value = path
        .to_str()
        .ok_or_else(|| crate::failed(format!("RenoDX {label} is not valid UTF-8")))?;
    PathRef::new(value.to_owned())
        .map_err(|error| crate::failed(format!("RenoDX {label} is invalid: {error}")))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn absent_config_seals_the_canonical_game_root() {
        let game = tempdir().expect("game directory");
        let authority = RenoDxRootAuthority::resolve(game.path(), HostKind::Vulkan, None, None)
            .expect("authority");

        assert!(!authority.seal().config_source().is_present());
        assert_eq!(authority.payload_root(), None);
        assert_eq!(authority.exact_proxy_host(), None);
        assert_eq!(
            authority.exact_ini_path(),
            &crate::paths::canonicalize_existing(game.path())
                .expect("canonical game")
                .join("ReShade.ini")
        );
        assert_eq!(
            authority.canonical_game_root(),
            crate::paths::canonicalize_existing(game.path()).expect("canonical game")
        );
    }

    #[test]
    fn retained_config_and_proxy_overlay_are_sealed_without_rereading() {
        let game = tempdir().expect("game directory");
        let external = tempdir().expect("external payload directory");
        let addon_root = game.path().join("configured-addons");
        let overlay_root = external.path().join("overlay-addons");
        fs::create_dir(&addon_root).expect("configured add-on root");
        fs::create_dir(&overlay_root).expect("overlay add-on root");
        let ini = b"[ADDON]\nAddonPath=configured-addons\n";
        fs::write(game.path().join("ReShade.ini"), ini).expect("config");

        let authority = RenoDxRootAuthority::resolve(
            game.path(),
            HostKind::Proxy,
            Some(overlay_root.to_str().expect("UTF-8 overlay path")),
            None,
        )
        .expect("authority");
        let source = authority.seal().config_source();
        let canonical_game =
            crate::paths::canonicalize_existing(game.path()).expect("canonical game");
        let canonical_overlay =
            crate::paths::canonicalize_existing(&overlay_root).expect("canonical overlay");
        let canonical_proxy_host = canonical_game.join("ReShade64.dll");

        assert!(source.is_present());
        assert_eq!(source.owned_bytes(), Some(ini.as_slice()));
        assert_eq!(source.raw_addon_path_token(), Some("configured-addons"));
        assert_eq!(authority.payload_root(), Some(canonical_overlay.as_path()));
        assert_eq!(
            authority.exact_proxy_host(),
            Some(canonical_proxy_host.as_path())
        );

        let nested = PathRef::new(
            canonical_overlay
                .join("nested/addon.addon64")
                .to_str()
                .expect("UTF-8 nested payload")
                .to_owned(),
        )
        .expect("nested payload path");
        assert_eq!(
            authority.authorized_root(&nested).expect("payload root"),
            authority
                .seal()
                .payload_root_ref()
                .expect("payload root ref")
        );
    }

    #[test]
    fn vulkan_ignores_configured_addon_path_and_keeps_no_proxy_host() {
        let game = tempdir().expect("game directory");
        let addon_root = game.path().join("configured-addons");
        fs::create_dir(&addon_root).expect("configured add-on root");
        fs::write(
            game.path().join("ReShade.ini"),
            b"[ADDON]\nAddonPath=configured-addons\n",
        )
        .expect("config");

        let authority = RenoDxRootAuthority::resolve(game.path(), HostKind::Vulkan, None, None)
            .expect("authority");

        assert_eq!(
            authority.effective_addon_root(),
            authority.canonical_game_root()
        );
        assert_eq!(authority.payload_root(), None);
        assert_eq!(authority.exact_proxy_host(), None);
    }

    #[test]
    fn registered_executable_must_be_a_strict_game_descendant() {
        let game = tempdir().expect("game directory");
        let outside = tempdir().expect("outside directory");
        let executable = outside.path().join("game.exe");
        fs::write(&executable, b"not an executable").expect("executable");

        let error =
            RenoDxRootAuthority::resolve(game.path(), HostKind::Vulkan, None, Some(&executable))
                .expect_err("outside executable must be rejected");
        assert!(error.to_string().contains("registered executable"));
    }

    #[cfg(unix)]
    #[test]
    fn path_ref_rejects_non_utf8_paths() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        let game = tempdir().expect("game directory");
        let invalid_path = OsString::from_vec(vec![b'g', b'a', 0x80]);
        let invalid = Path::new(&invalid_path);
        let authority = RenoDxRootAuthority::resolve(game.path(), HostKind::Vulkan, None, None)
            .expect("authority");

        assert!(authority.path_ref(invalid, "test path").is_err());
    }
}
