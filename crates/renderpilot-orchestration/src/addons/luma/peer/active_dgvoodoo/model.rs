use renderpilot_domain::{PathRef, TrackedSource};

use crate::{addons::engine::IniSection, peer_mutation_executor::PeerPathSnapshot};

#[derive(Debug)]
pub(crate) struct ActiveDgVoodooPlan {
    pub(super) targets: Vec<ActiveDgVoodooTarget>,
    pub(super) observation_paths: Vec<PathRef>,
    pub(super) tracked_source: Option<TrackedSource>,
}

impl ActiveDgVoodooPlan {
    pub(crate) fn observation_paths(&self) -> &[PathRef] {
        &self.observation_paths
    }

    /// Transfers the validated target set to an active-update projection.
    ///
    /// The target implementation remains private to the planner; this owned
    /// view is the narrow hand-off needed by the phase-3 update slice.
    pub(crate) fn into_target_views(self) -> Vec<ActiveDgVoodooTargetView> {
        self.targets
            .into_iter()
            .map(|target| ActiveDgVoodooTargetView {
                live: target.live,
                sidecar: target.sidecar,
                payload: match target.kind {
                    ActiveDgVoodooTargetPayload::Runtime { bytes } => {
                        ActiveDgVoodooTargetViewPayload::Runtime { bytes }
                    }
                    ActiveDgVoodooTargetPayload::Config { default, sections } => {
                        ActiveDgVoodooTargetViewPayload::Config { default, sections }
                    }
                },
            })
            .collect()
    }
}

/// Owned, crate-visible projection of one planner-validated dgVoodoo target.
#[derive(Debug)]
pub(crate) struct ActiveDgVoodooTargetView {
    live: PathRef,
    sidecar: PathRef,
    payload: ActiveDgVoodooTargetViewPayload,
}

impl ActiveDgVoodooTargetView {
    pub(crate) fn live(&self) -> &PathRef {
        &self.live
    }

    pub(crate) fn sidecar(&self) -> &PathRef {
        &self.sidecar
    }

    pub(crate) fn payload(&self) -> &ActiveDgVoodooTargetViewPayload {
        &self.payload
    }
}

/// Payload projection paired with a validated dgVoodoo target.
#[derive(Debug)]
pub(crate) enum ActiveDgVoodooTargetViewPayload {
    Runtime {
        bytes: Vec<u8>,
    },
    Config {
        default: Vec<u8>,
        sections: Vec<IniSection>,
    },
}

#[derive(Debug)]
pub(super) struct ActiveDgVoodooTarget {
    pub(super) live: PathRef,
    pub(super) sidecar: PathRef,
    pub(super) kind: ActiveDgVoodooTargetPayload,
}

#[derive(Debug)]
pub(super) enum ActiveDgVoodooTargetPayload {
    Runtime {
        bytes: Vec<u8>,
    },
    Config {
        default: Vec<u8>,
        sections: Vec<IniSection>,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct ActiveDgVoodooSnapshot<'a> {
    pub(super) path: PathRef,
    pub(super) image: &'a PeerPathSnapshot,
}

impl<'a> ActiveDgVoodooSnapshot<'a> {
    pub(crate) fn new(path: &PathRef, image: &'a PeerPathSnapshot) -> Self {
        Self {
            path: path.clone(),
            image,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct ActiveDgVoodooRecordProjection {
    pub(super) created_files: Vec<PathRef>,
    pub(super) backed_up_files: Vec<PathRef>,
    pub(super) tracked_source: Option<TrackedSource>,
}

impl ActiveDgVoodooRecordProjection {
    pub(crate) fn created_files(&self) -> &[PathRef] {
        &self.created_files
    }

    pub(crate) fn backed_up_files(&self) -> &[PathRef] {
        &self.backed_up_files
    }

    pub(crate) fn tracked_source(&self) -> Option<&TrackedSource> {
        self.tracked_source.as_ref()
    }
}
