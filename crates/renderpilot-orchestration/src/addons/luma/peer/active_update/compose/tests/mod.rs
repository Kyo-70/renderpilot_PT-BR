mod effects;
mod endpoint_free;
mod physical;

use std::path::Path;

use renderpilot_domain::{
    AddonKind, FileReceipt, GameId, GameProxyTopology, InstalledAddon, ManagedAddonFile, PathRef,
    ProxyImplementation, ProxyLink, ProxyRootPrestate, Sha256Hash,
};

pub(super) fn path(root: &Path, relative: &str) -> PathRef {
    PathRef::new(root.join(relative).to_string_lossy().into_owned()).expect("path")
}

pub(super) fn hash(value: char) -> Sha256Hash {
    Sha256Hash::new(format!("{:x}", (value as u8) % 16).repeat(64)).expect("hash")
}

pub(super) fn game() -> GameId {
    GameId::new("manual:luma-active-update-compose").expect("game")
}

pub(super) fn topology(root: &Path, game_id: &GameId) -> GameProxyTopology {
    let root_slot = path(root, "dxgi.dll");
    let downstream = path(root, "ReShade64.dll");
    GameProxyTopology {
        id: "optiscaler:luma-active-update-compose".to_owned(),
        game_id: game_id.clone(),
        root_slot: root_slot.clone(),
        outer: ProxyLink {
            implementation: ProxyImplementation::OptiScaler,
            path: root_slot.clone(),
            receipt: FileReceipt::owned("outer", hash('a')).expect("outer receipt"),
        },
        downstream: Some(ProxyLink {
            implementation: ProxyImplementation::ReShade,
            path: downstream,
            receipt: FileReceipt::reused("reshade", hash('b')).expect("downstream receipt"),
        }),
        downstream_origin: Some(root_slot),
        root_prestate: ProxyRootPrestate::Absent,
    }
}

pub(super) fn record(
    root: &Path,
    game_id: &GameId,
    managed: Vec<ManagedAddonFile>,
) -> InstalledAddon {
    InstalledAddon::new(game_id.clone(), AddonKind::Luma, path(root, "Luma.addon64"))
        .try_with_managed_files(managed)
        .expect("record")
}
