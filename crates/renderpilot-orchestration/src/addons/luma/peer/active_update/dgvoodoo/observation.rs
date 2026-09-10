use renderpilot_domain::PathRef;

use crate::{
    addons::luma::peer::{
        active_update::error::LumaActiveUpdateError, root_authority::LumaPeerRootAuthority,
    },
    peer_mutation_executor::{PeerPathSnapshot, observe_peer_path_snapshot},
};

use super::model::{Candidate, ObservedCandidate};

pub(super) fn observe_candidates(
    authority: &LumaPeerRootAuthority,
    candidates: &[Candidate],
) -> Result<Vec<ObservedCandidate>, LumaActiveUpdateError> {
    candidates
        .iter()
        .map(|candidate| {
            let live = observe(authority, &candidate.live)?;
            let sidecar = observe(authority, &candidate.sidecar)?;
            Ok(ObservedCandidate { live, sidecar })
        })
        .collect()
}

fn observe(
    authority: &LumaPeerRootAuthority,
    path: &PathRef,
) -> Result<PeerPathSnapshot, LumaActiveUpdateError> {
    observe_peer_path_snapshot(path, authority.canonical_game_root_ref())
        .map_err(|error| LumaActiveUpdateError::observation(path.clone(), error))
}
