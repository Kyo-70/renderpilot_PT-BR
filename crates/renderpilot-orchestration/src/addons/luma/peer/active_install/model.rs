use renderpilot_domain::{
    GameProxyTopology, InstalledAddon, PathRef, PlannedGameProxyTopology, Version,
};

use crate::{
    addons::{luma::install::PreparedInstall, reshade::host_policy::TopologyHostAssessment},
    coordinated_files::CatalogPathClaim,
    peer_mutation_executor::{ExactEndpointProgram, PeerPathSnapshot},
};

use super::super::root_authority::LumaPeerRootAuthority;

/// Complete sealed evidence consumed by the active-install composer. Keeping
/// the prepared release with its topology, host image, catalog claim, and
/// authority proof prevents the composition boundary from accepting a
/// partially assembled install request.
pub(crate) struct LumaActiveInstallInput<'a> {
    pub(crate) prepared: PreparedInstall,
    pub(crate) topology: &'a GameProxyTopology,
    pub(crate) authority: &'a LumaPeerRootAuthority,
    pub(crate) assessment: &'a TopologyHostAssessment,
    pub(crate) host_path: &'a PathRef,
    pub(crate) host_snapshot: &'a PeerPathSnapshot,
    pub(crate) catalog_claim: &'a CatalogPathClaim,
    pub(crate) minimum_host_version: &'a Version,
}

/// Complete active-install projection. The program and payloads stay aligned
/// by endpoint ordinal and are consumed by the later peer package boundary.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct LumaActiveInstallComposition {
    record: InstalledAddon,
    program: ExactEndpointProgram,
    payloads: Vec<Option<Vec<u8>>>,
    planned_topology: PlannedGameProxyTopology,
}

impl LumaActiveInstallComposition {
    pub(crate) fn new(
        record: InstalledAddon,
        program: ExactEndpointProgram,
        payloads: Vec<Option<Vec<u8>>>,
        planned_topology: PlannedGameProxyTopology,
    ) -> Self {
        Self {
            record,
            program,
            payloads,
            planned_topology,
        }
    }

    pub(crate) fn record(&self) -> &InstalledAddon {
        &self.record
    }

    #[cfg(test)]
    pub(crate) fn program(&self) -> &ExactEndpointProgram {
        &self.program
    }

    #[cfg(test)]
    pub(crate) fn payloads(&self) -> &[Option<Vec<u8>>] {
        &self.payloads
    }

    pub(crate) fn into_parts(
        self,
    ) -> (
        InstalledAddon,
        ExactEndpointProgram,
        Vec<Option<Vec<u8>>>,
        PlannedGameProxyTopology,
    ) {
        (
            self.record,
            self.program,
            self.payloads,
            self.planned_topology,
        )
    }
}
