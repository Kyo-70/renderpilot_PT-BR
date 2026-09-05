use super::*;

fn state() -> OptiScalerInstallState {
    let receipt = FileReceipt::owned("config-id", Sha256Hash::new("a".repeat(64)).expect("hash"))
        .expect("receipt");
    from_persisted(
        OptiScalerInstallStateParts {
            game_id: GameId::new("steam:1").expect("id"),
            release_id: "v0.9.4".to_owned(),
            manifest_revision: "catalog-1".to_owned(),
            archive_sha256: None,
            source: None,
            target_exe_path: PathRef::new("C:/Games/Test/Game.exe").expect("exe"),
            target_dir: PathRef::new("C:/Games/Test").expect("dir"),
            modules: vec!["core".to_owned()],
            release_files: vec![OptiScalerFileReceipt {
                path: PathRef::new("C:/Games/Test/OptiScaler.ini").expect("file"),
                installed: receipt,
                role: OptiScalerFileRole::Configuration,
                cleanup: OptiScalerFileCleanup::RemoveIfUnchanged,
                baseline: OptiScalerReleaseFileBaseline::Absent,
            }],
            runtime_bindings: Vec::new(),
            directory_receipts: Vec::new(),
            proxy_topology_id: Some("optiscaler:steam:1".to_owned()),
            config_schema: 1,
            config_base_release: "v0.9.4".to_owned(),
            adoption_state: OptiScalerAdoptionState::Managed,
            prerequisite_binding: OptiScalerPrerequisiteBinding::None,
            created_at: None,
            updated_at: None,
        },
        OptiScalerConfigurationBaseline::absent(),
    )
    .expect("state")
}

#[test]
fn validates_a_complete_state() {
    state().validate().expect("valid state");
}

#[test]
fn retained_fsr_original_is_typed_owned_runtime_custody() {
    let mut value = state();
    let original = FileReceipt::reused(
        "fsr-original-id",
        Sha256Hash::new("b".repeat(64)).expect("hash"),
    )
    .expect("original receipt");
    value.release_files.push(OptiScalerFileReceipt {
        path: PathRef::new("C:/Games/Test/amd_fidelityfx_dx12.dll").expect("entry point"),
        installed: FileReceipt::owned(
            "optiscaler-id",
            Sha256Hash::new("c".repeat(64)).expect("hash"),
        )
        .expect("installed receipt"),
        role: OptiScalerFileRole::Runtime,
        cleanup: OptiScalerFileCleanup::RemoveIfUnchanged,
        baseline: OptiScalerReleaseFileBaseline::RetainedFsrEntryPoint {
            component_id: ComponentId::new("component:amd-fsr").expect("component id"),
            custody_path: PathRef::new(
                "C:/Games/Test/.amd_fidelityfx_dx12.dll.renderpilot-optiscaler-original",
            )
            .expect("custody path"),
            original,
        },
    });
    value
        .release_files
        .sort_by(|left, right| left.path.as_str().cmp(right.path.as_str()));
    value.validate().expect("retained original state");

    let retained = value
        .release_files
        .iter()
        .find(|file| file.role == OptiScalerFileRole::Runtime)
        .expect("runtime receipt");
    assert!(retained.baseline.retained_original().is_some());

    let retained = value
        .release_files
        .iter_mut()
        .find(|file| file.role == OptiScalerFileRole::Runtime)
        .expect("runtime receipt");
    let OptiScalerReleaseFileBaseline::RetainedFsrEntryPoint { custody_path, .. } =
        &mut retained.baseline
    else {
        unreachable!("retained baseline")
    };
    *custody_path = PathRef::new("C:/Games/Test/amd_fidelityfx_dx12.dll.bak")
        .expect("invalid custody for test");
    assert!(matches!(
        value.validate(),
        Err(OptiScalerStateError::RetainedFsrBaselinePath)
    ));
}

#[test]
fn timestamp_agnostic_equality_ignores_only_persistence_timestamps() {
    let mut persisted = state();
    persisted.created_at = Some(10);
    persisted.updated_at = Some(20);
    let mut retimestamped = persisted.clone();
    retimestamped.created_at = Some(30);
    retimestamped.updated_at = Some(40);
    assert!(persisted.eq_ignoring_persistence_timestamps(&retimestamped));

    retimestamped.release_id = "v0.9.5".to_owned();
    assert!(!persisted.eq_ignoring_persistence_timestamps(&retimestamped));

    let mut rebinding = persisted.clone();
    rebinding.prerequisite_binding = OptiScalerPrerequisiteBinding::Luma;
    assert!(!persisted.eq_ignoring_persistence_timestamps(&rebinding));
}

#[test]
fn prerequisite_binding_has_one_strict_wire_and_storage_vocabulary() {
    assert_eq!(OptiScalerPrerequisiteBinding::None.as_str(), "none");
    assert_eq!(OptiScalerPrerequisiteBinding::Luma.as_str(), "luma");
    assert_eq!(
        serde_json::to_string(&OptiScalerPrerequisiteBinding::Luma).expect("serialize"),
        "\"luma\""
    );
    assert_eq!(
        serde_json::from_str::<OptiScalerPrerequisiteBinding>("\"none\"").expect("parse"),
        OptiScalerPrerequisiteBinding::None
    );
    assert!("Luma".parse::<OptiScalerPrerequisiteBinding>().is_err());
    assert!(serde_json::from_str::<OptiScalerPrerequisiteBinding>("\"other\"").is_err());
}

#[test]
fn parts_round_trip_preserves_prerequisite_binding() {
    let mut original = state();
    original.prerequisite_binding = OptiScalerPrerequisiteBinding::Luma;
    let parts = OptiScalerInstallStateParts::from(&original);
    let rebuilt =
        from_persisted(parts, original.configuration_baseline().clone()).expect("rebuild state");
    assert_eq!(
        rebuilt.prerequisite_binding,
        OptiScalerPrerequisiteBinding::Luma
    );
}

#[test]
fn rejects_unsorted_modules_and_escaped_paths() {
    let mut value = state();
    value.modules = vec!["z".to_owned(), "core".to_owned()];
    assert!(value.validate().is_err());
    value = state();
    value.release_files[0].path = PathRef::new("C:/Games/Other/OptiScaler.ini").unwrap();
    assert!(value.validate().is_err());
}

#[test]
fn rejects_duplicate_private_paths() {
    let mut value = state();
    value.runtime_bindings.push(OptiScalerModuleRuntimeBinding {
        module: "core".to_owned(),
        path: value.release_files[0].path.clone(),
        installed: FileReceipt::owned("runtime-id", Sha256Hash::new("b".repeat(64)).unwrap())
            .unwrap(),
        baseline: OptiScalerFileBaseline::Absent,
    });
    assert!(matches!(
        value.validate(),
        Err(OptiScalerStateError::DuplicatePath(_))
    ));
}

#[test]
fn requires_complete_release_identity() {
    let mut value = state();
    value.archive_sha256 = Some(Sha256Hash::new("a".repeat(64)).expect("hash"));
    assert!(matches!(
        value.validate(),
        Err(OptiScalerStateError::InvalidField(_))
    ));
}

#[test]
fn receipts_expose_ownership_identity_and_digest() {
    let mut value = state();
    let path = PathRef::new("C:/Games/Test/core.dll").expect("runtime path");
    let hash = Sha256Hash::new("b".repeat(64)).expect("hash");
    value.runtime_bindings.push(OptiScalerModuleRuntimeBinding {
        module: "core".to_owned(),
        path,
        installed: FileReceipt::reused("file-id-1", hash.clone()).expect("receipt"),
        baseline: OptiScalerFileBaseline::Present {
            receipt: FileReceipt::reused("file-id-1", hash).expect("baseline"),
        },
    });
    value.validate().expect("exact binding with exact baseline");

    let receipt = &value.runtime_bindings[0].installed;
    assert_eq!(receipt.identity(), "file-id-1");
    assert_eq!(receipt.ownership(), FileOwnership::Reused);
    assert!(!receipt.authorizes_destructive_cleanup());
    assert!(
        FileReceipt::owned("file-id-2", Sha256Hash::new("c".repeat(64)).unwrap())
            .expect("receipt")
            .authorizes_destructive_cleanup()
    );
}

#[test]
fn reused_runtime_binding_cannot_promote_an_owned_baseline() {
    let mut value = state();
    let hash = Sha256Hash::new("b".repeat(64)).expect("hash");
    value.runtime_bindings.push(OptiScalerModuleRuntimeBinding {
        module: "core".to_owned(),
        path: PathRef::new("C:/Games/Test/core.dll").expect("runtime path"),
        installed: FileReceipt::reused("runtime-id", hash.clone()).expect("installed"),
        baseline: OptiScalerFileBaseline::Present {
            receipt: FileReceipt::owned("runtime-id", hash).expect("baseline"),
        },
    });
    assert!(matches!(
        value.validate(),
        Err(OptiScalerStateError::ReusedRuntimeBindingOwnedBaseline(_))
    ));
}

#[test]
fn owned_runtime_binding_requires_an_absent_baseline() {
    let mut value = state();
    let hash = Sha256Hash::new("b".repeat(64)).expect("hash");
    value.runtime_bindings.push(OptiScalerModuleRuntimeBinding {
        module: "core".to_owned(),
        path: PathRef::new("C:/Games/Test/core.dll").expect("runtime path"),
        installed: FileReceipt::owned("runtime-id", hash.clone()).expect("installed"),
        baseline: OptiScalerFileBaseline::Present {
            receipt: FileReceipt::reused("runtime-id", hash).expect("baseline"),
        },
    });
    assert!(matches!(
        value.validate(),
        Err(OptiScalerStateError::OwnedRuntimeBindingHasPresentBaseline { module })
            if module == "core"
    ));
}

#[test]
fn runtime_binding_matrix_accepts_only_owned_absent_and_reused_exact_present() {
    let mut value = state();
    let hash = Sha256Hash::new("b".repeat(64)).expect("hash");

    value.runtime_bindings.push(OptiScalerModuleRuntimeBinding {
        module: "core".to_owned(),
        path: PathRef::new("C:/Games/Test/core.dll").expect("runtime path"),
        installed: FileReceipt::owned("owned-id", hash.clone()).expect("installed"),
        baseline: OptiScalerFileBaseline::Absent,
    });
    value.validate().expect("owned + absent is valid");

    value.runtime_bindings[0].installed =
        FileReceipt::reused("reused-id", hash.clone()).expect("installed");
    value.runtime_bindings[0].baseline = OptiScalerFileBaseline::Present {
        receipt: FileReceipt::reused("reused-id", hash).expect("baseline"),
    };
    value.validate().expect("reused + exact present is valid");
}

#[test]
fn receipt_wire_is_unversioned_and_rejects_unknown_fields() {
    let receipt = FileReceipt::owned("file-id-1", Sha256Hash::new("a".repeat(64)).expect("hash"))
        .expect("receipt");
    let json = serde_json::to_value(&receipt).expect("serialize");
    assert_eq!(json["identity"], "file-id-1");
    assert_eq!(json["ownership"], "owned");
    assert!(json.get("format").is_none());
    let decoded = serde_json::from_value::<FileReceipt>(json).expect("deserialize");
    assert_eq!(decoded.identity(), "file-id-1");
    assert_eq!(decoded.digest(), receipt.digest());
    assert_eq!(decoded.ownership(), FileOwnership::Owned);
    assert!(
        serde_json::from_value::<FileReceipt>(serde_json::json!({
            "identity": "file-id-1",
            "digest": "a".repeat(64),
            "ownership": "owned",
            "unknown": true
        }))
        .is_err()
    );
    for invalid in [
        serde_json::json!({
            "identity": "",
            "digest": "a".repeat(64),
            "ownership": "owned"
        }),
        serde_json::json!({
            "identity": "   ",
            "digest": "a".repeat(64),
            "ownership": "owned"
        }),
        serde_json::json!({
            "identity": "file-id-1",
            "digest": "not-a-sha256",
            "ownership": "owned"
        }),
        serde_json::json!({
            "identity": "file-id-1",
            "digest": "a".repeat(64),
            "ownership": "unknown"
        }),
    ] {
        assert!(
            serde_json::from_value::<FileReceipt>(invalid).is_err(),
            "invalid receipt must be rejected during deserialization"
        );
    }
    assert!(
        serde_json::from_value::<OptiScalerFileBaseline>(serde_json::json!({
            "kind": "present",
            "receipt": {
                "identity": "file-id-1",
                "digest": "a".repeat(64),
                "ownership": "owned"
            },
            "unknown": true
        }))
        .is_err()
    );
}
