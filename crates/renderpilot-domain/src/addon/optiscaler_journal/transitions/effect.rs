/// Validates one action transition and returns its direction.  The state graph
/// is intentionally owned here so storage can persist it without duplicating
/// action-edge rules.
pub fn validate_optiscaler_effect_transition(
    current: &OperationEffect,
    next: &OperationEffect,
) -> Result<OptiScalerTransitionDirection, OptiScalerJournalError> {
    let direction = match (current, next) {
        (OperationEffect::Write(left), OperationEffect::Write(right)) => {
            use WriteState::*;
            match (left.state(), right.state()) {
                (Planned, StageIntent { .. })
                | (StageIntent { .. }, Staged { .. })
                | (Staged { .. }, CaptureIntent { .. })
                | (CaptureIntent { .. }, Captured { .. })
                | (Captured { .. }, PublishIntent { .. })
                | (PublishIntent { .. }, Applied { .. }) => OptiScalerTransitionDirection::Forward,
                (Applied { .. }, DiscardIntent { .. }) |
(DiscardIntent { .. }, PostimageDiscarded { .. }) |
(PostimageDiscarded { .. } | RestoreIntent { .. } | StageIntent { .. } |
Staged { .. } | CaptureIntent { .. } | Captured { .. } | PublishIntent { .. }
| Planned, Preserved) | (Applied { .. }, RestoreIntent { .. }) => OptiScalerTransitionDirection::Reverse,
                _ => {
                    return Err(OptiScalerJournalError::Invalid(
                        "write action transition is not legal",
                    ));
                }
            }
        }
        (OperationEffect::Delete(left), OperationEffect::Delete(right)) => {
            use DeleteState::*;
            match (left.state(), right.state()) {
                (Planned, CaptureIntent)
                | (CaptureIntent, Captured { .. })
                | (Captured { .. }, Applied { .. }) => OptiScalerTransitionDirection::Forward,
                (Applied { .. }, RestoreIntent { .. }) |
(RestoreIntent { .. } | CaptureIntent | Planned, Preserved) => OptiScalerTransitionDirection::Reverse,
                _ => {
                    return Err(OptiScalerJournalError::Invalid(
                        "delete action transition is not legal",
                    ));
                }
            }
        }
        (OperationEffect::Verify(left), OperationEffect::Verify(right)) => {
            use VerifyState::*;
            match (left.state(), right.state()) {
                (Planned, Applied { .. }) => OptiScalerTransitionDirection::Forward,
                (Applied { .. } | Planned, Preserved) => {
                    OptiScalerTransitionDirection::Reverse
                }
                _ => {
                    return Err(OptiScalerJournalError::Invalid(
                        "verify action transition is not legal",
                    ));
                }
            }
        }
        (OperationEffect::Relocate(left), OperationEffect::Relocate(right)) => {
            use RelocateState::*;
            if !same_endpoint_identity(left.source(), right.source())
                || !same_endpoint_identity(left.destination(), right.destination())
            {
                return Err(OptiScalerJournalError::Invalid(
                    "relocation transition changed endpoint identity",
                ));
            }
            match (left.state(), right.state()) {
                (Planned, MoveIntent) | (MoveIntent, Applied { .. }) => {
                    OptiScalerTransitionDirection::Forward
                }
                (Applied { .. }, ReverseIntent) |
(ReverseIntent | MoveIntent | Planned, Preserved) => OptiScalerTransitionDirection::Reverse,
                _ => {
                    return Err(OptiScalerJournalError::Invalid(
                        "relocation action transition is not legal",
                    ));
                }
            }
        }
        (OperationEffect::CreateDirectory(left), OperationEffect::CreateDirectory(right)) => {
            use CreateDirectoryState::*;
            match (left.state(), right.state()) {
                (Planned, StageIntent)
                | (StageIntent, Staged { .. })
                | (Staged { .. }, PublishIntent { .. })
                | (PublishIntent { .. }, Applied { .. }) => OptiScalerTransitionDirection::Forward,
                (Applied { .. }, DiscardIntent { .. }) |
(DiscardIntent { .. }, PostimageDiscarded { .. }) |
(PostimageDiscarded { .. } | StageIntent | Staged { .. } | PublishIntent { ..
} | Planned, Preserved) => OptiScalerTransitionDirection::Reverse,
                _ => {
                    return Err(OptiScalerJournalError::Invalid(
                        "directory action transition is not legal",
                    ));
                }
            }
        }
        (
            OperationEffect::PostCommitRemoveDirectory(left),
            OperationEffect::PostCommitRemoveDirectory(right),
        ) => {
            use RemoveDirectoryState::*;
            match (left.state(), right.state()) {
                (Planned { .. }, RemoveIntent { .. }) | (RemoveIntent { .. }, Applied { .. }) => {
                    OptiScalerTransitionDirection::Forward
                }
                _ => {
                    return Err(OptiScalerJournalError::Invalid(
                        "post-commit directory transition is not legal",
                    ));
                }
            }
        }
        _ => {
            return Err(OptiScalerJournalError::Invalid(
                "operation transition changed its action kind",
            ));
        }
    };
    validate_action_payload_transition(current, next)?;
    Ok(direction)
}
