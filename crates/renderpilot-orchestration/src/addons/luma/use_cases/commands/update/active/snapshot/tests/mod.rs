mod policy;
mod validation;

use std::path::PathBuf;

use renderpilot_domain::{AddonKind, FileReceipt, GameId, PathRef, Sha256Hash};

use crate::addons::luma::types::LumaExternalRequirement;
use crate::addons::luma::use_cases::update_target::ResolvedUpdateTarget;

pub(super) fn game_id() -> GameId {
    GameId::new("snapshot-test-game").expect("game id")
}

pub(super) fn path(value: impl Into<String>) -> PathRef {
    PathRef::new(value.into()).expect("path")
}

pub(super) fn target(requirement: Option<LumaExternalRequirement>) -> ResolvedUpdateTarget {
    target_at(PathBuf::from("C:/Games/Snapshot"), requirement)
}

pub(super) fn target_at(
    game_dir: PathBuf,
    requirement: Option<LumaExternalRequirement>,
) -> ResolvedUpdateTarget {
    ResolvedUpdateTarget {
        game_dir,
        asset: "luma.zip".to_owned(),
        addon_file: "Luma.addon64".to_owned(),
        arch: renderpilot_domain::Architecture::X64,
        proxy_dll_name: "dxgi.dll".to_owned(),
        external_requirement: requirement,
    }
}

pub(super) fn record() -> renderpilot_domain::InstalledAddon {
    renderpilot_domain::InstalledAddon::new(
        game_id(),
        AddonKind::Luma,
        path("C:/Games/Snapshot/Luma.addon64"),
    )
}

pub(super) fn owned_receipt(identity: &str, byte: char) -> FileReceipt {
    FileReceipt::owned(
        identity,
        Sha256Hash::new(byte.to_string().repeat(64)).expect("digest"),
    )
    .expect("receipt")
}

pub(super) fn reused_receipt(identity: &str, byte: char) -> FileReceipt {
    FileReceipt::reused(
        identity,
        Sha256Hash::new(byte.to_string().repeat(64)).expect("digest"),
    )
    .expect("receipt")
}
