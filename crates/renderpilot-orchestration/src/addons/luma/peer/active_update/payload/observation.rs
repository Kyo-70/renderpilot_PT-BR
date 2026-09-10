use renderpilot_domain::PathRef;

use crate::peer_mutation_executor::{PeerPathSnapshot, observe_peer_path_snapshot};

use crate::addons::luma::peer::{
    active_update::error::LumaActiveUpdateError, root_authority::LumaPeerRootAuthority,
};

use super::{
    model::{Candidate, ObservedCandidate},
    validation::effective_root,
};

pub(super) fn observe_candidates(
    authority: &LumaPeerRootAuthority,
    candidates: &[Candidate],
) -> Result<Vec<ObservedCandidate>, LumaActiveUpdateError> {
    let root = effective_root(authority);
    candidates
        .iter()
        .map(|candidate| {
            let live = observe(candidate.live(), root)?;
            let sidecar = observe(candidate.sidecar(), root)?;
            Ok(ObservedCandidate { live, sidecar })
        })
        .collect()
}

fn observe(path: &PathRef, root: &PathRef) -> Result<PeerPathSnapshot, LumaActiveUpdateError> {
    observe_peer_path_snapshot(path, root)
        .map_err(|error| LumaActiveUpdateError::observation(path.clone(), error))
}
