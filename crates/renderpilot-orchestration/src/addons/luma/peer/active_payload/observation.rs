use renderpilot_domain::PathRef;

use crate::peer_mutation_executor::{PeerPathSnapshot, observe_peer_path_snapshot};

use super::model::{ActivePayloadError, ActivePayloadObservationError};

pub(super) fn observe(
    path: &PathRef,
    root: &PathRef,
) -> Result<PeerPathSnapshot, ActivePayloadError> {
    observe_peer_path_snapshot(path, root).map_err(|error| {
        ActivePayloadError::Observation(ActivePayloadObservationError {
            path: path.clone(),
            error,
        })
    })
}
