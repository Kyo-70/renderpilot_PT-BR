use std::path::PathBuf;

use renderpilot_domain::{GameProxyTopology, PathRef};

use crate::addons::luma::matcher::ResolvedLumaInstall;
use crate::addons::luma::types::LumaExternalRequirement;
use crate::peer_mutation_executor::PeerPathSnapshot;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DgVoodooPrepKind {
    None,
    Managed,
    Reused,
}

#[derive(Debug)]
pub(super) struct ActiveInstallSnapshot {
    pub(super) game_root: PathBuf,
    pub(super) target_dir: PathBuf,
    pub(super) asset: String,
    pub(super) addon_file: String,
    pub(super) arch: renderpilot_domain::Architecture,
    pub(super) proxy_dll_name: String,
    pub(super) external_requirement: Option<LumaExternalRequirement>,
    pub(super) writes_host: bool,
    pub(super) dgvoodoo_kind: DgVoodooPrepKind,
    pub(super) topology: GameProxyTopology,
    pub(super) host_path: PathRef,
    pub(super) host_snapshot: PeerPathSnapshot,
}

pub(super) struct ActiveInstallResolution {
    pub(super) snapshot: ActiveInstallSnapshot,
    pub(super) plan: ResolvedLumaInstall,
}
