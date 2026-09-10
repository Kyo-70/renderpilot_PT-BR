use renderpilot_domain::PathRef;

use crate::addons::engine::IniSection;
use crate::peer_mutation_executor::PeerPathSnapshot;

#[derive(Debug)]
pub(super) enum CandidatePayload {
    Runtime {
        bytes: Vec<u8>,
    },
    Config {
        default: Vec<u8>,
        sections: Vec<IniSection>,
    },
}

#[derive(Debug)]
pub(super) enum CandidateKind {
    Desired(CandidatePayload),
    Removed,
}

#[derive(Debug)]
pub(super) struct Candidate {
    pub(super) live: PathRef,
    pub(super) sidecar: PathRef,
    pub(super) kind: CandidateKind,
    pub(super) created: bool,
    pub(super) backed: bool,
}

impl Candidate {
    pub(super) fn is_removed(&self) -> bool {
        matches!(self.kind, CandidateKind::Removed)
    }

    pub(super) fn key(&self) -> String {
        renderpilot_domain::normalized_path_key(self.live.as_str())
    }
}

#[derive(Debug)]
pub(super) struct ObservedCandidate {
    pub(super) live: PeerPathSnapshot,
    pub(super) sidecar: PeerPathSnapshot,
}

#[derive(Debug)]
pub(super) enum CandidateAction {
    Unchanged,
    Create(Vec<u8>),
    Replace(Vec<u8>),
    RemoveCreated,
    ReleaseBacked,
}

#[derive(Debug)]
pub(super) struct ClassifiedCandidate {
    pub(super) index: usize,
    pub(super) action: CandidateAction,
}
