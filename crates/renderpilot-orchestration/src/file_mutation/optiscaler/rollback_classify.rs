/// Invariant violation or inconsistency detected during rollback classification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RollbackInconsistency {
    pub(crate) reason: String,
}

impl RollbackInconsistency {
    pub(crate) fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
        }
    }
}

impl From<RollbackInconsistency> for ServiceError {
    fn from(err: RollbackInconsistency) -> Self {
        crate::failed(err.reason)
    }
}

/// Recorded transaction state and artifact slot tokens.
pub(crate) struct RollbackRecorded<'a> {
    pub(crate) state: &'a DomainWriteState,
    pub(crate) artifact_custody: &'a DiskObservation,
    pub(crate) artifact_stage: &'a DiskObservation,
    pub(crate) artifact_discard: &'a DiskObservation,
}

/// Authoritative expectations resolved for the operation.
pub(crate) struct RollbackExpected<'a> {
    pub(crate) before: &'a DiskObservation,
    pub(crate) expected: &'a DiskObservation,
}

/// Physical facts observed from disk and private namespaces.
pub(crate) struct RollbackObserved<'a> {
    pub(crate) live: &'a DiskObservation,
    pub(crate) stage: &'a DiskObservation,
    pub(crate) custody: &'a DiskObservation,
    pub(crate) discard: &'a DiskObservation,
}

/// Precise semantic situation describing current recovery phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RollbackSituation {
    /// Live path has not been modified; fast-forward journal to Preserved.
    PublishNotStarted,
    /// Interrupted publish: live file partially overwritten with retained stable identity.
    PublishTorn,
    /// Live file completely published, but private stage artifact remains on disk.
    PublishedStageRetained,
    /// Live file published and stage cleaned up, but journal record not yet updated to Applied.
    PublishCompletePendingAppliedRecord,
    /// Canonical Applied state with valid live postimage; ready for symmetric rollback.
    AppliedReadyForRollback,
    /// Rollback for absent preimage: live postimage must be unlinked.
    DiscardPending,
    /// Live postimage already unlinked; advance journal through PostimageDiscarded to Preserved.
    PostimageDiscarded,
    /// Rollback for existing preimage: live file must be restored from custody bytes.
    RestorePending,
    /// Preimage restored to live path; custody artifact must be deleted and state set to Preserved.
    RestoreCompleteCleanupPending,
    /// Operation is already completely preserved; no further action needed.
    AlreadyPreserved,
}

/// Pure classifier that inspects recorded, expected, and observed facts and determines
/// the exact recovery situation according to transactional invariants.
pub(crate) fn classify_rollback_situation(
    recorded: &RollbackRecorded<'_>,
    expected: &RollbackExpected<'_>,
    observed: &RollbackObserved<'_>,
) -> Result<RollbackSituation, RollbackInconsistency> {
    let state = recorded.state;
    let live = observed.live;
    let before = expected.before;
    let expected_postimage = expected.expected;
    let stage_observed = observed.stage;
    let custody_observed = observed.custody;
    let discard_observed = observed.discard;
    let artifact_custody = recorded.artifact_custody;
    let artifact_stage = recorded.artifact_stage;
    let artifact_discard = recorded.artifact_discard;

    // Phase 1: validate state consistency and check for invariant violations
    match state {
        DomainWriteState::StageIntent { target_digest } => {
            if live != before
                || custody_observed != &DiskObservation::Absent
                || discard_observed != &DiskObservation::Absent
                || artifact_custody != &DiskObservation::Absent
                || artifact_stage != &DiskObservation::Absent
                || artifact_discard != &DiskObservation::Absent
            {
                return Err(RollbackInconsistency::new(
                    "write stage intent observations are inconsistent",
                ));
            }
            if let DiskObservation::File { digest, .. } = stage_observed {
                if digest != target_digest {
                    return Err(RollbackInconsistency::new(
                        "write stage intent observed stage digest does not match target",
                    ));
                }
            } else if stage_observed != &DiskObservation::Absent {
                return Err(RollbackInconsistency::new(
                    "write stage intent has an unsafe private stage",
                ));
            }
        }
        DomainWriteState::Staged {
            stage: expected_stage,
        }
        | DomainWriteState::CaptureIntent {
            stage: expected_stage,
        } => {
            let expected_stage = native(expected_stage);
            if stage_observed != &expected_stage
                || artifact_stage != &expected_stage
                || discard_observed != &DiskObservation::Absent
                || artifact_discard != &DiskObservation::Absent
            {
                return Err(RollbackInconsistency::new(
                    "write capture observations are inconsistent",
                ));
            }
            let capture_complete = (live == &DiskObservation::Absent && custody_observed == before)
                || (before != &DiskObservation::Absent
                    && live == before
                    && matches!(custody_observed, DiskObservation::File { .. })
                    && same_file_digest(custody_observed, before)
                    && custody_observed == artifact_custody);
            let capture_pending = live == before && custody_observed == &DiskObservation::Absent;
            if (!capture_complete || matches!(state, DomainWriteState::Staged { .. }))
                && !capture_pending
                || artifact_custody != custody_observed
            {
                return Err(RollbackInconsistency::new(
                    "write capture tuple is invalid",
                ));
            }
        }
        DomainWriteState::Captured {
            stage: expected_stage,
            custody,
        } => {
            let expected_stage = native(expected_stage);
            let expected_custody = native(custody);
            if stage_observed != &expected_stage
                || artifact_stage != &expected_stage
                || custody_observed != &expected_custody
                || artifact_custody != &expected_custody
                || discard_observed != &DiskObservation::Absent
                || artifact_discard != &DiskObservation::Absent
                || (live != &DiskObservation::Absent
                    && !(live == before
                        && before != &DiskObservation::Absent
                        && same_file_digest(custody_observed, before)))
            {
                return Err(RollbackInconsistency::new(
                    "write captured tuple is invalid",
                ));
            }
            if before != &DiskObservation::Absent && live == &DiskObservation::Absent {
                return Err(RollbackInconsistency::new(
                    "write captured tuple has missing live file",
                ));
            }
        }
        DomainWriteState::PublishIntent {
            stage: expected_stage,
            custody,
        } => {
            let expected_stage = native(expected_stage);
            let expected_custody = native(custody);
            if artifact_stage != &expected_stage
                || artifact_custody != &expected_custody
                || artifact_discard != &DiskObservation::Absent
                || discard_observed != &DiskObservation::Absent
                || custody_observed != &expected_custody
            {
                return Err(RollbackInconsistency::new(
                    "write publish tuple has invalid custody",
                ));
            }
            if before != &DiskObservation::Absent && live == &DiskObservation::Absent {
                return Err(RollbackInconsistency::new(
                    "write publish tuple has missing live file",
                ));
            }
            let pre_publish = stage_observed == &expected_stage
                && (before == &DiskObservation::Absent && live == &DiskObservation::Absent
                    || (before != &DiskObservation::Absent && live == before));
            let post_overwrite = before != &DiskObservation::Absent
                && stage_observed == &expected_stage
                && live == expected_postimage;
            let post_publish = stage_observed == &DiskObservation::Absent && live == expected_postimage;
            let torn_overwrite = before != &DiskObservation::Absent
                && same_file_identity(before, expected_postimage)
                && stage_observed == &expected_stage
                && same_file_identity(live, expected_postimage)
                && live != expected_postimage;
            if !pre_publish && !post_overwrite && !post_publish && !torn_overwrite {
                return Err(RollbackInconsistency::new(
                    "write publish tuple is invalid",
                ));
            }
        }
        DomainWriteState::Applied {
            live: expected_live,
            custody,
        } => {
            let expected_live = native(expected_live);
            let expected_custody = native(custody);
            if live != &expected_live
                || live != expected_postimage
                || stage_observed != &DiskObservation::Absent
                || artifact_stage != &DiskObservation::Absent
                || custody_observed != &expected_custody
                || artifact_custody != &expected_custody
                || discard_observed != &DiskObservation::Absent
                || artifact_discard != &DiskObservation::Absent
            {
                return Err(RollbackInconsistency::new(
                    "write applied tuple is invalid",
                ));
            }
        }
        DomainWriteState::DiscardIntent { postimage, custody } => {
            let postimage = native(postimage);
            let custody = native(custody);
            if expected_postimage != &postimage
                || custody_observed != &custody
                || artifact_custody != &custody
                || stage_observed != &DiskObservation::Absent
                || artifact_stage != &DiskObservation::Absent
                || (live != &postimage && live != &DiskObservation::Absent)
                || (discard_observed != artifact_discard
                    && discard_observed != &DiskObservation::Absent)
            {
                return Err(RollbackInconsistency::new(
                    "write discard tuple is invalid",
                ));
            }
        }
        DomainWriteState::PostimageDiscarded {
            discard: expected_discard,
            custody,
        } => {
            let expected_discard = native(expected_discard);
            let custody = native(custody);
            if expected_postimage != &expected_discard
                || custody_observed != &custody
                || artifact_custody != &custody
                || live != &DiskObservation::Absent
                || (discard_observed != &expected_discard
                    && discard_observed != &DiskObservation::Absent)
            {
                return Err(RollbackInconsistency::new(
                    "write discarded tuple is invalid",
                ));
            }
        }
        DomainWriteState::RestoreIntent {
            preimage,
            discard: expected_discard,
        } => {
            let preimage = native(preimage);
            let expected_discard = native(expected_discard);
            let live_is_exact_or_same_identity =
                live == expected_postimage || same_file_identity(live, before);
            if before != &preimage
                || stage_observed != &DiskObservation::Absent
                || artifact_stage != &DiskObservation::Absent
                || (custody_observed != artifact_custody
                    && !(live == before && custody_observed == &DiskObservation::Absent))
                || !live_is_exact_or_same_identity
                || (discard_observed != &expected_discard
                    && discard_observed != &DiskObservation::Absent)
            {
                return Err(RollbackInconsistency::new(
                    "write restore tuple is invalid",
                ));
            }
        }
        DomainWriteState::Planned | DomainWriteState::Preserved => {}
    }

    // Phase 2: classify into concrete recovery situation
    match state {
        DomainWriteState::Planned => Ok(RollbackSituation::PublishNotStarted),
        DomainWriteState::Preserved => Ok(RollbackSituation::AlreadyPreserved),

        DomainWriteState::StageIntent { .. }
        | DomainWriteState::Staged { .. }
        | DomainWriteState::CaptureIntent { .. }
        | DomainWriteState::Captured { .. } => Ok(RollbackSituation::PublishNotStarted),

        DomainWriteState::PublishIntent { stage, .. } => {
            let expected_stage = native(stage);
            let is_pre_publish = stage_observed == &expected_stage
                && (before == &DiskObservation::Absent && live == &DiskObservation::Absent
                    || (before != &DiskObservation::Absent && live == before));
            if is_pre_publish {
                return Ok(RollbackSituation::PublishNotStarted);
            }

            let is_torn = before != &DiskObservation::Absent
                && same_file_identity(before, expected_postimage)
                && stage_observed != &DiskObservation::Absent
                && same_file_identity(stage_observed, artifact_stage)
                && same_file_identity(live, expected_postimage)
                && live != expected_postimage;
            if is_torn {
                return Ok(RollbackSituation::PublishTorn);
            }

            let is_overwrite_published = before != &DiskObservation::Absent
                && stage_observed == &expected_stage
                && live == expected_postimage;
            if is_overwrite_published {
                return Ok(RollbackSituation::PublishedStageRetained);
            }

            let is_published_complete = stage_observed == &DiskObservation::Absent
                && live == expected_postimage;
            if is_published_complete {
                return Ok(RollbackSituation::PublishCompletePendingAppliedRecord);
            }

            Err(RollbackInconsistency::new("unrecognized publish intent state"))
        }

        DomainWriteState::Applied { .. } => Ok(RollbackSituation::AppliedReadyForRollback),

        DomainWriteState::DiscardIntent { .. } => {
            if live == expected_postimage {
                Ok(RollbackSituation::DiscardPending)
            } else {
                Ok(RollbackSituation::PostimageDiscarded)
            }
        }

        DomainWriteState::PostimageDiscarded { .. } => Ok(RollbackSituation::PostimageDiscarded),

        DomainWriteState::RestoreIntent { .. } => {
            if live == before {
                if custody_observed != &DiskObservation::Absent {
                    Ok(RollbackSituation::RestoreCompleteCleanupPending)
                } else {
                    Ok(RollbackSituation::AlreadyPreserved)
                }
            } else {
                Ok(RollbackSituation::RestorePending)
            }
        }
    }
}
