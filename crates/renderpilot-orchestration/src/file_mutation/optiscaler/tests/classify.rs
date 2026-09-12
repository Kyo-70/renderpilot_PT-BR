use super::*;

fn test_hash(val: &str) -> Sha256Hash {
    Sha256Hash::new(format!("{:0>64}", val)).expect("valid hash")
}

fn file_obs(identity: &str, digest_val: &str) -> DiskObservation {
    DiskObservation::File {
        identity: identity.to_owned(),
        digest: test_hash(digest_val),
    }
}

#[test]
fn classify_publish_not_started_for_planned_and_early_phases() {
    let before = file_obs("file-1", "aaa");
    let expected = file_obs("file-1", "bbb");
    let absent = DiskObservation::Absent;

    let expected_facts = RollbackExpected {
        before: &before,
        expected: &expected,
    };

    // Planned
    let recorded_planned = RollbackRecorded {
        state: &DomainWriteState::Planned,
        artifact_custody: &absent,
        artifact_stage: &absent,
        artifact_discard: &absent,
    };
    let observed_clean = RollbackObserved {
        live: &before,
        stage: &absent,
        custody: &absent,
        discard: &absent,
    };
    assert_eq!(
        classify_rollback_situation(&recorded_planned, &expected_facts, &observed_clean),
        Ok(RollbackSituation::PublishNotStarted)
    );

    // StageIntent
    let recorded_stage_intent = RollbackRecorded {
        state: &DomainWriteState::StageIntent {
            target_digest: test_hash("bbb"),
        },
        artifact_custody: &absent,
        artifact_stage: &absent,
        artifact_discard: &absent,
    };
    assert_eq!(
        classify_rollback_situation(&recorded_stage_intent, &expected_facts, &observed_clean),
        Ok(RollbackSituation::PublishNotStarted)
    );

    // Captured
    let stage_file = file_obs("stage-1", "bbb");
    let custody_file = file_obs("custody-1", "aaa");
    let recorded_captured = RollbackRecorded {
        state: &DomainWriteState::Captured {
            stage: durable(&stage_file),
            custody: durable(&custody_file),
        },
        artifact_custody: &custody_file,
        artifact_stage: &stage_file,
        artifact_discard: &absent,
    };
    let observed_captured = RollbackObserved {
        live: &before,
        stage: &stage_file,
        custody: &custody_file,
        discard: &absent,
    };
    assert_eq!(
        classify_rollback_situation(&recorded_captured, &expected_facts, &observed_captured),
        Ok(RollbackSituation::PublishNotStarted)
    );
}

#[test]
fn classify_publish_phases() {
    let before = file_obs("file-1", "aaa");
    let expected = file_obs("file-1", "bbb");
    let stage_file = file_obs("stage-1", "bbb");
    let custody_file = file_obs("custody-1", "aaa");
    let absent = DiskObservation::Absent;

    let expected_facts = RollbackExpected {
        before: &before,
        expected: &expected,
    };
    let recorded_publish = RollbackRecorded {
        state: &DomainWriteState::PublishIntent {
            stage: durable(&stage_file),
            custody: durable(&custody_file),
        },
        artifact_custody: &custody_file,
        artifact_stage: &stage_file,
        artifact_discard: &absent,
    };

    // 1. Pre-publish: live == before, stage exists
    let observed_pre_publish = RollbackObserved {
        live: &before,
        stage: &stage_file,
        custody: &custody_file,
        discard: &absent,
    };
    assert_eq!(
        classify_rollback_situation(&recorded_publish, &expected_facts, &observed_pre_publish),
        Ok(RollbackSituation::PublishNotStarted)
    );

    // 2. Torn publish: live has same identity as expected, but different digest
    let live_torn = file_obs("file-1", "cccc");
    let observed_torn = RollbackObserved {
        live: &live_torn,
        stage: &stage_file,
        custody: &custody_file,
        discard: &absent,
    };
    assert_eq!(
        classify_rollback_situation(&recorded_publish, &expected_facts, &observed_torn),
        Ok(RollbackSituation::PublishTorn)
    );

    // 3. Post-overwrite with stage retained: live == expected, stage exists
    let observed_post_overwrite = RollbackObserved {
        live: &expected,
        stage: &stage_file,
        custody: &custody_file,
        discard: &absent,
    };
    assert_eq!(
        classify_rollback_situation(&recorded_publish, &expected_facts, &observed_post_overwrite),
        Ok(RollbackSituation::PublishedStageRetained)
    );

    // 4. Complete publish pending record: live == expected, stage absent
    let observed_published_complete = RollbackObserved {
        live: &expected,
        stage: &absent,
        custody: &custody_file,
        discard: &absent,
    };
    assert_eq!(
        classify_rollback_situation(
            &recorded_publish,
            &expected_facts,
            &observed_published_complete
        ),
        Ok(RollbackSituation::PublishCompletePendingAppliedRecord)
    );
}

#[test]
fn classify_applied_ready_for_rollback() {
    let before = file_obs("file-1", "aaa");
    let expected = file_obs("file-1", "bbb");
    let custody_file = file_obs("custody-1", "aaa");
    let absent = DiskObservation::Absent;

    let expected_facts = RollbackExpected {
        before: &before,
        expected: &expected,
    };
    let recorded_applied = RollbackRecorded {
        state: &DomainWriteState::Applied {
            live: durable(&expected),
            custody: durable(&custody_file),
        },
        artifact_custody: &custody_file,
        artifact_stage: &absent,
        artifact_discard: &absent,
    };
    let observed_applied = RollbackObserved {
        live: &expected,
        stage: &absent,
        custody: &custody_file,
        discard: &absent,
    };
    assert_eq!(
        classify_rollback_situation(&recorded_applied, &expected_facts, &observed_applied),
        Ok(RollbackSituation::AppliedReadyForRollback)
    );
}

#[test]
fn classify_discard_phases_for_absent_preimage() {
    let before = DiskObservation::Absent;
    let expected = file_obs("file-1", "bbb");
    let absent = DiskObservation::Absent;

    let expected_facts = RollbackExpected {
        before: &before,
        expected: &expected,
    };
    let recorded_discard = RollbackRecorded {
        state: &DomainWriteState::DiscardIntent {
            postimage: durable(&expected),
            custody: durable(&absent),
        },
        artifact_custody: &absent,
        artifact_stage: &absent,
        artifact_discard: &absent,
    };

    // Live postimage still present -> DiscardPending
    let observed_pending = RollbackObserved {
        live: &expected,
        stage: &absent,
        custody: &absent,
        discard: &absent,
    };
    assert_eq!(
        classify_rollback_situation(&recorded_discard, &expected_facts, &observed_pending),
        Ok(RollbackSituation::DiscardPending)
    );

    // Live postimage already unlinked -> PostimageDiscarded
    let observed_unlinked = RollbackObserved {
        live: &absent,
        stage: &absent,
        custody: &absent,
        discard: &absent,
    };
    assert_eq!(
        classify_rollback_situation(&recorded_discard, &expected_facts, &observed_unlinked),
        Ok(RollbackSituation::PostimageDiscarded)
    );
}

#[test]
fn classify_restore_phases_for_present_preimage() {
    let before = file_obs("file-1", "aaa");
    let expected = file_obs("file-1", "bbb");
    let custody_file = file_obs("custody-1", "aaa");
    let absent = DiskObservation::Absent;

    let expected_facts = RollbackExpected {
        before: &before,
        expected: &expected,
    };
    let recorded_restore = RollbackRecorded {
        state: &DomainWriteState::RestoreIntent {
            preimage: durable(&before),
            discard: durable(&expected),
        },
        artifact_custody: &custody_file,
        artifact_stage: &absent,
        artifact_discard: &absent,
    };

    // 1. Live still contains postimage -> RestorePending
    let observed_pending = RollbackObserved {
        live: &expected,
        stage: &absent,
        custody: &custody_file,
        discard: &absent,
    };
    assert_eq!(
        classify_rollback_situation(&recorded_restore, &expected_facts, &observed_pending),
        Ok(RollbackSituation::RestorePending)
    );

    // 2. Live restored to preimage, but custody artifact remains -> RestoreCompleteCleanupPending
    let observed_restored = RollbackObserved {
        live: &before,
        stage: &absent,
        custody: &custody_file,
        discard: &absent,
    };
    assert_eq!(
        classify_rollback_situation(&recorded_restore, &expected_facts, &observed_restored),
        Ok(RollbackSituation::RestoreCompleteCleanupPending)
    );

    // 3. Live restored and custody removed -> AlreadyPreserved
    let observed_done = RollbackObserved {
        live: &before,
        stage: &absent,
        custody: &absent,
        discard: &absent,
    };
    assert_eq!(
        classify_rollback_situation(&recorded_restore, &expected_facts, &observed_done),
        Ok(RollbackSituation::AlreadyPreserved)
    );
}

#[test]
fn differential_case_1_stage_intent_missing_live_rejects_inconsistent() {
    let before = file_obs("file-1", "aaa");
    let expected = file_obs("file-1", "bbb");
    let absent = DiskObservation::Absent;

    let expected_facts = RollbackExpected {
        before: &before,
        expected: &expected,
    };
    let recorded_stage_intent = RollbackRecorded {
        state: &DomainWriteState::StageIntent {
            target_digest: test_hash("bbb"),
        },
        artifact_custody: &absent,
        artifact_stage: &absent,
        artifact_discard: &absent,
    };
    let observed_missing_live = RollbackObserved {
        live: &absent,
        stage: &absent,
        custody: &absent,
        discard: &absent,
    };
    let result = classify_rollback_situation(
        &recorded_stage_intent,
        &expected_facts,
        &observed_missing_live,
    );
    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err().reason,
        "write stage intent observations are inconsistent"
    );
}

#[test]
fn differential_case_2_staged_missing_live_rejects_invalid() {
    let before = file_obs("file-1", "aaa");
    let expected = file_obs("file-1", "bbb");
    let stage_file = file_obs("stage-1", "bbb");
    let absent = DiskObservation::Absent;

    let expected_facts = RollbackExpected {
        before: &before,
        expected: &expected,
    };
    let recorded_staged = RollbackRecorded {
        state: &DomainWriteState::Staged {
            stage: durable(&stage_file),
        },
        artifact_custody: &absent,
        artifact_stage: &stage_file,
        artifact_discard: &absent,
    };
    let observed_missing_live = RollbackObserved {
        live: &absent,
        stage: &stage_file,
        custody: &absent,
        discard: &absent,
    };
    let result =
        classify_rollback_situation(&recorded_staged, &expected_facts, &observed_missing_live);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().reason, "write capture tuple is invalid");
}

#[test]
fn differential_case_3_capture_intent_missing_live_rejects_invalid() {
    let before = file_obs("file-1", "aaa");
    let expected = file_obs("file-1", "bbb");
    let stage_file = file_obs("stage-1", "bbb");
    let absent = DiskObservation::Absent;

    let expected_facts = RollbackExpected {
        before: &before,
        expected: &expected,
    };
    let recorded_capture_intent = RollbackRecorded {
        state: &DomainWriteState::CaptureIntent {
            stage: durable(&stage_file),
        },
        artifact_custody: &absent,
        artifact_stage: &stage_file,
        artifact_discard: &absent,
    };
    let observed_missing_live = RollbackObserved {
        live: &absent,
        stage: &stage_file,
        custody: &absent,
        discard: &absent,
    };
    let result = classify_rollback_situation(
        &recorded_capture_intent,
        &expected_facts,
        &observed_missing_live,
    );
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().reason, "write capture tuple is invalid");
}

#[test]
fn tightened_case_4_captured_missing_live_rejects_as_inconsistent() {
    let before = file_obs("file-1", "aaa");
    let expected = file_obs("file-1", "bbb");
    let stage_file = file_obs("stage-1", "bbb");
    let custody_file = file_obs("custody-1", "aaa");
    let absent = DiskObservation::Absent;

    let expected_facts = RollbackExpected {
        before: &before,
        expected: &expected,
    };
    let recorded_captured = RollbackRecorded {
        state: &DomainWriteState::Captured {
            stage: durable(&stage_file),
            custody: durable(&custody_file),
        },
        artifact_custody: &custody_file,
        artifact_stage: &stage_file,
        artifact_discard: &absent,
    };
    let observed_missing_live = RollbackObserved {
        live: &absent,
        stage: &stage_file,
        custody: &custody_file,
        discard: &absent,
    };
    let result =
        classify_rollback_situation(&recorded_captured, &expected_facts, &observed_missing_live);
    // Step 2 semantic tightening: missing live when preimage existed is an anomaly
    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err().reason,
        "write captured tuple has missing live file"
    );
}

#[test]
fn tightened_case_5_publish_intent_missing_live_rejects_as_inconsistent() {
    let before = file_obs("file-1", "aaa");
    let expected = file_obs("file-1", "bbb");
    let stage_file = file_obs("stage-1", "bbb");
    let custody_file = file_obs("custody-1", "aaa");
    let absent = DiskObservation::Absent;

    let expected_facts = RollbackExpected {
        before: &before,
        expected: &expected,
    };
    let recorded_publish = RollbackRecorded {
        state: &DomainWriteState::PublishIntent {
            stage: durable(&stage_file),
            custody: durable(&custody_file),
        },
        artifact_custody: &custody_file,
        artifact_stage: &stage_file,
        artifact_discard: &absent,
    };
    let observed_missing_live = RollbackObserved {
        live: &absent,
        stage: &stage_file,
        custody: &custody_file,
        discard: &absent,
    };
    let result =
        classify_rollback_situation(&recorded_publish, &expected_facts, &observed_missing_live);
    // Step 2 semantic tightening: missing live before publish is an anomaly when preimage existed
    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err().reason,
        "write publish tuple has missing live file"
    );
}

#[test]
fn pre_publish_absent_preimage_missing_live_still_accepts_as_publish_not_started() {
    let before = DiskObservation::Absent;
    let expected = file_obs("file-1", "bbb");
    let stage_file = file_obs("stage-1", "bbb");
    let absent = DiskObservation::Absent;

    let expected_facts = RollbackExpected {
        before: &before,
        expected: &expected,
    };
    let recorded_publish = RollbackRecorded {
        state: &DomainWriteState::PublishIntent {
            stage: durable(&stage_file),
            custody: durable(&absent),
        },
        artifact_custody: &absent,
        artifact_stage: &stage_file,
        artifact_discard: &absent,
    };
    let observed_missing_live = RollbackObserved {
        live: &absent,
        stage: &stage_file,
        custody: &absent,
        discard: &absent,
    };
    let result =
        classify_rollback_situation(&recorded_publish, &expected_facts, &observed_missing_live);
    assert_eq!(result, Ok(RollbackSituation::PublishNotStarted));
}

#[test]
fn classify_rejects_corrupted_foreign_live_during_restore() {
    let before = file_obs("file-1", "aaa");
    let expected = file_obs("file-1", "bbb");
    let custody_file = file_obs("custody-1", "aaa");
    let foreign_file = file_obs("foreign-identity", "dddd");
    let absent = DiskObservation::Absent;

    let expected_facts = RollbackExpected {
        before: &before,
        expected: &expected,
    };
    let recorded_restore = RollbackRecorded {
        state: &DomainWriteState::RestoreIntent {
            preimage: durable(&before),
            discard: durable(&expected),
        },
        artifact_custody: &custody_file,
        artifact_stage: &absent,
        artifact_discard: &absent,
    };

    // Live is neither expected postimage nor preimage -> must fail
    let observed_corrupted = RollbackObserved {
        live: &foreign_file,
        stage: &absent,
        custody: &custody_file,
        discard: &absent,
    };
    let result =
        classify_rollback_situation(&recorded_restore, &expected_facts, &observed_corrupted);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().reason, "write restore tuple is invalid");
}

#[test]
fn classify_already_preserved_is_terminal() {
    let before = file_obs("file-1", "aaaa");
    let expected = file_obs("file-1", "bbbb");
    let absent = DiskObservation::Absent;

    let expected_facts = RollbackExpected {
        before: &before,
        expected: &expected,
    };
    let recorded_preserved = RollbackRecorded {
        state: &DomainWriteState::Preserved,
        artifact_custody: &absent,
        artifact_stage: &absent,
        artifact_discard: &absent,
    };
    let observed = RollbackObserved {
        live: &before,
        stage: &absent,
        custody: &absent,
        discard: &absent,
    };
    assert_eq!(
        classify_rollback_situation(&recorded_preserved, &expected_facts, &observed),
        Ok(RollbackSituation::AlreadyPreserved)
    );
}

#[test]
fn differential_case_6_restore_intent_live_restored_custody_mismatch_cleans_up() {
    let before = file_obs("file-1", "aaaa");
    let expected = file_obs("file-1", "bbbb");
    let custody_corrupted = file_obs("custody-1", "cccc");
    let absent = DiskObservation::Absent;

    let expected_facts = RollbackExpected {
        before: &before,
        expected: &expected,
    };
    let recorded_restore = RollbackRecorded {
        state: &DomainWriteState::RestoreIntent {
            preimage: durable(&before),
            discard: durable(&expected),
        },
        artifact_custody: &custody_corrupted,
        artifact_stage: &absent,
        artifact_discard: &absent,
    };
    // Baseline equivalence: when live == before, restore is already complete on disk.
    // Custody digest is not inspected; custody artifact is unlinked as cleanup.
    let observed = RollbackObserved {
        live: &before,
        stage: &absent,
        custody: &custody_corrupted,
        discard: &absent,
    };
    let result = classify_rollback_situation(&recorded_restore, &expected_facts, &observed);
    assert_eq!(result, Ok(RollbackSituation::RestoreCompleteCleanupPending));
}

#[test]
fn differential_case_6_restore_intent_live_still_postimage_pending_restore() {
    let before = file_obs("file-1", "aaaa");
    let expected = file_obs("file-1", "bbbb");
    let custody_corrupted = file_obs("custody-1", "cccc");
    let absent = DiskObservation::Absent;

    let expected_facts = RollbackExpected {
        before: &before,
        expected: &expected,
    };
    let recorded_restore = RollbackRecorded {
        state: &DomainWriteState::RestoreIntent {
            preimage: durable(&before),
            discard: durable(&expected),
        },
        artifact_custody: &custody_corrupted,
        artifact_stage: &absent,
        artifact_discard: &absent,
    };
    // When live == expected, transaction is classified as RestorePending.
    // The custody bytes/digest verification happens inside the physical executor (execute_restore_rollback).
    let observed = RollbackObserved {
        live: &expected,
        stage: &absent,
        custody: &custody_corrupted,
        discard: &absent,
    };
    let result = classify_rollback_situation(&recorded_restore, &expected_facts, &observed);
    assert_eq!(result, Ok(RollbackSituation::RestorePending));
}

#[test]
fn classify_rejects_custody_token_drift_during_restore() {
    let before = file_obs("file-1", "aaaa");
    let expected = file_obs("file-1", "bbbb");
    let custody_recorded = file_obs("custody-1", "aaaa");
    let custody_drifted = file_obs("custody-drift", "aaaa");
    let absent = DiskObservation::Absent;

    let expected_facts = RollbackExpected {
        before: &before,
        expected: &expected,
    };
    let recorded_restore = RollbackRecorded {
        state: &DomainWriteState::RestoreIntent {
            preimage: durable(&before),
            discard: durable(&expected),
        },
        artifact_custody: &custody_recorded,
        artifact_stage: &absent,
        artifact_discard: &absent,
    };
    let observed = RollbackObserved {
        live: &expected,
        stage: &absent,
        custody: &custody_drifted,
        discard: &absent,
    };
    let result = classify_rollback_situation(&recorded_restore, &expected_facts, &observed);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().reason, "write restore tuple is invalid");
}

#[test]
fn classify_rejects_unexpected_live_in_discard_intent() {
    let absent = DiskObservation::Absent;
    let expected = file_obs("new-file", "bbbb");
    let foreign = file_obs("foreign-file", "cccc");

    let expected_facts = RollbackExpected {
        before: &absent,
        expected: &expected,
    };
    let recorded_discard = RollbackRecorded {
        state: &DomainWriteState::DiscardIntent {
            postimage: durable(&expected),
            custody: durable(&absent),
        },
        artifact_custody: &absent,
        artifact_stage: &absent,
        artifact_discard: &absent,
    };
    let observed = RollbackObserved {
        live: &foreign,
        stage: &absent,
        custody: &absent,
        discard: &absent,
    };
    let result = classify_rollback_situation(&recorded_discard, &expected_facts, &observed);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().reason, "write discard tuple is invalid");
}

#[test]
fn classify_torn_publish_preserves_stable_identity() {
    let before = file_obs("stable-handle-1", "aaaa");
    let expected = file_obs("stable-handle-1", "bbbb");
    let stage = file_obs("stage-token", "bbbb");
    let custody = file_obs("custody-token", "aaaa");
    let absent = DiskObservation::Absent;

    let expected_facts = RollbackExpected {
        before: &before,
        expected: &expected,
    };
    let recorded = RollbackRecorded {
        state: &DomainWriteState::PublishIntent {
            stage: durable(&stage),
            custody: durable(&custody),
        },
        artifact_custody: &custody,
        artifact_stage: &stage,
        artifact_discard: &absent,
    };

    // Live has same stable identity but partially written content ("cccc")
    let live_partially_written = file_obs("stable-handle-1", "cccc");
    let observed = RollbackObserved {
        live: &live_partially_written,
        stage: &stage,
        custody: &custody,
        discard: &absent,
    };

    assert_eq!(
        classify_rollback_situation(&recorded, &expected_facts, &observed),
        Ok(RollbackSituation::PublishTorn)
    );
}
