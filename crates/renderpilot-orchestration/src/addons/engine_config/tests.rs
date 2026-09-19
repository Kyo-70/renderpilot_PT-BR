use super::*;
use std::path::Path;
use tempfile::tempdir;

fn recipe(entries: Vec<EngineIniEntry>) -> EngineIniRecipe {
    EngineIniRecipe::new("test", 1, entries).expect("recipe")
}
fn set(recipe: &EngineIniRecipe) -> EngineIniRecipeSet {
    EngineIniRecipeSet::from_recipes([recipe]).expect("set")
}

fn complete_release(bytes: &[u8], receipt: &EngineIniReceipt) -> Option<Vec<u8>> {
    let result = release_engine_ini_report(bytes, receipt).expect("release");
    result.complete.then_some(result.bytes)
}

#[test]
fn applies_only_missing_keys_in_target_section() {
    let recipe = recipe(vec![EngineIniEntry {
        section: "SystemSettings".into(),
        key: "r.AllowHDR".into(),
        value: "1".into(),
    }]);
    let result = apply_engine_ini(
        Path::new("Engine.ini"),
        Some(b"; keep\r\n[SystemSettings]\r\nr.Other=2\r\n"),
        &set(&recipe),
    )
    .expect("apply");
    assert_eq!(
        String::from_utf8(result.bytes).expect("utf8"),
        "; keep\r\n[SystemSettings]\r\nr.Other=2\r\nr.AllowHDR=1\r\n"
    );
    assert!(result.receipt.is_some());
}

#[test]
fn exact_existing_value_is_unowned_and_idempotent() {
    let recipe = recipe(vec![EngineIniEntry {
        section: "SystemSettings".into(),
        key: "r.AllowHDR".into(),
        value: "1".into(),
    }]);
    let before = b"[systemsettings]\nr.allowhdr=1\n";
    let result =
        apply_engine_ini(Path::new("Engine.ini"), Some(before), &set(&recipe)).expect("apply");
    assert_eq!(result.bytes, before);
    assert!(result.receipt.is_none());
}

#[test]
fn conflicting_recipe_is_rejected_before_io() {
    let first = recipe(vec![EngineIniEntry {
        section: "SystemSettings".into(),
        key: "r.X".into(),
        value: "1".into(),
    }]);
    let second = EngineIniRecipe::new(
        "other",
        1,
        vec![EngineIniEntry {
            section: "SYSTEMSETTINGS".into(),
            key: "R.X".into(),
            value: "2".into(),
        }],
    )
    .expect("recipe");
    assert!(matches!(
        EngineIniRecipeSet::from_recipes([&first, &second]),
        Err(EngineIniError::RecipeConflict { .. })
    ));
}

#[test]
fn release_requires_unique_exact_contribution() {
    let recipe = recipe(vec![EngineIniEntry {
        section: "SystemSettings".into(),
        key: "r.X".into(),
        value: "1".into(),
    }]);
    let applied = apply_engine_ini(
        Path::new("Engine.ini"),
        Some(b"[SystemSettings]\n"),
        &set(&recipe),
    )
    .expect("apply");
    let receipt = applied.receipt.expect("receipt");
    assert_eq!(
        complete_release(&applied.bytes, &receipt),
        Some(b"[SystemSettings]\n".to_vec())
    );
    let mut foreign = applied.bytes;
    foreign.extend_from_slice(b"r.X=1\n");
    assert_eq!(complete_release(&foreign, &receipt), None);
}

#[test]
fn release_restores_multiple_insertions_and_introduced_prefix() {
    let recipe = recipe(vec![
        EngineIniEntry {
            section: "SystemSettings".into(),
            key: "r.AllowHDR".into(),
            value: "1".into(),
        },
        EngineIniEntry {
            section: "SystemSettings".into(),
            key: "r.HDR.EnableHDROutput".into(),
            value: "1".into(),
        },
    ]);
    let before = b"[SystemSettings]";
    let applied =
        apply_engine_ini(Path::new("Engine.ini"), Some(before), &set(&recipe)).expect("apply");
    assert_eq!(
        String::from_utf8(applied.bytes.clone()).expect("utf8"),
        "[SystemSettings]\nr.AllowHDR=1\nr.HDR.EnableHDROutput=1\n"
    );
    let receipt = applied.receipt.expect("receipt");
    assert!(
        receipt
            .contributions
            .iter()
            .any(|entry| !entry.introduced_prefix.is_empty())
    );
    assert_eq!(
        complete_release(&applied.bytes, &receipt),
        Some(before.to_vec())
    );
}

#[test]
fn release_removes_created_sections_and_their_prefix() {
    let recipe = recipe(vec![EngineIniEntry {
        section: "SystemSettings".into(),
        key: "r.AllowHDR".into(),
        value: "1".into(),
    }]);
    let before = b"; foreign";
    let applied =
        apply_engine_ini(Path::new("Engine.ini"), Some(before), &set(&recipe)).expect("apply");
    let receipt = applied.receipt.expect("receipt");
    assert!(!receipt.created_header_prefixes[0].is_empty());
    assert_eq!(
        complete_release(&applied.bytes, &receipt),
        Some(before.to_vec())
    );
}

#[test]
fn release_restores_multiple_created_sections() {
    let recipe = recipe(vec![
        EngineIniEntry {
            section: "SystemSettings".into(),
            key: "r.AllowHDR".into(),
            value: "1".into(),
        },
        EngineIniEntry {
            section: "/Script/Engine.RendererSettings".into(),
            key: "r.LUT.UpdateEveryFrame".into(),
            value: "1".into(),
        },
    ]);
    let before = b"; foreign";
    let applied =
        apply_engine_ini(Path::new("Engine.ini"), Some(before), &set(&recipe)).expect("apply");
    let receipt = applied.receipt.expect("receipt");
    assert_eq!(receipt.created_headers.len(), 2);
    assert_eq!(
        complete_release(&applied.bytes, &receipt),
        Some(before.to_vec())
    );
}

#[test]
fn resolver_is_bounded_and_never_creates_parent() {
    let temp = tempdir().expect("temp");
    let app = temp.path().join("Local");
    let config = app.join("Demo/Saved/Config/Windows");
    std::fs::create_dir_all(&config).expect("config");
    assert!(matches!(
        resolve_unreal_engine_ini(None, Some("Demo"), Some(&app)),
        EngineIniResolution::ReadyToCreate(_)
    ));
    assert!(!temp.path().join("Demo").exists());
}

#[test]
fn resolver_uses_proven_project_config_without_local_app_data() {
    let temp = tempdir().expect("temp");
    let project = temp.path().join("Demo");
    let config = project.join("Saved/Config/Windows");
    std::fs::create_dir_all(&config).expect("config");
    assert!(matches!(
        resolve_unreal_engine_ini(Some(&project), Some("Demo"), None),
        EngineIniResolution::ReadyToCreate(path) if path == config.join("Engine.ini")
    ));
}

#[test]
fn preserves_supported_bom_encodings_and_newline_style() {
    let recipe = recipe(vec![EngineIniEntry {
        section: "SystemSettings".into(),
        key: "r.AllowHDR".into(),
        value: "1".into(),
    }]);
    let utf8_before = b"\xEF\xBB\xBF[SystemSettings]\r\nr.Existing=1\r\n".to_vec();
    let applied = apply_engine_ini(Path::new("Engine.ini"), Some(&utf8_before), &set(&recipe))
        .expect("utf8 bom");
    assert!(applied.bytes.starts_with(b"\xEF\xBB\xBF"));
    assert!(applied.bytes.windows(2).any(|window| window == b"\r\n"));

    let mut utf16_before = vec![0xFF, 0xFE];
    utf16_before.extend_from_slice(&encode_text("[SystemSettings]\n", IniEncoding::Utf16Le));
    let applied_utf16 =
        apply_engine_ini(Path::new("Engine.ini"), Some(&utf16_before), &set(&recipe))
            .expect("utf16");
    assert!(applied_utf16.bytes.starts_with(&[0xFF, 0xFE]));
    assert!(applied_utf16.receipt.is_some());

    let mut utf16_be_before = vec![0xFE, 0xFF];
    utf16_be_before.extend_from_slice(&encode_text("[SystemSettings]\r\n", IniEncoding::Utf16Be));
    let applied_utf16_be = apply_engine_ini(
        Path::new("Engine.ini"),
        Some(&utf16_be_before),
        &set(&recipe),
    )
    .expect("utf16 be");
    assert!(applied_utf16_be.bytes.starts_with(&[0xFE, 0xFF]));
}

#[test]
fn rejects_unsupported_or_malformed_encodings() {
    let recipe = recipe(vec![EngineIniEntry {
        section: "SystemSettings".into(),
        key: "r.AllowHDR".into(),
        value: "1".into(),
    }]);
    for bytes in [
        vec![0xFF, 0xFE, 0, 0],
        vec![0xFF, 0xFE, 0x5B],
        vec![0x00, b'[', 0x00, b']'],
        vec![0xFF, 0xFE, 0x00, 0xD8],
    ] {
        assert!(matches!(
            apply_engine_ini(Path::new("Engine.ini"), Some(&bytes), &set(&recipe)),
            Err(EngineIniError::InvalidEncoding(_))
        ));
    }
}

#[test]
fn foreign_change_releases_only_unique_assignment_body() {
    let recipe = recipe(vec![EngineIniEntry {
        section: "SystemSettings".into(),
        key: "r.AllowHDR".into(),
        value: "1".into(),
    }]);
    let applied = apply_engine_ini(
        Path::new("Engine.ini"),
        Some(b"[SystemSettings]\nr.Existing=1\n"),
        &set(&recipe),
    )
    .expect("apply");
    let mut foreign = applied.bytes.clone();
    foreign.extend_from_slice(b"; user edit\n");
    assert_eq!(
        release_engine_ini_report(&foreign, &applied.receipt.expect("receipt")).expect("release"),
        EngineIniRelease {
            bytes: b"[SystemSettings]\nr.Existing=1\n; user edit\n".to_vec(),
            complete: false,
        }
    );
}

#[test]
fn missing_final_newline_is_preserved_as_foreign_structure() {
    let recipe = recipe(vec![EngineIniEntry {
        section: "SystemSettings".into(),
        key: "r.AllowHDR".into(),
        value: "1".into(),
    }]);
    let before = b"[SystemSettings]";
    let applied =
        apply_engine_ini(Path::new("Engine.ini"), Some(before), &set(&recipe)).expect("apply");
    assert_eq!(
        String::from_utf8(applied.bytes.clone()).expect("utf8"),
        "[SystemSettings]\nr.AllowHDR=1\n"
    );
    let receipt = applied.receipt.expect("receipt");
    assert_eq!(
        complete_release(&applied.bytes, &receipt),
        Some(before.to_vec())
    );
}

#[test]
fn revising_first_assignment_keeps_separator_and_final_release_restores_no_newline() {
    let initial = recipe(vec![
        EngineIniEntry {
            section: "SystemSettings".into(),
            key: "r.First".into(),
            value: "1".into(),
        },
        EngineIniEntry {
            section: "SystemSettings".into(),
            key: "r.Second".into(),
            value: "2".into(),
        },
    ]);
    let before = b"[SystemSettings]\nr.Existing=1";
    let applied =
        apply_engine_ini(Path::new("Engine.ini"), Some(before), &set(&initial)).expect("apply");
    let revised = recipe(vec![EngineIniEntry {
        section: "SystemSettings".into(),
        key: "r.Second".into(),
        value: "2".into(),
    }]);
    let edit = reconcile_engine_ini(
        Path::new("Engine.ini"),
        Some(&applied.bytes),
        &applied.receipt.expect("receipt"),
        &set(&revised),
    )
    .expect("reconcile");
    assert_eq!(edit.bytes, b"[SystemSettings]\nr.Existing=1\nr.Second=2\n");
    let receipt = edit.receipt.expect("receipt");
    assert_eq!(
        complete_release(&edit.bytes, &receipt),
        Some(before.to_vec())
    );
}
