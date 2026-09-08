use super::*;

#[test]
fn initial_planned_chain_accepts_an_unresolved_prior_postimage() {
    let chain = chained_journal(WriteState::Planned, WriteState::Planned, false);
    validate_optiscaler_journal_for_begin(&serde_json::to_string(&chain).expect("chain json"))
        .expect("planned producer and consumer");
}

#[test]
fn producer_partial_progress_accepts_a_planned_future_consumer() {
    let current = chained_journal(WriteState::Planned, WriteState::Planned, false);
    let mut next = current.clone();
    if let OperationEffect::Write(effect) = next.operations_mut()[0].effect_mut() {
        *effect.state_mut() = WriteState::StageIntent {
            target_digest: hash(),
        };
    }
    validate_optiscaler_journal_for_cas(
        &serde_json::to_string(&current).expect("current json"),
        &serde_json::to_string(&next).expect("next json"),
        "preparing",
    )
    .expect("producer partial progress");
}

#[test]
fn rollback_preserved_consumer_may_wait_for_an_unresolved_prior_postimage() {
    let current = chained_journal(
        WriteState::StageIntent {
            target_digest: hash(),
        },
        WriteState::Planned,
        false,
    );
    let mut next = current.clone();
    if let OperationEffect::Write(effect) = next.operations_mut()[1].effect_mut() {
        *effect.state_mut() = WriteState::Preserved;
    }
    validate_optiscaler_journal_for_cas(
        &serde_json::to_string(&current).expect("current json"),
        &serde_json::to_string(&next).expect("preserved json"),
        "preparing",
    )
    .expect("rollback preserved consumer may retain a pending postimage");
}

#[test]
fn consumer_progress_before_an_applied_producer_is_rejected() {
    let current = chained_journal(
        WriteState::StageIntent {
            target_digest: hash(),
        },
        WriteState::Planned,
        false,
    );
    let mut next = current.clone();
    if let OperationEffect::Write(effect) = next.operations_mut()[1].effect_mut() {
        *effect.state_mut() = WriteState::StageIntent {
            target_digest: hash(),
        };
    }
    assert!(
        validate_optiscaler_journal_for_cas(
            &serde_json::to_string(&current).expect("current json"),
            &serde_json::to_string(&next).expect("next json"),
            "preparing",
        )
        .is_err()
    );
}

#[test]
fn consumer_progress_after_an_applied_producer_is_accepted() {
    let current = chained_journal(
        WriteState::Applied {
            live: DurableObservation::File {
                identity: "producer-live".to_owned(),
                digest: hash(),
            },
            custody: DurableObservation::Absent,
        },
        WriteState::Planned,
        false,
    );
    let mut next = current.clone();
    if let OperationEffect::Write(effect) = next.operations_mut()[1].effect_mut() {
        *effect.state_mut() = WriteState::StageIntent {
            target_digest: hash(),
        };
    }
    validate_optiscaler_journal_for_cas(
        &serde_json::to_string(&current).expect("current json"),
        &serde_json::to_string(&next).expect("next json"),
        "preparing",
    )
    .expect("consumer progress after producer");
}

#[test]
fn finish_rejects_an_unresolved_prior_postimage_chain() {
    let chain = chained_journal(
        WriteState::StageIntent {
            target_digest: hash(),
        },
        WriteState::Planned,
        true,
    );
    assert!(
        validate_optiscaler_journal_for_prepared(&serde_json::to_string(&chain).expect("json"))
            .is_err()
    );
}
