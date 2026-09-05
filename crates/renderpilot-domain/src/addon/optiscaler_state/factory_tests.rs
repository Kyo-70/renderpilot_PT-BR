use std::fmt::Write;

use sha2::{Digest, Sha256};

use super::{
    FileOwnership, FileReceipt, GameId, OptiScalerAdoptionState, OptiScalerConfigurationBaseline,
    OptiScalerFileCleanup, OptiScalerFileReceipt, OptiScalerFileRole, OptiScalerInstallState,
    OptiScalerInstallStateParts, OptiScalerPrerequisiteBinding, OptiScalerStateError, PathRef,
    Sha256Hash, from_new_adoption, from_new_install, from_persisted,
};

fn digest(bytes: &[u8]) -> Sha256Hash {
    let mut hex = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        let _ = write!(hex, "{byte:02x}");
    }
    Sha256Hash::new(hex).expect("valid digest")
}

fn baseline(identity: &str, bytes: &[u8]) -> OptiScalerConfigurationBaseline {
    OptiScalerConfigurationBaseline::present(
        FileReceipt::reused(identity, digest(bytes)).expect("receipt"),
        bytes.to_vec(),
    )
    .expect("baseline")
}

fn parts(installed: FileReceipt, cleanup: OptiScalerFileCleanup) -> OptiScalerInstallStateParts {
    OptiScalerInstallStateParts {
        game_id: GameId::new("steam:factory-tests").expect("game id"),
        release_id: "v0.9.4".to_owned(),
        manifest_revision: "catalog-1".to_owned(),
        archive_sha256: None,
        source: None,
        target_exe_path: PathRef::new("C:/Games/Factory/Game.exe").expect("exe"),
        target_dir: PathRef::new("C:/Games/Factory").expect("target"),
        modules: vec!["core".to_owned()],
        release_files: vec![OptiScalerFileReceipt {
            path: PathRef::new("C:/Games/Factory/OptiScaler.ini").expect("config"),
            installed,
            role: OptiScalerFileRole::Configuration,
            cleanup,
            baseline: super::OptiScalerReleaseFileBaseline::Absent,
        }],
        runtime_bindings: Vec::new(),
        directory_receipts: Vec::new(),
        proxy_topology_id: Some("optiscaler:factory-tests".to_owned()),
        config_schema: 1,
        config_base_release: "v0.9.4".to_owned(),
        adoption_state: OptiScalerAdoptionState::Managed,
        prerequisite_binding: OptiScalerPrerequisiteBinding::None,
        created_at: None,
        updated_at: None,
    }
}

fn owned_configuration(identity: &str, bytes: &[u8]) -> FileReceipt {
    FileReceipt::owned(identity, digest(bytes)).expect("owned receipt")
}

fn reused_configuration(identity: &str, bytes: &[u8]) -> FileReceipt {
    FileReceipt::reused(identity, digest(bytes)).expect("reused receipt")
}

fn managed_state() -> OptiScalerInstallState {
    from_new_install(
        parts(
            owned_configuration("config-id", b"new config"),
            OptiScalerFileCleanup::RemoveIfUnchanged,
        ),
        OptiScalerConfigurationBaseline::absent(),
    )
    .expect("managed state")
}

#[test]
fn new_install_accepts_all_three_canonical_configuration_tuples() {
    let old = b"old config";
    let new = b"new config";

    from_new_install(
        parts(
            owned_configuration("created-config", new),
            OptiScalerFileCleanup::RemoveIfUnchanged,
        ),
        OptiScalerConfigurationBaseline::absent(),
    )
    .expect("absent + owned + remove");

    from_new_install(
        parts(
            owned_configuration("config-id", new),
            OptiScalerFileCleanup::PreserveCurrentThenRestoreBaseline,
        ),
        baseline("config-id", old),
    )
    .expect("present + owned + restore");

    from_new_install(
        parts(
            reused_configuration("config-id", old),
            OptiScalerFileCleanup::PreserveUnchanged,
        ),
        baseline("config-id", old),
    )
    .expect("present + reused + untouched");
}

#[test]
fn new_adoption_accepts_only_present_reused_exact_unchanged_configuration() {
    let bytes = b"existing config";
    from_new_adoption(
        parts(
            reused_configuration("config-id", bytes),
            OptiScalerFileCleanup::PreserveUnchanged,
        ),
        baseline("config-id", bytes),
    )
    .expect("canonical adoption");

    assert!(
        from_new_adoption(
            parts(
                owned_configuration("created-config", bytes),
                OptiScalerFileCleanup::RemoveIfUnchanged,
            ),
            OptiScalerConfigurationBaseline::absent(),
        )
        .is_err()
    );
    assert!(
        from_new_adoption(
            parts(
                owned_configuration("config-id", b"replacement"),
                OptiScalerFileCleanup::PreserveCurrentThenRestoreBaseline,
            ),
            baseline("config-id", bytes),
        )
        .is_err()
    );
}

#[test]
fn reused_configuration_acquisition_rebases_only_at_the_first_write() {
    let adopted = from_new_adoption(
        parts(
            reused_configuration("config-id", b"adopted"),
            OptiScalerFileCleanup::PreserveUnchanged,
        ),
        baseline("config-id", b"adopted"),
    )
    .expect("adopted state");
    let mut next = OptiScalerInstallStateParts::from(&adopted);
    next.release_files[0].installed = owned_configuration("config-id", b"managed");
    next.release_files[0].cleanup = OptiScalerFileCleanup::PreserveCurrentThenRestoreBaseline;
    let acquired = OptiScalerInstallState::from_existing_with_configuration_acquisition(
        &adopted,
        next,
        baseline("config-id", b"user edit before first write"),
    )
    .expect("exact acquisition");
    assert_eq!(
        acquired.configuration_baseline().bytes(),
        Some(b"user edit before first write".as_slice())
    );

    let mut recreated_live = OptiScalerInstallStateParts::from(&adopted);
    recreated_live.release_files[0].installed = owned_configuration("other-id", b"managed");
    recreated_live.release_files[0].cleanup =
        OptiScalerFileCleanup::PreserveCurrentThenRestoreBaseline;
    let recreated = OptiScalerInstallState::from_existing_with_configuration_acquisition(
        &adopted,
        recreated_live,
        baseline("config-id", b"user edit before first write"),
    )
    .expect("first managed write may recreate a missing configuration");
    assert_eq!(recreated.release_files[0].installed.identity(), "other-id");
}

#[test]
fn reused_configuration_retarget_leaves_the_old_user_file_outside_successor_state() {
    let previous = from_new_adoption(
        parts(
            reused_configuration("old-config-id", b"adopted"),
            OptiScalerFileCleanup::PreserveUnchanged,
        ),
        baseline("old-config-id", b"adopted"),
    )
    .expect("adopted state");
    let mut next = OptiScalerInstallStateParts::from(&previous);
    next.target_dir = PathRef::new("C:/Games/Factory/NewTarget").expect("new target");
    next.target_exe_path = PathRef::new("C:/Games/Factory/NewTarget/Game.exe").expect("new exe");
    next.release_files[0].path =
        PathRef::new("C:/Games/Factory/NewTarget/OptiScaler.ini").expect("new config path");
    next.release_files[0].installed = owned_configuration("new-config-id", b"managed");
    next.release_files[0].cleanup = OptiScalerFileCleanup::RemoveIfUnchanged;

    let retargeted =
        OptiScalerInstallState::from_existing_with_configuration_retarget(&previous, next)
            .expect("retargeted successor");

    assert_eq!(
        retargeted.configuration_baseline(),
        &OptiScalerConfigurationBaseline::Absent
    );
    assert_ne!(
        retargeted
            .configuration_receipt()
            .expect("new configuration")
            .path,
        previous
            .configuration_receipt()
            .expect("old configuration")
            .path
    );
    assert_eq!(
        retargeted
            .configuration_receipt()
            .expect("new configuration")
            .cleanup,
        OptiScalerFileCleanup::RemoveIfUnchanged
    );
}

#[test]
fn every_existing_successor_rejects_prerequisite_binding_drift() {
    let managed = managed_state();
    let mut ordinary = OptiScalerInstallStateParts::from(&managed);
    ordinary.prerequisite_binding = OptiScalerPrerequisiteBinding::Luma;
    assert!(matches!(
        OptiScalerInstallState::from_existing(&managed, ordinary),
        Err(OptiScalerStateError::PrerequisiteBindingTransition)
    ));

    let adopted = from_new_adoption(
        parts(
            reused_configuration("config-id", b"adopted"),
            OptiScalerFileCleanup::PreserveUnchanged,
        ),
        baseline("config-id", b"adopted"),
    )
    .expect("adopted state");

    let mut acquisition = OptiScalerInstallStateParts::from(&adopted);
    acquisition.prerequisite_binding = OptiScalerPrerequisiteBinding::Luma;
    acquisition.release_files[0].installed = owned_configuration("config-id", b"managed");
    acquisition.release_files[0].cleanup =
        OptiScalerFileCleanup::PreserveCurrentThenRestoreBaseline;
    assert!(matches!(
        OptiScalerInstallState::from_existing_with_configuration_acquisition(
            &adopted,
            acquisition,
            baseline("config-id", b"user edit"),
        ),
        Err(OptiScalerStateError::PrerequisiteBindingTransition)
    ));

    let mut retarget = OptiScalerInstallStateParts::from(&adopted);
    retarget.prerequisite_binding = OptiScalerPrerequisiteBinding::Luma;
    retarget.target_dir = PathRef::new("C:/Games/Factory/NewTarget").expect("target");
    retarget.target_exe_path = PathRef::new("C:/Games/Factory/NewTarget/Game.exe").expect("exe");
    retarget.release_files[0].path =
        PathRef::new("C:/Games/Factory/NewTarget/OptiScaler.ini").expect("configuration");
    retarget.release_files[0].installed = owned_configuration("new-config-id", b"managed");
    retarget.release_files[0].cleanup = OptiScalerFileCleanup::RemoveIfUnchanged;
    assert!(matches!(
        OptiScalerInstallState::from_existing_with_configuration_retarget(&adopted, retarget),
        Err(OptiScalerStateError::PrerequisiteBindingTransition)
    ));
}

#[test]
fn configuration_matrix_rejects_noncanonical_tuples() {
    let old = b"old config";
    let new = b"new config";

    assert!(
        from_new_install(
            parts(
                reused_configuration("config-id", old),
                OptiScalerFileCleanup::PreserveUnchanged,
            ),
            OptiScalerConfigurationBaseline::absent(),
        )
        .is_err()
    );
    assert!(
        from_new_install(
            parts(
                owned_configuration("config-id", new),
                OptiScalerFileCleanup::PreserveCurrentThenRestoreBaseline,
            ),
            OptiScalerConfigurationBaseline::absent(),
        )
        .is_err()
    );
    assert!(
        from_new_install(
            parts(
                owned_configuration("config-id", new),
                OptiScalerFileCleanup::RemoveIfUnchanged,
            ),
            baseline("config-id", old),
        )
        .is_err()
    );
    assert!(
        from_new_install(
            parts(
                owned_configuration("different-id", new),
                OptiScalerFileCleanup::PreserveCurrentThenRestoreBaseline,
            ),
            baseline("config-id", old),
        )
        .is_err()
    );
    assert!(
        from_new_install(
            parts(
                reused_configuration("config-id", new),
                OptiScalerFileCleanup::PreserveUnchanged,
            ),
            baseline("config-id", old),
        )
        .is_err()
    );
    assert!(
        from_new_install(
            parts(
                reused_configuration("config-id", old),
                OptiScalerFileCleanup::PreserveCurrentThenRestoreBaseline,
            ),
            baseline("config-id", old),
        )
        .is_err()
    );
}

#[test]
fn existing_factory_copies_immutable_baseline_and_allows_recreated_live_identity() {
    let old = b"old config";
    let previous = from_new_install(
        parts(
            owned_configuration("config-id", b"installed config"),
            OptiScalerFileCleanup::PreserveCurrentThenRestoreBaseline,
        ),
        baseline("config-id", old),
    )
    .expect("previous replacement");
    let mut next_parts = OptiScalerInstallStateParts::from(&previous);
    next_parts.release_id = "v0.9.5".to_owned();
    let next = OptiScalerInstallState::from_existing(&previous, next_parts).expect("successor");
    assert_eq!(
        next.configuration_baseline(),
        previous.configuration_baseline()
    );
    assert_eq!(
        next.release_files[0].cleanup,
        OptiScalerFileCleanup::PreserveCurrentThenRestoreBaseline
    );

    let mut drifted = OptiScalerInstallStateParts::from(&previous);
    drifted.release_files[0].cleanup = OptiScalerFileCleanup::RemoveIfUnchanged;
    assert!(matches!(
        OptiScalerInstallState::from_existing(&previous, drifted),
        Err(OptiScalerStateError::ConfigurationBaselineTransition(_))
    ));

    let mut identity_drift = OptiScalerInstallStateParts::from(&previous);
    identity_drift.release_files[0].installed = owned_configuration("different-id", b"installed");
    let recreated = OptiScalerInstallState::from_existing(&previous, identity_drift)
        .expect("a missing configuration may be recreated as a new live object");
    assert_eq!(
        recreated.configuration_baseline(),
        previous.configuration_baseline()
    );
    assert_eq!(
        recreated.release_files[0].installed.identity(),
        "different-id"
    );
}

#[test]
fn existing_factory_repairs_structural_legacy_invalid_state_only_through_candidate() {
    let old = b"old config";
    let legacy = from_persisted(
        parts(
            owned_configuration("config-id", b"replacement"),
            OptiScalerFileCleanup::RemoveIfUnchanged,
        ),
        baseline("config-id", old),
    )
    .expect("structurally valid legacy-invalid state");

    let mut candidate = OptiScalerInstallStateParts::from(&legacy);
    candidate.release_id = "v0.9.5".to_owned();
    candidate.release_files[0].cleanup = OptiScalerFileCleanup::PreserveCurrentThenRestoreBaseline;
    let repaired = OptiScalerInstallState::from_existing(&legacy, candidate)
        .expect("candidate is canonicalized explicitly");
    assert_eq!(
        repaired.configuration_baseline(),
        legacy.configuration_baseline()
    );
    assert_eq!(
        repaired.release_files[0].cleanup,
        OptiScalerFileCleanup::PreserveCurrentThenRestoreBaseline
    );

    let unchanged = OptiScalerInstallStateParts::from(&legacy);
    assert!(OptiScalerInstallState::from_existing(&legacy, unchanged).is_err());
}

#[test]
fn baseline_rejects_length_digest_and_oversize_mismatches() {
    let bytes = b"[OptiScaler]\n";
    let receipt = FileReceipt::reused("config-id", digest(bytes)).expect("receipt");
    assert!(matches!(
        OptiScalerConfigurationBaseline::from_parts(
            receipt.clone(),
            bytes.len() as u64 + 1,
            bytes.to_vec(),
        ),
        Err(OptiScalerStateError::ConfigurationBaselineLengthMismatch)
    ));
    assert!(matches!(
        OptiScalerConfigurationBaseline::present(receipt, b"different".to_vec()),
        Err(OptiScalerStateError::ConfigurationBaselineDigestMismatch)
    ));

    let oversized = vec![b'x'; 16 * 1024 * 1024 + 1];
    let oversized_receipt = FileReceipt::reused("oversized", digest(&oversized)).expect("receipt");
    assert!(matches!(
        OptiScalerConfigurationBaseline::present(oversized_receipt, oversized),
        Err(OptiScalerStateError::ConfigurationBaselineOversize)
    ));
}

#[test]
fn persisted_factory_requires_one_exact_configuration_receipt() {
    let state = managed_state();
    let mut parts = OptiScalerInstallStateParts::from(&state);
    parts.release_files.clear();
    assert!(matches!(
        from_persisted(parts, OptiScalerConfigurationBaseline::absent()),
        Err(OptiScalerStateError::ConfigurationReceiptCardinality)
    ));

    let mut parts = OptiScalerInstallStateParts::from(&state);
    parts.release_files.push(OptiScalerFileReceipt {
        path: PathRef::new("C:/Games/Factory/other.ini").expect("path"),
        installed: FileReceipt::owned("other", digest(b"other")).expect("receipt"),
        role: OptiScalerFileRole::Configuration,
        cleanup: OptiScalerFileCleanup::RemoveIfUnchanged,
        baseline: super::OptiScalerReleaseFileBaseline::Absent,
    });
    assert!(matches!(
        from_persisted(parts, OptiScalerConfigurationBaseline::absent()),
        Err(OptiScalerStateError::ConfigurationReceiptCardinality)
    ));

    let mut parts = OptiScalerInstallStateParts::from(&state);
    parts.release_files[0].path = PathRef::new("C:/Games/Factory/config.ini").expect("path");
    assert!(matches!(
        from_persisted(parts, OptiScalerConfigurationBaseline::absent()),
        Err(OptiScalerStateError::ConfigurationReceiptPath(_))
    ));
}

#[test]
fn baseline_receipt_is_always_reused() {
    let bytes = b"config";
    let owned = FileReceipt::owned("config-id", digest(bytes)).expect("receipt");
    assert!(matches!(
        OptiScalerConfigurationBaseline::present(owned, bytes.to_vec()),
        Err(OptiScalerStateError::ConfigurationBaselineNotReused)
    ));
}

#[test]
fn baseline_present_exposes_exact_length_bytes_and_receipt() {
    let bytes = b"config";
    let value = baseline("config-id", bytes);
    assert_eq!(value.bytes(), Some(bytes.as_slice()));
    assert_eq!(value.length(), Some(bytes.len() as u64));
    assert_eq!(
        value.receipt().expect("receipt").ownership(),
        FileOwnership::Reused
    );
}

#[test]
fn cleanup_serde_uses_only_canonical_spellings() {
    assert_eq!(
        serde_json::to_value(OptiScalerFileCleanup::PreserveCurrentThenRestoreBaseline)
            .expect("serialize restore policy"),
        serde_json::json!("preserve_current_then_restore_baseline")
    );
    assert_eq!(
        serde_json::to_value(OptiScalerFileCleanup::PreserveUnchanged)
            .expect("serialize unchanged policy"),
        serde_json::json!("preserve_unchanged")
    );
    assert!(
        serde_json::from_value::<OptiScalerFileCleanup>(serde_json::json!("recover_then_remove"))
            .is_err()
    );
}
