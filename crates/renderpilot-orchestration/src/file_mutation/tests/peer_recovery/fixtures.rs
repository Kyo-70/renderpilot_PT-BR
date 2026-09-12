use std::fs;
use std::path::Path;

use renderpilot_domain::{GameId, PathRef};
use renderpilot_storage_sqlite::BeginFileMutationPreparation;
use serde_json::json;

use crate::Context;

pub(super) const BEFORE_DIGEST: &str =
    "6db7d803e74f1ffa7d8f5adc0bf95b3e15bf4c8373fffadf546227cc6c6742cb";
pub(super) const AFTER_DIGEST: &str =
    "f778dacc09a326990d63568063853fdffb39d070ee6049054e23b1a666d1bf63";

pub(super) fn peer_manifest(
    root: &Path,
    transaction_dir: &Path,
    id: &str,
    target: &Path,
) -> String {
    let before_identity = fixture_identity(target).unwrap_or_else(|| "before-identity".to_owned());
    json!({
        "format_version": 1,
        "roots": [slash(root)],
        "transaction_dir": slash(transaction_dir),
        "snapshots": [{
            "path": slash(target),
            "snapshot": slash(&transaction_dir.join("before.bin"))
        }],
        "peer_program": {
            "format": 1,
            "transaction_owner": id,
            "execution_class": "ordinary",
            "roots": [slash(root)],
            "stage": [],
            "custody": [format!("{}:peer.dll", slash(root))],
            "created_ancestors": [],
            "endpoints": [{
                "ordinal": 0,
                "path": slash(target),
                "role": "disjoint",
                "operation": "replace",
                "planned_sha256": AFTER_DIGEST,
                "planned_length": 6,
                "before": {
                    "identity": before_identity,
                    "sha256": BEFORE_DIGEST,
                    "length": 6
                },
                "read_guards": [format!("{}:peer.dll", slash(root))],
                "subtree_publishes": []
            }]
        }
    })
    .to_string()
}

pub(super) fn prepare_peer_row(
    context: &Context,
    game_id: &GameId,
    id: &str,
    target: &Path,
) -> (String, std::path::PathBuf) {
    let transaction_dir = context.file_mutation_root().join(id);
    fs::create_dir_all(&transaction_dir).expect("transaction directory");
    fs::write(transaction_dir.join("before.bin"), b"before").expect("before snapshot");
    if !target.exists() {
        fs::write(target, b"before").expect("before target");
    }
    let manifest = peer_manifest(target.parent().expect("root"), &transaction_dir, id, target);
    context
        .storage()
        .begin_file_mutation_preparation(&BeginFileMutationPreparation {
            id: id.to_owned(),
            game_id: game_id.clone(),
            feature: renderpilot_domain::mutation_features::LUMA_INSTALL.to_owned(),
            subject_id: None,
            initial_manifest_json: "{}".to_owned(),
        })
        .expect("begin peer row");
    context
        .storage()
        .finish_preparing_file_mutation(id, &manifest)
        .expect("finish peer row");
    (manifest, transaction_dir)
}

pub(super) fn slash(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

pub(super) fn nested_peer_manifest(
    root: &Path,
    transaction_dir: &Path,
    id: &str,
    target: &Path,
    snapshot: Option<&Path>,
) -> String {
    let root = slash(root);
    let target = slash(target);
    let nested = Path::new(&root).join("nested");
    let deeper = nested.join("deeper");
    let nested_token = format!("{root}:nested");
    let deeper_token = format!("{root}:nested/deeper");
    let endpoint_token = format!("{root}:nested/deeper/peer.dll");
    json!({
        "format_version": 1,
        "roots": [root],
        "transaction_dir": slash(transaction_dir),
        "snapshots": [{
            "path": target,
            "snapshot": snapshot.map(slash)
        }],
        "peer_ancestors": [
            {"path": slash(&nested), "consumer_ordinals": [0]},
            {"path": slash(&deeper), "consumer_ordinals": [0]}
        ],
        "peer_program": {
            "format": 1,
            "transaction_owner": id,
            "execution_class": "ordinary",
            "roots": [root],
            "stage": [],
            "custody": [],
            "created_ancestors": [nested_token, deeper_token],
            "endpoints": [{
                "ordinal": 0,
                "path": target,
                "role": "disjoint",
                "operation": "create",
                "planned_sha256": AFTER_DIGEST,
                "planned_length": 6,
                "before": null,
                "read_guards": [endpoint_token],
                "subtree_publishes": [nested_token, deeper_token]
            }]
        }
    })
    .to_string()
}

pub(super) fn prepare_nested_peer_row(
    context: &Context,
    game_id: &GameId,
    id: &str,
    root: &Path,
    target: &Path,
    snapshot: Option<&Path>,
) -> std::path::PathBuf {
    let transaction_dir = context.file_mutation_root().join(id);
    fs::create_dir_all(&transaction_dir).expect("transaction directory");
    if let Some(snapshot) = snapshot {
        fs::write(snapshot, b"before").expect("before snapshot");
    }
    let manifest = nested_peer_manifest(root, &transaction_dir, id, target, snapshot);
    context
        .storage()
        .begin_file_mutation_preparation(&BeginFileMutationPreparation {
            id: id.to_owned(),
            game_id: game_id.clone(),
            feature: renderpilot_domain::mutation_features::LUMA_INSTALL.to_owned(),
            subject_id: None,
            initial_manifest_json: "{}".to_owned(),
        })
        .expect("begin peer row");
    context
        .storage()
        .finish_preparing_file_mutation(id, &manifest)
        .expect("finish peer row");
    transaction_dir
}

pub(super) fn ordinary_remove_manifest(
    root: &Path,
    transaction_dir: &Path,
    id: &str,
    target: &Path,
) -> String {
    let before_identity = fixture_identity(target).unwrap_or_else(|| "before-identity".to_owned());
    let root = slash(root);
    let target = slash(target);
    let token = format!(
        "{root}:{}",
        target.trim_start_matches(&root).trim_start_matches('/')
    );
    json!({
        "format_version": 1,
        "roots": [root],
        "transaction_dir": slash(transaction_dir),
        "snapshots": [{
            "path": target,
            "snapshot": slash(&transaction_dir.join("before.bin"))
        }],
        "peer_program": {
            "format": 1,
            "transaction_owner": id,
            "execution_class": "ordinary",
            "roots": [root],
            "stage": [],
            "custody": [token],
            "created_ancestors": [],
            "endpoints": [{
                "ordinal": 0,
                "path": target,
                "role": "disjoint",
                "operation": "remove",
                "before": {
                    "identity": before_identity,
                    "sha256": BEFORE_DIGEST,
                    "length": 6
                },
                "read_guards": [token],
                "subtree_publishes": []
            }]
        }
    })
    .to_string()
}

pub(super) fn prepare_ordinary_remove_row(
    context: &Context,
    game_id: &GameId,
    id: &str,
    root: &Path,
    target: &Path,
) -> std::path::PathBuf {
    let transaction_dir = context.file_mutation_root().join(id);
    fs::create_dir_all(&transaction_dir).expect("transaction directory");
    fs::write(transaction_dir.join("before.bin"), b"before").expect("before snapshot");
    fs::write(target, b"before").expect("before target");
    let manifest = ordinary_remove_manifest(root, &transaction_dir, id, target);
    fs::remove_file(target).expect("removed target");
    context
        .storage()
        .begin_file_mutation_preparation(&BeginFileMutationPreparation {
            id: id.to_owned(),
            game_id: game_id.clone(),
            feature: renderpilot_domain::mutation_features::LUMA_INSTALL.to_owned(),
            subject_id: None,
            initial_manifest_json: "{}".to_owned(),
        })
        .expect("begin ordinary remove row");
    context
        .storage()
        .finish_preparing_file_mutation(id, &manifest)
        .expect("finish ordinary remove row");
    transaction_dir
}

fn fixture_identity(path: &Path) -> Option<String> {
    let path = PathRef::new(slash(path)).ok()?;
    match crate::peer_mutation_executor::observe_peer_path_state(&path).ok()? {
        crate::peer_mutation_executor::PeerPathObservation::File { observation, .. } => {
            Some(observation.identity)
        }
        crate::peer_mutation_executor::PeerPathObservation::Absent { .. } => None,
    }
}
