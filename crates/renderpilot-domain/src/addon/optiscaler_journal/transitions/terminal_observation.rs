static ABSENT: DurableObservation = DurableObservation::Absent;

fn terminal_observation(
    effect: &OperationEffect,
    endpoint: Endpoint,
) -> Option<&DurableObservation> {
    match effect {
        OperationEffect::Write(payload) => match payload.state() {
            WriteState::Applied { live, .. } if endpoint == Endpoint::Single => Some(live),
            WriteState::DiscardIntent { postimage, .. } if endpoint == Endpoint::Single => {
                Some(postimage)
            }
            WriteState::PostimageDiscarded { discard, .. } if endpoint == Endpoint::Single => {
                Some(discard)
            }
            WriteState::RestoreIntent { discard, .. } if endpoint == Endpoint::Single => {
                Some(discard)
            }
            _ => None,
        },
        OperationEffect::Delete(payload) => match payload.state() {
            DeleteState::Applied { .. } | DeleteState::RestoreIntent { .. }
                if endpoint == Endpoint::Single =>
            {
                Some(&ABSENT)
            }
            _ => None,
        },
        OperationEffect::Verify(payload) => match payload.state() {
            VerifyState::Applied { observed } if endpoint == Endpoint::Single => Some(observed),
            _ => None,
        },
        OperationEffect::Relocate(payload) => match (payload.state(), endpoint) {
            (
                RelocateState::Applied {
                    source_after,
                    destination_after: _,
                },
                Endpoint::Source,
            ) => Some(source_after),
            (
                RelocateState::Applied {
                    source_after: _,
                    destination_after,
                },
                Endpoint::Destination,
            ) => Some(destination_after),
            _ => None,
        },
        OperationEffect::CreateDirectory(payload) => match payload.state() {
            CreateDirectoryState::Applied { live } if endpoint == Endpoint::Single => Some(live),
            CreateDirectoryState::DiscardIntent { directory }
            | CreateDirectoryState::PostimageDiscarded { discard: directory }
                if endpoint == Endpoint::Single =>
            {
                Some(directory)
            }
            _ => None,
        },
        OperationEffect::PostCommitRemoveDirectory(payload) => match payload.state() {
            RemoveDirectoryState::Applied { .. } if endpoint == Endpoint::Single => Some(&ABSENT),
            _ => None,
        },
    }
}
