//! Closed runtime and move-only permit data.

use std::sync::Arc;

use renderpilot_domain::{
    ExactOptiConfigProjection, GameId, GameProxyTopology, InstalledAddon, LibraryComponent,
    OptiScalerInstallState, PeerCatalogRollbackClaim, PeerReadGuardEvidence,
    PeerTransitionContract, PlannedGameProxyTopology, ProxyPeerRoute, RenoDxReshadeIniAuthority,
};

use super::super::catalog::OwnedCatalogProjection;
use super::super::manifest::ParsedPeerProgram;
use super::super::read_guards::ReadGuardPermitState;
use super::super::shared_roots::BoundSharedPeerRoots;
use crate::repositories::{ComponentBaselineMutation, SqliteStorage};

/// Borrowed input for a game-file peer preparation.
#[derive(Debug, Clone, Copy)]
pub struct PeerCommitPreparation<'a> {
    /// Durable pending mutation id.
    pub mutation_id: &'a str,
    /// Expected game owner.
    pub game_id: &'a GameId,
    /// Persisted feature owner.
    pub feature: &'a str,
    /// Expected subject owner.
    pub subject_id: Option<&'a str>,
    /// Final manifest containing the strict peer program.
    pub manifest_json: &'a str,
    /// Process-local canonical game root; never persisted.
    pub canonical_game_root: &'a str,
    /// Ordered initial observations for the storage-derived read guards.
    pub initial_read_guards: &'a [PeerReadGuardEvidence],
    /// Exact peer before image.
    pub before_peer: Option<&'a InstalledAddon>,
    /// Exact peer after image.
    pub after_peer: Option<&'a InstalledAddon>,
    /// Exact topology before image.
    pub before_topology: Option<&'a GameProxyTopology>,
    /// Planned topology after image.
    pub planned_after_topology: Option<&'a PlannedGameProxyTopology>,
    /// Closed domain route.
    pub route: ProxyPeerRoute,
    /// Optional catalog component projection.
    pub component_set: Option<&'a [LibraryComponent]>,
    /// Catalog rollback-baseline changes coupled to this commit.
    pub baseline_mutations: &'a [ComponentBaselineMutation<'a>],
    /// Optional closed catalog rollback claim. Storage rederives and owns it
    /// before the pending row is allowed to become Prepared.
    pub catalog_claim: Option<&'a PeerCatalogRollbackClaim>,
    /// Optional storage-bound RenoDX ReShade.ini authority.
    pub renodx_reshade_ini: Option<&'a RenoDxReshadeIniAuthority>,
}

/// Borrowed input for a game-scoped shared-Vulkan peer preparation.
#[derive(Debug, Clone, Copy)]
pub struct SharedPeerCommitPreparation<'a> {
    /// Durable shared mutation id.
    pub mutation_id: &'a str,
    /// Persisted feature owner.
    pub feature: &'a str,
    /// Exact game owner.
    pub game_id: &'a GameId,
    /// Final manifest containing the strict peer program.
    pub manifest_json: &'a str,
    /// Exact peer before image.
    pub before_peer: Option<&'a InstalledAddon>,
    /// Exact peer after image.
    pub after_peer: Option<&'a InstalledAddon>,
    /// Exact topology before image.
    pub before_topology: Option<&'a GameProxyTopology>,
    /// Planned topology after image.
    pub planned_after_topology: Option<&'a PlannedGameProxyTopology>,
    /// Closed domain route.
    pub route: ProxyPeerRoute,
    /// Optional storage-bound RenoDX ReShade.ini authority.
    pub renodx_reshade_ini: Option<&'a RenoDxReshadeIniAuthority>,
}

/// Whole storage owner for peer mutation commits.
#[derive(Debug)]
pub struct PeerStorageRuntime {
    pub(super) storage: SqliteStorage,
    pub(super) instance: Arc<RuntimeInstance>,
}

#[derive(Debug)]
pub(crate) struct RuntimeInstance;

/// Move-only proof for one exact Prepared row.
#[derive(Debug)]
pub struct PreparedPeerCommitPermit {
    pub(super) instance: Arc<RuntimeInstance>,
    pub(super) binding: PreparedRowBinding,
    pub(super) fingerprint: RowFingerprint,
    pub(super) mutation_id: String,
    pub(super) game_id: GameId,
    pub(super) feature: String,
    pub(super) subject_id: Option<String>,
    pub(super) program: ParsedPeerProgram,
    pub(super) contract: PeerTransitionContract,
    pub(super) before_peer: Option<InstalledAddon>,
    pub(super) after_peer: Option<InstalledAddon>,
    pub(super) before_topology: Option<GameProxyTopology>,
    pub(super) planned_after_topology: Option<PlannedGameProxyTopology>,
    pub(super) route: ProxyPeerRoute,
    pub(super) catalog: OwnedCatalogProjection,
    pub(super) optiscaler_config: Option<RenoDxOptiScalerConfigPeerCommit>,
    pub(super) read_guard_state: ReadGuardPermitState,
}

/// Storage-owned state companion for the sole RenoDX proxy route that may
/// change OptiScaler.ini. It remains inside the opaque permit so callers
/// cannot apply its state update independently of the sealed peer commit.
#[derive(Debug, Clone)]
pub(crate) struct RenoDxOptiScalerConfigPeerCommit {
    pub(super) before_state: OptiScalerInstallState,
    pub(super) projection: ExactOptiConfigProjection,
}

impl RenoDxOptiScalerConfigPeerCommit {
    pub(crate) fn new(
        before_state: OptiScalerInstallState,
        projection: ExactOptiConfigProjection,
    ) -> Self {
        Self {
            before_state,
            projection,
        }
    }

    pub(crate) fn before_state(&self) -> &OptiScalerInstallState {
        &self.before_state
    }

    pub(crate) fn projection(&self) -> &ExactOptiConfigProjection {
        &self.projection
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PreparedRowBinding {
    File,
    Shared,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RowFingerprint {
    pub(crate) rowid: i64,
    pub(crate) created_at: i64,
    pub(crate) updated_at: i64,
    pub(crate) manifest_sha256: [u8; 32],
    pub(crate) root_capabilities_sha256: Option<[u8; 32]>,
}

impl PeerStorageRuntime {
    /// Takes ownership of the complete SQLite storage handle.
    #[must_use]
    pub fn new(storage: SqliteStorage) -> Self {
        Self {
            storage,
            instance: Arc::new(RuntimeInstance),
        }
    }

    /// Returns only the repository delegation surface; peer commit methods are
    /// intentionally not exposed by the underlying storage handle.
    #[must_use]
    pub fn repositories(&self) -> &SqliteStorage {
        &self.storage
    }

    /// Returns the process-local identity that mints opaque permits.
    pub(crate) fn runtime_instance(&self) -> Arc<RuntimeInstance> {
        Arc::clone(&self.instance)
    }
}

impl PreparedPeerCommitPermit {
    pub(crate) fn fingerprint(&self) -> RowFingerprint {
        self.fingerprint
    }
    pub(crate) fn mutation_id(&self) -> &str {
        &self.mutation_id
    }
    pub(crate) fn game_id(&self) -> &GameId {
        &self.game_id
    }
    pub(crate) fn feature(&self) -> &str {
        &self.feature
    }
    pub(crate) fn subject_id(&self) -> Option<&str> {
        self.subject_id.as_deref()
    }
    pub(crate) fn program(&self) -> &ParsedPeerProgram {
        &self.program
    }
    pub(crate) fn contract(&self) -> &PeerTransitionContract {
        &self.contract
    }
    pub(crate) fn planned_after_topology(&self) -> Option<&PlannedGameProxyTopology> {
        self.planned_after_topology.as_ref()
    }
}

#[derive(Debug)]
pub(crate) struct PermitValues {
    pub(super) mutation_id: String,
    pub(super) game_id: GameId,
    pub(super) feature: String,
    pub(super) subject_id: Option<String>,
    pub(super) manifest_json: String,
    pub(super) program: ParsedPeerProgram,
    pub(super) contract: PeerTransitionContract,
    pub(super) before_peer: Option<InstalledAddon>,
    pub(super) after_peer: Option<InstalledAddon>,
    pub(super) before_topology: Option<GameProxyTopology>,
    pub(super) planned_after_topology: Option<PlannedGameProxyTopology>,
    pub(super) route: ProxyPeerRoute,
    pub(super) catalog: OwnedCatalogProjection,
    pub(super) optiscaler_config: Option<RenoDxOptiScalerConfigPeerCommit>,
    pub(super) read_guard_state: ReadGuardPermitState,
}

impl PermitValues {
    pub(super) fn from_file(
        preparation: PeerCommitPreparation<'_>,
        program: ParsedPeerProgram,
        contract: PeerTransitionContract,
        read_guards: super::super::read_guards::ReadGuardProjection,
        catalog: OwnedCatalogProjection,
        optiscaler_config: Option<RenoDxOptiScalerConfigPeerCommit>,
    ) -> Self {
        Self {
            mutation_id: preparation.mutation_id.to_owned(),
            game_id: preparation.game_id.clone(),
            feature: preparation.feature.to_owned(),
            subject_id: preparation.subject_id.map(str::to_owned),
            manifest_json: preparation.manifest_json.to_owned(),
            program,
            contract,
            before_peer: preparation.before_peer.cloned(),
            after_peer: preparation.after_peer.cloned(),
            before_topology: preparation.before_topology.cloned(),
            planned_after_topology: preparation.planned_after_topology.cloned(),
            route: preparation.route,
            catalog,
            optiscaler_config,
            read_guard_state: ReadGuardPermitState::File {
                canonical_game_root: read_guards.canonical_game_root,
                requirements: read_guards.requirements,
                initial_evidence: read_guards.initial_evidence,
                catalog_claim: read_guards.catalog_claim,
            },
        }
    }

    pub(super) fn from_shared(
        preparation: SharedPeerCommitPreparation<'_>,
        program: ParsedPeerProgram,
        contract: PeerTransitionContract,
        roots: BoundSharedPeerRoots,
    ) -> Self {
        Self {
            mutation_id: preparation.mutation_id.to_owned(),
            game_id: preparation.game_id.clone(),
            feature: preparation.feature.to_owned(),
            subject_id: None,
            manifest_json: preparation.manifest_json.to_owned(),
            program,
            contract,
            before_peer: preparation.before_peer.cloned(),
            after_peer: preparation.after_peer.cloned(),
            before_topology: preparation.before_topology.cloned(),
            planned_after_topology: preparation.planned_after_topology.cloned(),
            route: preparation.route,
            catalog: OwnedCatalogProjection::default(),
            optiscaler_config: None,
            read_guard_state: ReadGuardPermitState::Shared { roots },
        }
    }

    pub(super) fn into_permit(
        self,
        instance: Arc<RuntimeInstance>,
        binding: PreparedRowBinding,
        fingerprint: RowFingerprint,
    ) -> PreparedPeerCommitPermit {
        PreparedPeerCommitPermit {
            instance,
            binding,
            fingerprint,
            mutation_id: self.mutation_id,
            game_id: self.game_id,
            feature: self.feature,
            subject_id: self.subject_id,
            program: self.program,
            contract: self.contract,
            before_peer: self.before_peer,
            after_peer: self.after_peer,
            before_topology: self.before_topology,
            planned_after_topology: self.planned_after_topology,
            route: self.route,
            catalog: self.catalog,
            optiscaler_config: self.optiscaler_config,
            read_guard_state: self.read_guard_state,
        }
    }
}
