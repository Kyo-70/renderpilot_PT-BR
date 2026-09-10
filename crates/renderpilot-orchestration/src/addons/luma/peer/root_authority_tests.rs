use std::path::Path;

use renderpilot_domain::{
    AddonKind, ComponentFile, ComponentId, ComponentKind, ComponentRollbackBaseline, GameId,
    InstalledAddon, LibraryComponent, LibraryTechnology, ManagedAddonFile, PathRef,
    PeerCatalogDeletedBaseline, PeerCatalogRollbackClaim, Sha256Hash, Swappability,
};

use crate::addons::reshade::scan::ReshadeContent;

use super::root_authority::LumaPeerRootAuthority;

fn path_ref(path: &Path) -> PathRef {
    PathRef::from_canonical_native_absolute(path).expect("path")
}

fn digest(value: &str) -> Sha256Hash {
    let nibble = value.as_bytes().first().copied().unwrap_or(b'a') % 16;
    Sha256Hash::new(format!("{nibble:x}").repeat(64)).expect("digest")
}

fn authority_with_payload(root: &Path, payload: &Path) -> LumaPeerRootAuthority {
    std::fs::write(
        root.join("ReShade.ini"),
        format!("[ADDON]\r\nAddonPath={}\r\n", payload.to_string_lossy()),
    )
    .expect("ini");
    LumaPeerRootAuthority::resolve(root, &root.join("dxgi.dll")).expect("authority")
}

fn luma_record(game_id: &GameId, addon_file: &Path) -> InstalledAddon {
    InstalledAddon::new(game_id.clone(), AddonKind::Luma, path_ref(addon_file))
}

fn catalog_claim(game_id: &GameId, path: &Path) -> PeerCatalogRollbackClaim {
    let id = ComponentId::new("component:root-authority").expect("component id");
    let component = LibraryComponent::new(
        id.clone(),
        game_id.clone(),
        ComponentKind::NativeLibrary,
        LibraryTechnology::AmdFsr,
        Swappability::Swappable,
    )
    .with_file(ComponentFile::new(path_ref(path)).with_sha256(digest("active")));
    PeerCatalogRollbackClaim::new(
        vec![component],
        vec![PeerCatalogDeletedBaseline::new(
            id,
            ComponentRollbackBaseline::new(vec![
                ComponentFile::new(path_ref(path)).with_sha256(digest("baseline")),
            ]),
        )],
    )
    .expect("catalog claim")
}

#[test]
fn seals_one_root_and_distinct_external_addon_root() {
    let game = tempfile::tempdir().expect("game");
    let one = LumaPeerRootAuthority::resolve(game.path(), &game.path().join("dxgi.dll"))
        .expect("one-root authority");
    assert_eq!(
        one.canonical_game_root(),
        crate::paths::canonicalize_existing(game.path())
            .expect("game")
            .as_path()
    );
    assert_eq!(
        one.canonical_game_root_ref(),
        &path_ref(one.canonical_game_root())
    );
    assert!(one.external_capability_root().is_none());
    assert!(one.external_capability_root_ref().is_none());
    assert_eq!(one.effective_addon_root(), one.canonical_game_root());
    assert_eq!(one.content(), ReshadeContent::Empty);

    let payload = tempfile::tempdir().expect("payload");
    let external = authority_with_payload(game.path(), payload.path());
    assert_eq!(
        external.external_capability_root(),
        Some(
            crate::paths::canonicalize_existing(payload.path())
                .expect("payload")
                .as_path()
        )
    );
    assert_eq!(
        external.external_capability_root_ref(),
        external.external_capability_root().map(path_ref).as_ref()
    );
    assert_eq!(
        external.effective_addon_root(),
        external
            .external_capability_root()
            .expect("external semantic add-on root")
    );
    assert_eq!(
        external.authorized_root(&path_ref(&game.path().join("created.dll"))),
        Ok(external.canonical_game_root_ref())
    );
    assert_eq!(
        external.authorized_root(&path_ref(&payload.path().join("created.dll"))),
        external
            .external_capability_root_ref()
            .ok_or_else(|| panic!("payload root"))
    );
}

#[test]
fn nested_addon_path_keeps_one_capability_root_and_its_own_semantic_root() {
    let game = tempfile::tempdir().expect("game");
    let nested = game.path().join("LumaAddons");
    std::fs::create_dir(&nested).expect("nested add-on directory");
    std::fs::write(
        game.path().join("ReShade.ini"),
        format!("[ADDON]\r\nAddonPath={}\r\n", nested.to_string_lossy()),
    )
    .expect("ini");

    let authority = LumaPeerRootAuthority::resolve(game.path(), &game.path().join("dxgi.dll"))
        .expect("nested authority");
    let canonical_nested = crate::paths::canonicalize_existing(&nested).expect("nested");

    assert!(authority.external_capability_root().is_none());
    assert_eq!(authority.effective_addon_root(), canonical_nested.as_path());
    assert_eq!(
        authority.effective_dlss_target().expect("DLSS target"),
        path_ref(&canonical_nested.join(renderpilot_detection::NVNGX_DLSS_FILE_NAME))
    );
    assert_eq!(
        authority
            .authorized_root(&path_ref(&canonical_nested.join("Luma.addon")))
            .expect("nested path is authorized by the runtime root"),
        authority.canonical_game_root_ref(),
    );
}

#[test]
fn rejects_addon_path_that_contains_the_runtime_root() {
    let parent = tempfile::tempdir().expect("parent");
    let game = parent.path().join("game");
    std::fs::create_dir(&game).expect("game");
    std::fs::write(
        game.join("ReShade.ini"),
        format!(
            "[ADDON]\r\nAddonPath={}\r\n",
            parent.path().to_string_lossy()
        ),
    )
    .expect("ini");

    let error = LumaPeerRootAuthority::resolve(&game, &game.join("dxgi.dll"))
        .expect_err("ancestor add-on root must be rejected");
    assert!(error.to_string().contains("must not be an ancestor"));
}

#[test]
fn authority_is_frozen_when_reshade_configuration_changes_or_disappears() {
    let game = tempfile::tempdir().expect("game");
    let first = tempfile::tempdir().expect("first payload");
    let second = tempfile::tempdir().expect("second payload");
    std::fs::write(first.path().join("foreign.addon64"), b"addon").expect("foreign addon");
    let authority = authority_with_payload(game.path(), first.path());
    assert_eq!(authority.content(), ReshadeContent::UserContent);

    std::fs::write(
        game.path().join("ReShade.ini"),
        format!(
            "[ADDON]\r\nAddonPath={}\r\n",
            second.path().to_string_lossy()
        ),
    )
    .expect("replacement ini");
    assert!(
        authority
            .authorized_root(&path_ref(&first.path().join("still-authorized.dll")))
            .is_ok()
    );
    assert!(
        authority
            .authorized_root(&path_ref(&second.path().join("drifted.dll")))
            .is_err()
    );
    std::fs::remove_file(game.path().join("ReShade.ini")).expect("delete ini");
    assert_eq!(authority.content(), ReshadeContent::UserContent);
    assert!(
        authority
            .authorized_root(&path_ref(&first.path().join("still-authorized.dll")))
            .is_ok()
    );
}

#[test]
fn captures_content_from_the_same_external_addon_path_snapshot() {
    let game = tempfile::tempdir().expect("game");
    let payload = tempfile::tempdir().expect("payload");
    std::fs::write(payload.path().join("foreign.addon64"), b"addon").expect("foreign addon");
    let authority = authority_with_payload(game.path(), payload.path());

    assert_eq!(authority.content(), ReshadeContent::UserContent);

    // The classification is frozen with the roots. Removing the observed file
    // cannot turn the already sealed operation into an empty-runtime adoption.
    std::fs::remove_file(payload.path().join("foreign.addon64")).expect("remove addon");
    assert_eq!(authority.content(), ReshadeContent::UserContent);
}

#[test]
fn rejects_external_or_root_host_before_config_resolution() {
    let game = tempfile::tempdir().expect("game");
    let outside = tempfile::tempdir().expect("outside");
    std::fs::write(game.path().join("ReShade.ini"), [0xff, 0xfe]).expect("invalid ini");
    let error = LumaPeerRootAuthority::resolve(game.path(), &outside.path().join("dxgi.dll"))
        .err()
        .map(|error| error.to_string())
        .expect("external host rejected");
    assert!(error.contains("strict descendant"));
    assert!(LumaPeerRootAuthority::resolve(game.path(), game.path()).is_err());
}

#[test]
fn accepts_valid_external_record_catalog_and_endpoint_constraints() {
    let game = tempfile::tempdir().expect("game");
    let payload = tempfile::tempdir().expect("payload");
    let authority = authority_with_payload(game.path(), payload.path());
    let game_id = GameId::new("manual:root-authority-valid").expect("game id");
    let addon = payload.path().join("Luma.addon");
    let managed = payload.path().join("nvngx_dlss.dll");
    let mut record = luma_record(&game_id, &addon)
        .with_created_file(path_ref(&game.path().join("dxgi.dll")))
        .with_backed_up_file(path_ref(&payload.path().join("ReShade.ini.bak")))
        .with_registered_exe_path(path_ref(&game.path().join("game.exe")));
    record = record
        .try_with_managed_files(vec![ManagedAddonFile::owned(
            path_ref(&managed),
            renderpilot_domain::ManagedFileBaseline::Absent,
            digest("installed"),
        )])
        .expect("managed path");
    let claim = catalog_claim(&game_id, &payload.path().join("amd_fsr.dll"));
    let endpoints = vec![
        path_ref(&game.path().join("dxgi.dll")),
        path_ref(&payload.path().join("amd_fsr.dll")),
    ];
    authority
        .validate(Some(&record), Some(&record), Some(&claim), &endpoints)
        .expect("valid authority constraints");
}

#[test]
fn rejects_kind_parent_third_root_lexical_escape_and_root_equality() {
    let game = tempfile::tempdir().expect("game");
    let payload = tempfile::tempdir().expect("payload");
    let third = tempfile::tempdir().expect("third");
    let authority = authority_with_payload(game.path(), payload.path());
    let game_id = GameId::new("manual:root-authority-invalid").expect("game id");
    let payload_addon = payload.path().join("Luma.addon");

    let foreign = InstalledAddon::new(game_id.clone(), AddonKind::RenoDx, path_ref(&payload_addon));
    assert!(authority.validate(Some(&foreign), None, None, &[]).is_err());

    let drifted = luma_record(&game_id, &game.path().join("Luma.addon"));
    assert!(authority.validate(Some(&drifted), None, None, &[]).is_err());

    let outside_record = luma_record(&game_id, &payload_addon)
        .with_created_file(path_ref(&third.path().join("owned.dll")));
    assert!(
        authority
            .validate(Some(&outside_record), None, None, &[])
            .is_err()
    );

    let outside_claim = catalog_claim(&game_id, &third.path().join("catalog.dll"));
    assert!(
        authority
            .validate(None, None, Some(&outside_claim), &[])
            .is_err()
    );
    let outside_endpoint = path_ref(&third.path().join("endpoint.dll"));
    assert!(
        authority
            .validate(None, None, None, &[outside_endpoint])
            .is_err()
    );

    let lexical_escape = PathRef::new(
        payload
            .path()
            .join("sub")
            .join("..")
            .join("escape.dll")
            .to_string_lossy()
            .into_owned(),
    )
    .expect("noncanonical test input");
    assert!(
        authority
            .validate(None, None, None, &[lexical_escape])
            .is_err()
    );
    assert!(
        authority
            .validate(None, None, None, &[path_ref(payload.path())])
            .is_err()
    );
}

#[test]
fn rejects_a_not_yet_created_external_addon_root() {
    let game = tempfile::tempdir().expect("game");
    let external_parent = tempfile::tempdir().expect("external parent");
    let missing = external_parent.path().join("missing-addon-root");
    std::fs::write(
        game.path().join("ReShade.ini"),
        format!("[ADDON]\r\nAddonPath={}\r\n", missing.to_string_lossy()),
    )
    .expect("ini");
    let error = LumaPeerRootAuthority::resolve(game.path(), &game.path().join("dxgi.dll"))
        .expect_err("external capability roots must be reachable before mutation");
    assert!(error.to_string().contains("not a reachable directory"));
}

#[test]
fn rejects_invalid_utf8_reshade_configuration() {
    let game = tempfile::tempdir().expect("game");
    std::fs::write(game.path().join("ReShade.ini"), [0xff, 0xfe]).expect("invalid ini");

    let error = LumaPeerRootAuthority::resolve(game.path(), &game.path().join("dxgi.dll"))
        .err()
        .map(|error| error.to_string())
        .expect("invalid configuration rejected");
    assert!(error.contains("not valid UTF-8"));
}

#[test]
fn rejects_directory_named_reshade_configuration() {
    let game = tempfile::tempdir().expect("game");
    std::fs::create_dir(game.path().join("ReShade.ini")).expect("directory config");

    let error = LumaPeerRootAuthority::resolve(game.path(), &game.path().join("dxgi.dll"))
        .err()
        .map(|error| error.to_string())
        .expect("directory configuration rejected");
    assert!(error.contains("not a regular file"));
}

#[cfg(unix)]
#[test]
fn rejects_symlink_named_reshade_configuration() {
    let game = tempfile::tempdir().expect("game");
    let target = game.path().join("actual.ini");
    std::fs::write(&target, b"[ADDON]\r\n").expect("target config");
    std::os::unix::fs::symlink(&target, game.path().join("ReShade.ini")).expect("symlink config");

    let error = LumaPeerRootAuthority::resolve(game.path(), &game.path().join("dxgi.dll"))
        .err()
        .map(|error| error.to_string())
        .expect("symlink configuration rejected");
    assert!(error.contains("symbolic link"));
}
