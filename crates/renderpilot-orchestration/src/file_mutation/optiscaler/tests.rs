use super::*;
use std::fs;

mod classify;
mod journal;
mod recovery;

fn owned_receipt(path: &Path) -> FileReceipt {
    let (parent, leaf) = crate::fs::verified_parent(path).expect("verified parent");
    let entry = parent
        .observe_leaf(&leaf)
        .expect("observe file")
        .expect("file exists");
    let digest = Sha256Hash::new(entry.digest.expect("file digest")).expect("digest");
    FileReceipt::owned(entry.identity, digest).expect("owned receipt")
}

fn planned_journal(root: &Path, operations: &[OptiScalerPlannedOperation]) -> OptiScalerJournal {
    let scope = MutationScope::single(root).expect("scope");
    build_journal(
        &scope,
        &root.join("transaction-id"),
        operations,
        ThreatModel::CooperativeSameUid,
    )
    .expect("journal")
}

fn prepared_for<'a>(
    executor: &'a PeerMutationExecutor,
    root: &Path,
    journal: OptiScalerJournal,
    next_operation_id: usize,
) -> PreparedFileMutation<'a> {
    let storage = executor.repositories();
    let game_id = renderpilot_domain::GameId::new("test:optiscaler").expect("game id");
    seed_optiscaler_test_game(storage, &game_id, root);
    let journal_json = serde_json::to_string(&journal).expect("journal json");
    let preparing = executor
        .begin_optiscaler_journal_aggregate(
            OptiScalerJournalAggregateBegin::new(
                "test-mutation",
                game_id.clone(),
                "optiscaler_install",
                None,
                journal_json.clone(),
            )
            .expect("begin aggregate"),
        )
        .expect("reserve mutation");
    PreparedFileMutation {
        id: "test-mutation".to_owned(),
        game_id,
        executor,
        authority: JournalAuthority::ActivePreparing(Box::new(preparing)),
        transaction_root: root.to_path_buf(),
        journal_json,
        journal,
        next_operation_id,
        managed_endpoint_roots: std::collections::HashMap::new(),
    }
}

fn seed_optiscaler_test_game(
    storage: &SqliteStorage,
    game_id: &renderpilot_domain::GameId,
    root: &Path,
) {
    let game = renderpilot_domain::GameInstallation::new(
        renderpilot_domain::GameIdentity::new(
            game_id.clone(),
            "OptiScaler test",
            renderpilot_domain::Launcher::Manual,
        )
        .expect("game identity"),
        renderpilot_domain::Platform::Windows,
        renderpilot_domain::GameRuntime::NativeWindows,
        renderpilot_domain::PathRef::new(root.to_string_lossy().replace('\\', "/"))
            .expect("game root"),
    );
    renderpilot_application::GameRepository::upsert_game(storage, &game).expect("store game");
}
