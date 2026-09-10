use renderpilot_domain::{
    GameProxyTopology, InstalledAddon, ManagedAddonFile, PathRef, PlannedGameProxyTopology,
    TrackedSource, Version,
};

use crate::addons::luma::{
    dgvoodoo::PreparedDgVoodoo, fetch::types::LumaPayload,
    peer::root_authority::LumaPeerRootAuthority,
};
use crate::addons::reshade::host_policy::TopologyHostAssessment;
use crate::catalog::cascade::CascadeResult;
use crate::coordinated_files::CatalogPathClaim;
use crate::peer_mutation_executor::{ExactEndpointProgram, PeerPathSnapshot};

/// The payload preparation supplied to an active Luma update.
#[derive(Debug)]
pub(crate) enum LumaActiveUpdatePayloadInput {
    /// Keep the recorded payload projection unchanged.
    Preserve,
    /// Replace the recorded payload projection with this verified release.
    Full(LumaPayload),
}

/// The ReShade downstream preparation supplied to an active Luma update.
#[derive(Debug)]
pub(crate) enum LumaActiveUpdateHostInput {
    /// Keep the existing downstream binding unchanged.
    Preserve,
    /// Use these already-prepared ReShade bytes when an owned replacement is valid.
    Replace { bytes: Vec<u8> },
}

/// The dgVoodoo preparation supplied to an active Luma update.
#[derive(Debug)]
pub(crate) enum LumaActiveUpdateDgVoodooInput {
    /// Keep the recorded dgVoodoo projection unchanged.
    Preserve,
    /// Replace or repair the owned dgVoodoo projection with this archive.
    Replace(Box<PreparedDgVoodoo>),
    /// Remove the recorded owned dgVoodoo projection.
    Remove,
}

/// Immutable, already-prepared artifacts for an active Luma update.
#[derive(Debug)]
pub(crate) struct LumaActiveUpdatePrepared {
    payload: LumaActiveUpdatePayloadInput,
    host: LumaActiveUpdateHostInput,
    dgvoodoo: LumaActiveUpdateDgVoodooInput,
    dependency_paths: Vec<PathRef>,
    tracked_sources: Vec<TrackedSource>,
    addon_version: Option<String>,
}

impl LumaActiveUpdatePrepared {
    /// Creates a prepared active-update input, taking ownership of all artifacts.
    pub(crate) fn new(
        payload: LumaActiveUpdatePayloadInput,
        host: LumaActiveUpdateHostInput,
        dgvoodoo: LumaActiveUpdateDgVoodooInput,
        dependency_paths: Vec<PathRef>,
        tracked_sources: Vec<TrackedSource>,
        addon_version: Option<String>,
    ) -> Self {
        Self {
            payload,
            host,
            dgvoodoo,
            dependency_paths,
            tracked_sources,
            addon_version,
        }
    }

    /// Returns the prepared payload decision.
    pub(crate) fn payload(&self) -> &LumaActiveUpdatePayloadInput {
        &self.payload
    }

    /// Decomposes the prepared input without cloning owned artifacts.
    pub(crate) fn into_parts(
        self,
    ) -> (
        LumaActiveUpdatePayloadInput,
        LumaActiveUpdateHostInput,
        LumaActiveUpdateDgVoodooInput,
        Vec<PathRef>,
        Vec<TrackedSource>,
        Option<String>,
    ) {
        (
            self.payload,
            self.host,
            self.dgvoodoo,
            self.dependency_paths,
            self.tracked_sources,
            self.addon_version,
        )
    }
}

/// Exact phase-3 evidence for the active topology's ReShade downstream.
#[derive(Debug, Clone, Copy)]
pub(crate) struct LumaActiveUpdateHostObservation<'a> {
    assessment: &'a TopologyHostAssessment,
    path: &'a PathRef,
    snapshot: &'a PeerPathSnapshot,
}

impl<'a> LumaActiveUpdateHostObservation<'a> {
    /// Creates a borrowed host-evidence bundle.
    pub(crate) fn new(
        assessment: &'a TopologyHostAssessment,
        path: &'a PathRef,
        snapshot: &'a PeerPathSnapshot,
    ) -> Self {
        Self {
            assessment,
            path,
            snapshot,
        }
    }

    /// Decomposes the borrowed host evidence.
    pub(crate) fn into_parts(
        self,
    ) -> (
        &'a TopologyHostAssessment,
        &'a PathRef,
        &'a PeerPathSnapshot,
    ) {
        (self.assessment, self.path, self.snapshot)
    }
}

/// Complete immutable input to an active Luma update composer.
#[derive(Debug)]
pub(crate) struct LumaActiveUpdateInput<'a> {
    evidence: LumaActiveUpdateEvidence<'a>,
    prepared: LumaActiveUpdatePrepared,
}

/// Immutable phase-three facts whose consistency must be preserved through
/// active-update composition. Prepared artifacts are held separately so they
/// can be consumed exactly once after this evidence is accepted.
#[derive(Debug)]
pub(crate) struct LumaActiveUpdateEvidence<'a> {
    before_record: &'a InstalledAddon,
    topology: &'a GameProxyTopology,
    authority: &'a LumaPeerRootAuthority,
    host_observation: LumaActiveUpdateHostObservation<'a>,
    minimum_host_version: &'a Version,
    catalog_claim: &'a CatalogPathClaim,
    cascade: &'a CascadeResult,
}

impl<'a> LumaActiveUpdateEvidence<'a> {
    /// Creates the complete borrowed evidence set for an active update.
    pub(crate) fn new(
        before_record: &'a InstalledAddon,
        topology: &'a GameProxyTopology,
        authority: &'a LumaPeerRootAuthority,
        host_observation: LumaActiveUpdateHostObservation<'a>,
        minimum_host_version: &'a Version,
        catalog_claim: &'a CatalogPathClaim,
        cascade: &'a CascadeResult,
    ) -> Self {
        Self {
            before_record,
            topology,
            authority,
            host_observation,
            minimum_host_version,
            catalog_claim,
            cascade,
        }
    }
}

impl<'a> LumaActiveUpdateInput<'a> {
    /// Creates a complete active-update input without resolving mutable authority.
    pub(crate) fn new(
        evidence: LumaActiveUpdateEvidence<'a>,
        prepared: LumaActiveUpdatePrepared,
    ) -> Self {
        Self { evidence, prepared }
    }

    /// Decomposes the complete input, transferring the prepared artifacts.
    pub(crate) fn into_parts(
        self,
    ) -> (
        &'a InstalledAddon,
        &'a GameProxyTopology,
        &'a LumaPeerRootAuthority,
        LumaActiveUpdateHostObservation<'a>,
        &'a Version,
        &'a CatalogPathClaim,
        &'a CascadeResult,
        LumaActiveUpdatePrepared,
    ) {
        let Self { evidence, prepared } = self;
        let LumaActiveUpdateEvidence {
            before_record,
            topology,
            authority,
            host_observation,
            minimum_host_version,
            catalog_claim,
            cascade,
        } = evidence;
        (
            before_record,
            topology,
            authority,
            host_observation,
            minimum_host_version,
            catalog_claim,
            cascade,
            prepared,
        )
    }
}

/// Set-delta for generic created and backed-up record claims.
#[derive(Debug, Default)]
pub(super) struct LumaActiveUpdateClaimDelta {
    add_created: Vec<PathRef>,
    remove_created: Vec<PathRef>,
    add_backed_up: Vec<PathRef>,
    remove_backed_up: Vec<PathRef>,
}

impl LumaActiveUpdateClaimDelta {
    pub(super) fn new(
        add_created: Vec<PathRef>,
        remove_created: Vec<PathRef>,
        add_backed_up: Vec<PathRef>,
        remove_backed_up: Vec<PathRef>,
    ) -> Self {
        Self {
            add_created,
            remove_created,
            add_backed_up,
            remove_backed_up,
        }
    }

    pub(super) fn into_parts(self) -> (Vec<PathRef>, Vec<PathRef>, Vec<PathRef>, Vec<PathRef>) {
        (
            self.add_created,
            self.remove_created,
            self.add_backed_up,
            self.remove_backed_up,
        )
    }
}

/// The DLSS input selected for a full payload projection.
#[derive(Debug)]
pub(super) enum LumaActiveUpdateDlssInput {
    Preserve,
    Full { bundled_bytes: Option<Vec<u8>> },
}

/// Generic payload changes plus the new main add-on anchor.
#[derive(Debug)]
pub(super) struct PayloadRecordProjection {
    addon_file: PathRef,
    claims: LumaActiveUpdateClaimDelta,
}

impl PayloadRecordProjection {
    pub(super) fn new(addon_file: PathRef, claims: LumaActiveUpdateClaimDelta) -> Self {
        Self { addon_file, claims }
    }

    pub(super) fn into_parts(self) -> (PathRef, LumaActiveUpdateClaimDelta) {
        (self.addon_file, self.claims)
    }
}

/// Complete payload-side record projection.
#[derive(Debug)]
pub(super) struct PayloadProjection {
    record: PayloadRecordProjection,
    dlss: LumaActiveUpdateDlssInput,
    mtime: Option<LumaActiveUpdateMtime>,
}

impl PayloadProjection {
    pub(super) fn new(
        record: PayloadRecordProjection,
        dlss: LumaActiveUpdateDlssInput,
        mtime: Option<LumaActiveUpdateMtime>,
    ) -> Self {
        Self {
            record,
            dlss,
            mtime,
        }
    }

    pub(super) fn into_parts(
        self,
    ) -> (
        PayloadRecordProjection,
        LumaActiveUpdateDlssInput,
        Option<LumaActiveUpdateMtime>,
    ) {
        (self.record, self.dlss, self.mtime)
    }
}

/// Active host binding and planned topology after the update.
#[derive(Debug)]
pub(super) struct HostProjection {
    binding: ManagedAddonFile,
    planned_topology: PlannedGameProxyTopology,
}

impl HostProjection {
    pub(super) fn new(
        binding: ManagedAddonFile,
        planned_topology: PlannedGameProxyTopology,
    ) -> Self {
        Self {
            binding,
            planned_topology,
        }
    }

    pub(super) fn into_parts(self) -> (ManagedAddonFile, PlannedGameProxyTopology) {
        (self.binding, self.planned_topology)
    }
}

/// dgVoodoo generic-claim delta.
#[derive(Debug)]
pub(super) struct DgVoodooProjection {
    claims: LumaActiveUpdateClaimDelta,
}

impl DgVoodooProjection {
    pub(super) fn new(claims: LumaActiveUpdateClaimDelta) -> Self {
        Self { claims }
    }

    pub(super) fn into_claims(self) -> LumaActiveUpdateClaimDelta {
        self.claims
    }
}

/// DLSS managed binding after a full payload projection.
#[derive(Debug)]
pub(super) struct DlssProjection {
    binding: Option<ManagedAddonFile>,
}

impl DlssProjection {
    pub(super) fn new(binding: Option<ManagedAddonFile>) -> Self {
        Self { binding }
    }

    pub(super) fn into_binding(self) -> Option<ManagedAddonFile> {
        self.binding
    }
}

/// Main add-on mtime/provenance selected for the update record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LumaActiveUpdateMtime {
    path: PathRef,
    last_modified: Option<String>,
}

impl LumaActiveUpdateMtime {
    pub(super) fn new(path: PathRef, last_modified: Option<String>) -> Self {
        Self {
            path,
            last_modified,
        }
    }

    #[cfg(test)]
    pub(crate) fn path(&self) -> &PathRef {
        &self.path
    }

    #[cfg(test)]
    pub(crate) fn last_modified(&self) -> Option<&str> {
        self.last_modified.as_deref()
    }

    pub(crate) fn into_parts(self) -> (PathRef, Option<String>) {
        (self.path, self.last_modified)
    }
}

/// Complete physical active-update projection for the later peer package.
#[derive(Debug)]
pub(crate) struct LumaActiveUpdatePhysical<'a> {
    after_record: InstalledAddon,
    program: ExactEndpointProgram,
    payloads: Vec<Option<Vec<u8>>>,
    planned_topology: PlannedGameProxyTopology,
    cascade: &'a CascadeResult,
    mtime: Option<LumaActiveUpdateMtime>,
}

/// Owned physical commit data in endpoint order. The program and payloads are
/// kept paired, while the topology, cascade evidence, and mtime are consumed
/// by the exact later commit ceremony.
pub(crate) type LumaActiveUpdatePhysicalParts<'a> = (
    InstalledAddon,
    ExactEndpointProgram,
    Vec<Option<Vec<u8>>>,
    PlannedGameProxyTopology,
    &'a CascadeResult,
    Option<(PathRef, Option<String>)>,
);

impl<'a> LumaActiveUpdatePhysical<'a> {
    pub(super) fn new(
        after_record: InstalledAddon,
        program: ExactEndpointProgram,
        payloads: Vec<Option<Vec<u8>>>,
        planned_topology: PlannedGameProxyTopology,
        cascade: &'a CascadeResult,
        mtime: Option<LumaActiveUpdateMtime>,
    ) -> Self {
        Self {
            after_record,
            program,
            payloads,
            planned_topology,
            cascade,
            mtime,
        }
    }

    #[cfg(test)]
    pub(crate) fn after_record(&self) -> &InstalledAddon {
        &self.after_record
    }

    #[cfg(test)]
    pub(crate) fn program(&self) -> &ExactEndpointProgram {
        &self.program
    }

    #[cfg(test)]
    pub(crate) fn payloads(&self) -> &[Option<Vec<u8>>] {
        &self.payloads
    }

    #[cfg(test)]
    pub(crate) fn planned_topology(&self) -> &PlannedGameProxyTopology {
        &self.planned_topology
    }

    pub(crate) fn into_parts(self) -> LumaActiveUpdatePhysicalParts<'a> {
        (
            self.after_record,
            self.program,
            self.payloads,
            self.planned_topology,
            self.cascade,
            self.mtime.map(LumaActiveUpdateMtime::into_parts),
        )
    }
}

/// Aggregate-only active update result.
#[derive(Debug)]
pub(crate) struct LumaActiveUpdateAggregateMembership {
    after_record: InstalledAddon,
}

impl LumaActiveUpdateAggregateMembership {
    pub(super) fn new(after_record: InstalledAddon) -> Self {
        Self { after_record }
    }

    pub(crate) fn into_record(self) -> InstalledAddon {
        self.after_record
    }
}

/// Metadata-only active update result.
#[derive(Debug)]
pub(crate) struct LumaActiveUpdateMetadata {
    after_record: InstalledAddon,
}

impl LumaActiveUpdateMetadata {
    pub(super) fn new(after_record: InstalledAddon) -> Self {
        Self { after_record }
    }

    pub(crate) fn into_record(self) -> InstalledAddon {
        self.after_record
    }
}

/// Result of composing one active Luma update.
#[derive(Debug)]
pub(crate) enum LumaActiveUpdateComposition<'a> {
    Physical(Box<LumaActiveUpdatePhysical<'a>>),
    AggregateMembership(LumaActiveUpdateAggregateMembership),
    Metadata(LumaActiveUpdateMetadata),
    Noop,
}
