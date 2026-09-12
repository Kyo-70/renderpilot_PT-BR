use super::*;

use renderpilot_application::ComponentRepository;
use renderpilot_domain::{
    ComponentFile, ComponentId, ComponentKind, LibraryComponent, LibraryTechnology, Swappability,
};

#[test]
fn official_amd_fsr_entry_point_is_backed_up_replaced_preserved_and_restored() {
    let database = tempdir().expect("database");
    let root = tempdir().expect("game root");
    let context = Context::open_at(database.path().join("catalog.sqlite")).expect("context");
    let game_id = GameId::new("manual:official-amd-fsr-retention").expect("game id");
    let executable = root.path().join("Game.exe");
    let entry_point = root.path().join("amd_fidelityfx_dx12.dll");
    let original = b"official AMD FSR entry point".to_vec();
    let first_opti = b"OptiScaler FSR replacement initial".to_vec();
    let second_opti = b"OptiScaler FSR replacement upgraded".to_vec();
    std::fs::write(&executable, b"game").expect("executable");
    std::fs::write(&entry_point, &original).expect("official FSR entry point");
    let game = GameInstallation::new(
        GameIdentity::new(game_id.clone(), "AMD FSR retention", Launcher::Manual)
            .expect("identity"),
        Platform::Windows,
        GameRuntime::NativeWindows,
        PathRef::new(root.path().to_string_lossy()).expect("game root"),
    );
    context.storage().upsert_game(&game).expect("game");
    let original_digest = renderpilot_detection::sha256_file(&entry_point).expect("original hash");
    context
        .storage()
        .replace_components_for_game(
            &game_id,
            &[LibraryComponent::new(
                ComponentId::new("component:official-amd-fsr").expect("component id"),
                game_id.clone(),
                ComponentKind::NativeLibrary,
                LibraryTechnology::AmdFsr,
                Swappability::Swappable,
            )
            .with_file(
                ComponentFile::new(
                    PathRef::new(entry_point.to_string_lossy()).expect("component path"),
                )
                .with_sha256(original_digest.clone()),
            )],
        )
        .expect("component");

    let manifest = manifest_store::parse_manifest(include_bytes!(
        "../../../../../assets/optiscaler-fallback.json"
    ))
    .expect("manifest");
    let source = manifest.releases[0].source.clone();
    let release = |id: &str, bytes: &[u8]| OptiScalerRelease {
        id: id.to_owned(),
        source: source.clone(),
        archive_sha256: "a".repeat(64),
        archive_size: 1,
        config_schema: 1,
        members: vec![
            OptiScalerArchiveMember {
                archive_path: "proxy.dll".to_owned(),
                target: "$proxy".to_owned(),
                sha256: archive::sha256_hex(b"proxy"),
                size: 5,
                module: "core".to_owned(),
                pe_x64: false,
            },
            OptiScalerArchiveMember {
                archive_path: "OptiScaler.ini".to_owned(),
                target: "OptiScaler.ini".to_owned(),
                sha256: archive::sha256_hex(b"[OptiScaler]\n"),
                size: 13,
                module: "core".to_owned(),
                pe_x64: false,
            },
            OptiScalerArchiveMember {
                archive_path: "amd_fidelityfx_dx12.dll".to_owned(),
                target: "amd_fidelityfx_dx12.dll".to_owned(),
                sha256: archive::sha256_hex(bytes),
                size: u64::try_from(bytes.len()).expect("member size"),
                module: "core".to_owned(),
                pe_x64: false,
            },
        ],
    };
    let archive_for = |bytes: &[u8]| {
        archive::PreparedArchive::from_files(HashMap::from([
            ("proxy.dll".to_owned(), b"proxy".to_vec()),
            ("OptiScaler.ini".to_owned(), b"[OptiScaler]\n".to_vec()),
            ("amd_fidelityfx_dx12.dll".to_owned(), bytes.to_vec()),
        ]))
    };
    let permit = |context: &Context| {
        let authority = FileSafetyAuthority::new();
        let assessment = authority
            .issue_game_assessment(context, &game_id)
            .expect("assessment");
        authority
            .game_permit(game_id.clone(), Some(&assessment.context_token))
            .expect("permit")
    };
    let plan = |intent, old_state, release, archive| ApplyPlan {
        intent,
        context: &context,
        guard: crate::game_mutation_lock::try_lock(&game_id).expect("guard"),
        manifest: manifest.clone(),
        game_id: game_id.clone(),
        old_state,
        old_managed_files: Vec::new(),
        old_config_base: None,
        release,
        modules: HashSet::from(["core".to_owned()]),
        artifacts: ArtifactSet {
            archive,
            module_artifacts: Vec::new(),
            native_artifacts: Vec::new(),
        },
        target: ApplyTarget {
            exe: executable.clone(),
            dir: root.path().to_path_buf(),
            proxy: EvaluatedProxyPlan {
                slot: root.path().join("dxgi.dll"),
                chain_reshade: false,
                downstream_path: None,
                conflict: None,
                reshade_source_path: None,
                reshade_source_sha256: None,
            },
        },
        safety: permit(&context),
        compatibility_invariants: Vec::new(),
        accepted_prerequisite_binding: renderpilot_domain::OptiScalerPrerequisiteBinding::None,
    };

    apply_release(&plan(
        ApplyIntent::Install {
            modules: Some(vec!["core".to_owned()]),
        },
        None,
        release("initial-release", &first_opti),
        archive_for(&first_opti),
    ))
    .expect("install replaces official FSR entry point");
    let backup = root
        .path()
        .join(".amd_fidelityfx_dx12.dll.renderpilot-optiscaler-original");
    assert_eq!(
        std::fs::read(&entry_point).expect("active replacement"),
        first_opti
    );
    assert_eq!(std::fs::read(&backup).expect("original backup"), original);
    let installed = context
        .storage()
        .get_optiscaler_install_state(&game_id)
        .expect("state")
        .expect("installed state");
    let retained = installed
        .release_files
        .iter()
        .find(|file| crate::paths::same_path(Path::new(file.path.as_str()), &entry_point))
        .expect("retained entry point");
    assert!(matches!(
        retained.baseline,
        renderpilot_domain::OptiScalerReleaseFileBaseline::RetainedFsrEntryPoint { .. }
    ));

    // A restart/rescan sees the active OptiScaler payload at the target, not
    // the retained original in its immutable sidecar. Updating must rely on
    // that sidecar's exact receipt rather than requiring the original to
    // remain in the component catalogue.
    let active_digest = renderpilot_detection::sha256_file(&entry_point).expect("active hash");
    context
        .storage()
        .replace_components_for_game(
            &game_id,
            &[LibraryComponent::new(
                ComponentId::new("component:post-install-optiscaler").expect("component id"),
                game_id.clone(),
                ComponentKind::NativeLibrary,
                LibraryTechnology::AmdFsr,
                Swappability::Swappable,
            )
            .with_file(
                ComponentFile::new(
                    PathRef::new(entry_point.to_string_lossy()).expect("component path"),
                )
                .with_sha256(active_digest),
            )],
        )
        .expect("post-install component rescan");

    apply_release(&plan(
        ApplyIntent::Update,
        Some(installed),
        release("upgraded-release", &second_opti),
        archive_for(&second_opti),
    ))
    .expect("update preserves original backup");
    assert_eq!(
        std::fs::read(&entry_point).expect("updated replacement"),
        second_opti
    );
    assert_eq!(
        std::fs::read(&backup).expect("unchanged original backup"),
        original
    );

    std::fs::remove_file(&entry_point).expect("manually remove active replacement");
    let guard = crate::game_mutation_lock::try_lock(&game_id).expect("uninstall guard");
    uninstall_locked(&context, &game_id, &guard).expect("uninstall restores original FSR");
    assert_eq!(
        std::fs::read(&entry_point).expect("restored original"),
        original
    );
    assert!(!backup.exists(), "original backup is consumed on uninstall");
    drop(guard);

    // A release can stop shipping the FSR entry point. That is the same
    // restoration transition as uninstall, but it must happen inside update
    // before the successor state omits the retained baseline.
    context
        .storage()
        .replace_components_for_game(
            &game_id,
            &[LibraryComponent::new(
                ComponentId::new("component:restored-official-amd-fsr").expect("component id"),
                game_id.clone(),
                ComponentKind::NativeLibrary,
                LibraryTechnology::AmdFsr,
                Swappability::Swappable,
            )
            .with_file(
                ComponentFile::new(
                    PathRef::new(entry_point.to_string_lossy()).expect("component path"),
                )
                .with_sha256(original_digest),
            )],
        )
        .expect("restored original component rescan");
    apply_release(&plan(
        ApplyIntent::Install {
            modules: Some(vec!["core".to_owned()]),
        },
        None,
        release("reinstall-release", &first_opti),
        archive_for(&first_opti),
    ))
    .expect("second install");
    let second_installed = context
        .storage()
        .get_optiscaler_install_state(&game_id)
        .expect("second state")
        .expect("second installed state");
    let mut release_without_fsr = release("release-without-fsr", &second_opti);
    release_without_fsr
        .members
        .retain(|member| member.target != "amd_fidelityfx_dx12.dll");
    apply_release(&plan(
        ApplyIntent::Update,
        Some(second_installed),
        release_without_fsr,
        archive_for(&second_opti),
    ))
    .expect("release removal restores original FSR entry point");
    assert_eq!(
        std::fs::read(&entry_point).expect("restored release-removed original"),
        original
    );
    assert!(
        !backup.exists(),
        "release removal consumes the original backup"
    );
}
