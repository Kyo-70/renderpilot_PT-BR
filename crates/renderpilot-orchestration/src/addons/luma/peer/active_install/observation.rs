use renderpilot_domain::PathRef;

use crate::peer_mutation_executor::{PeerPathSnapshot, observe_peer_path_snapshot};

use super::super::root_authority::LumaPeerRootAuthority;
use super::error::LumaActiveInstallCompositionError;

pub(super) fn effective_addon_root(authority: &LumaPeerRootAuthority) -> &PathRef {
    authority.effective_addon_root_ref()
}

pub(super) fn dlss_path(
    authority: &LumaPeerRootAuthority,
) -> Result<PathRef, LumaActiveInstallCompositionError> {
    authority
        .effective_dlss_target()
        .map_err(LumaActiveInstallCompositionError::Authority)
}

pub(super) fn observe(
    path: &PathRef,
    root: &PathRef,
) -> Result<PeerPathSnapshot, LumaActiveInstallCompositionError> {
    observe_peer_path_snapshot(path, root).map_err(|error| {
        LumaActiveInstallCompositionError::Observation {
            path: path.clone(),
            error,
        }
    })
}
