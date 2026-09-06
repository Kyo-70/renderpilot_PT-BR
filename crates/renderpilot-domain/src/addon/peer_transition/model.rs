//! Pure data types for a peer transition contract.

use std::{error::Error, fmt, str::FromStr};

use serde::{Deserialize, Serialize};

use crate::{
    ComponentId, FileReceipt, GameId, GameProxyTopology, InstalledAddon, PathRef,
    ProxyImplementation, ProxyLink, ProxyRootPrestate, Sha256Hash, normalized_path_key,
};

use super::renodx_reshade_ini::RenoDxReshadeIniAuthority;
use super::{dlss::RenoDxDlssProjection, optiscaler_config::ExactOptiConfigProjection};

/// Non-authoritative evidence for one observed file image.
///
/// The identity is retained for diagnostics only.  It never authorizes a
/// transition; callers must prove the typed route and the planned digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerFileImage {
    identity: String,
    sha256: Sha256Hash,
    length: u64,
}

impl PeerFileImage {
    /// Creates an image from an observed native identity, digest, and length.
    pub fn new(
        identity: impl Into<String>,
        sha256: Sha256Hash,
        length: u64,
    ) -> Result<Self, PeerTransitionError> {
        let identity = identity.into();
        if identity.trim().is_empty() {
            return Err(PeerTransitionError::EmptyImageIdentity);
        }
        Ok(Self {
            identity: identity.trim().to_owned(),
            sha256,
            length,
        })
    }

    /// Returns the native identity for diagnostics. It is not authority.
    #[must_use]
    pub fn identity(&self) -> &str {
        &self.identity
    }

    /// Returns the observed SHA-256 digest.
    #[must_use]
    pub fn sha256(&self) -> &Sha256Hash {
        &self.sha256
    }

    /// Returns the observed byte length.
    #[must_use]
    pub const fn length(&self) -> u64 {
        self.length
    }
}

/// Physical operation described by one endpoint intent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeerEndpointOperation {
    /// Materialize an absent endpoint.
    Create,
    /// Replace an existing endpoint at the same path.
    Replace,
    /// Remove an existing endpoint.
    Remove,
}

/// Physical role of one endpoint in a peer route.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PeerEndpointRole {
    /// An endpoint outside the persisted proxy topology.
    Disjoint,
    /// The one downstream endpoint coordinated with the outer proxy.
    TopologyDownstream,
    /// The RenoDX ReShade.ini endpoint admitted by typed root authority.
    RenoDxReshadeIni,
    /// The exact OptiScaler configuration endpoint admitted by typed root
    /// authority. Peers may toggle only the typed LoadReshade setting.
    OptiScalerConfig,
    /// The RenoDX DLSS-Fix companion endpoint bound to its typed claim
    /// projection.
    DlssFix,
}

impl PeerEndpointRole {
    /// Returns the canonical wire identifier used by peer-program envelopes.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Disjoint => "disjoint",
            Self::TopologyDownstream => "topology_downstream",
            Self::RenoDxReshadeIni => "renodx_reshade_ini",
            Self::OptiScalerConfig => "optiscaler_config",
            Self::DlssFix => "dlss_fix",
        }
    }

    /// Parses one canonical peer endpoint role.
    pub fn parse(value: &str) -> Result<Self, PeerTransitionError> {
        match value {
            "disjoint" => Ok(Self::Disjoint),
            "topology_downstream" => Ok(Self::TopologyDownstream),
            "renodx_reshade_ini" => Ok(Self::RenoDxReshadeIni),
            "optiscaler_config" => Ok(Self::OptiScalerConfig),
            "dlss_fix" => Ok(Self::DlssFix),
            _ => Err(PeerTransitionError::UnknownEndpointRole(value.to_owned())),
        }
    }
}

impl FromStr for PeerEndpointRole {
    type Err = PeerTransitionError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

/// Typed authorities admitted by one peer transition.
///
/// Each optional field is a singleton authority. Physical endpoint programs
/// are valid only when their paths and roles agree exactly with the supplied
/// authorities. The OptiScaler config field contains the exact Configuration
/// receipt and semantic operation; it never gives a peer ownership of the INI.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PeerTransitionAuthorities {
    /// Authority for the one topology downstream endpoint.
    pub topology_downstream: Option<PathRef>,
    /// Typed authority for game-root ReShade.ini.
    pub renodx_reshade_ini: Option<RenoDxReshadeIniAuthority>,
    /// Typed authority and operation for game-root OptiScaler.ini.
    pub optiscaler_config: Option<ExactOptiConfigProjection>,
    /// Typed RenoDX DLSS-Fix claim projection.
    pub dlss_fix: Option<RenoDxDlssProjection>,
}

impl PeerTransitionAuthorities {
    /// Returns an empty authority set.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            topology_downstream: None,
            renodx_reshade_ini: None,
            optiscaler_config: None,
            dlss_fix: None,
        }
    }
}

/// Immutable aggregate images and route for one peer transition derivation.
///
/// This groups the five inputs that always travel together through the typed
/// peer-contract extensions. It owns no data and grants no authority.
#[derive(Debug, Clone, Copy)]
pub struct PeerTransitionContext<'a> {
    pub(super) before_peer: Option<&'a InstalledAddon>,
    pub(super) after_peer: Option<&'a InstalledAddon>,
    pub(super) before_topology: Option<&'a GameProxyTopology>,
    pub(super) planned_after_topology: Option<&'a PlannedGameProxyTopology>,
    pub(super) route: ProxyPeerRoute,
}

impl<'a> PeerTransitionContext<'a> {
    /// Creates the exact context used to derive one peer transition.
    #[must_use]
    pub const fn new(
        before_peer: Option<&'a InstalledAddon>,
        after_peer: Option<&'a InstalledAddon>,
        before_topology: Option<&'a GameProxyTopology>,
        planned_after_topology: Option<&'a PlannedGameProxyTopology>,
        route: ProxyPeerRoute,
    ) -> Self {
        Self {
            before_peer,
            after_peer,
            before_topology,
            planned_after_topology,
            route,
        }
    }
}

/// Operation selected for the coordinated topology route.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoordinatedPeerOperation {
    /// Add a downstream peer to an outer-only topology.
    Create,
    /// Replace the downstream in place.
    ReplaceSamePath,
    /// Remove the downstream and restore its return target.
    Remove,
}

/// Closed route class for a peer transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProxyPeerRoute {
    /// Every endpoint is outside the proxy topology.
    DurableDisjoint,
    /// Exactly one endpoint participates in the proxy downstream transition.
    Coordinated(CoordinatedPeerOperation),
}

/// One ordered physical endpoint intent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerEndpointIntent {
    path: PathRef,
    role: PeerEndpointRole,
    operation: PeerEndpointOperation,
    planned_sha256: Option<Sha256Hash>,
    planned_length: Option<u64>,
}

impl PeerEndpointIntent {
    /// Creates an intent with optional typed postimage assertions.
    ///
    /// A remove has no postimage and therefore must carry no planned digest or
    /// length. Generic records may legitimately omit either assertion because
    /// they do not persist file content metadata.
    pub fn new(
        path: PathRef,
        role: PeerEndpointRole,
        operation: PeerEndpointOperation,
        planned_sha256: Option<Sha256Hash>,
        planned_length: Option<u64>,
    ) -> Result<Self, PeerTransitionError> {
        if operation == PeerEndpointOperation::Remove
            && (planned_sha256.is_some() || planned_length.is_some())
        {
            return Err(PeerTransitionError::RemoveHasPostimage(path));
        }
        Ok(Self {
            path,
            role,
            operation,
            planned_sha256,
            planned_length,
        })
    }

    /// Creates a create intent.
    pub fn create(
        path: PathRef,
        role: PeerEndpointRole,
        planned_sha256: Option<Sha256Hash>,
        planned_length: Option<u64>,
    ) -> Result<Self, PeerTransitionError> {
        Self::new(
            path,
            role,
            PeerEndpointOperation::Create,
            planned_sha256,
            planned_length,
        )
    }

    /// Creates a replace intent.
    pub fn replace(
        path: PathRef,
        role: PeerEndpointRole,
        planned_sha256: Option<Sha256Hash>,
        planned_length: Option<u64>,
    ) -> Result<Self, PeerTransitionError> {
        Self::new(
            path,
            role,
            PeerEndpointOperation::Replace,
            planned_sha256,
            planned_length,
        )
    }

    /// Creates a remove intent.
    pub fn remove(path: PathRef, role: PeerEndpointRole) -> Result<Self, PeerTransitionError> {
        Self::new(path, role, PeerEndpointOperation::Remove, None, None)
    }

    /// Returns the endpoint path.
    #[must_use]
    pub fn path(&self) -> &PathRef {
        &self.path
    }

    /// Returns the endpoint role.
    #[must_use]
    pub const fn role(&self) -> PeerEndpointRole {
        self.role
    }

    /// Returns the physical operation.
    #[must_use]
    pub const fn operation(&self) -> PeerEndpointOperation {
        self.operation
    }

    /// Returns the planned postimage digest, when one is persisted.
    #[must_use]
    pub fn planned_sha256(&self) -> Option<&Sha256Hash> {
        self.planned_sha256.as_ref()
    }

    /// Returns the planned postimage length, when one is known.
    #[must_use]
    pub const fn planned_length(&self) -> Option<u64> {
        self.planned_length
    }
}

/// Evidence collected for one endpoint intent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerEndpointEvidence {
    intent: PeerEndpointIntent,
    before: Option<PeerFileImage>,
    after: Option<PeerFileImage>,
}

impl PeerEndpointEvidence {
    /// Creates evidence. Structural consistency is checked by validation so a
    /// caller can construct and report a complete failed proof atomically.
    #[must_use]
    pub fn new(
        intent: PeerEndpointIntent,
        before: Option<PeerFileImage>,
        after: Option<PeerFileImage>,
    ) -> Self {
        Self {
            intent,
            before,
            after,
        }
    }

    /// Returns the intent this evidence claims to satisfy.
    #[must_use]
    pub fn intent(&self) -> &PeerEndpointIntent {
        &self.intent
    }

    /// Returns the observed preimage, when the endpoint existed.
    #[must_use]
    pub fn before(&self) -> Option<&PeerFileImage> {
        self.before.as_ref()
    }

    /// Returns the observed postimage, when the endpoint exists afterwards.
    #[must_use]
    pub fn after(&self) -> Option<&PeerFileImage> {
        self.after.as_ref()
    }
}

/// Domain error returned when a peer route or its evidence is not closed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PeerTransitionError {
    /// An image cannot carry an empty native identity.
    EmptyImageIdentity,
    /// A remove intent cannot carry postimage assertions.
    RemoveHasPostimage(PathRef),
    /// A route must contain at least one endpoint.
    EmptyIntentSet,
    /// An endpoint path occurred more than once.
    DuplicateEndpoint(PathRef),
    /// Two endpoint paths overlap lexically and cannot be independently owned.
    OverlappingEndpoints(PathRef, PathRef),
    /// A no-op endpoint was requested.
    NoopEndpoint(PathRef),
    /// A route carried an endpoint role not admitted by its route class.
    InvalidEndpointRole(PathRef),
    /// A peer-program role is not one of the canonical wire identifiers.
    UnknownEndpointRole(String),
    /// A singleton typed role occurred more than once in one program.
    DuplicateEndpointRole(PeerEndpointRole),
    /// A coordinated route did not carry exactly one downstream endpoint.
    InvalidDownstreamCardinality(usize),
    /// A coordinated operation disagrees with its downstream endpoint operation.
    CoordinatedOperationMismatch,
    /// A peer snapshot is not valid for this contract.
    InvalidPeerSnapshot(&'static str),
    /// A topology snapshot is not valid for this contract.
    InvalidTopologySnapshot(String),
    /// The route attempted to mutate the immutable root or outer link.
    ImmutableTopology,
    /// A topology transition has no required before/after image.
    MissingTopologyImage(&'static str),
    /// A managed downstream claim is absent or does not match its receipt.
    InvalidManagedDownstream(PathRef),
    /// A managed mode transition is not legal.
    InvalidManagedModeTransition(PathRef),
    /// A reused claim changed its accepted digest.
    ReusedClaimChanged(PathRef),
    /// A generic claim changed on a topology participant.
    GenericTopologyOverlap(PathRef),
    /// A route endpoint operation does not match the snapshot transition.
    OperationMismatch(PathRef),
    /// Evidence omitted or added an endpoint.
    ///
    /// `expected` is the number of derived intents and `actual` is the number
    /// of supplied evidence entries.
    EvidenceCardinality {
        /// Number of endpoint intents in the contract.
        expected: usize,
        /// Number of evidence entries supplied by the executor.
        actual: usize,
    },
    /// Evidence order or intent differs from the contract.
    EvidenceOrderMismatch(PathRef),
    /// The evidence has the wrong preimage shape.
    InvalidPreimage(PathRef),
    /// The evidence has the wrong postimage shape.
    InvalidPostimage(PathRef),
    /// A typed digest assertion does not match evidence.
    DigestMismatch(PathRef),
    /// A typed length assertion does not match evidence.
    LengthMismatch(PathRef),
    /// Paired live/sidecar evidence does not preserve the captured bytes.
    PairedImageMismatch(PathRef),
    /// A physical endpoint was not implied by the aggregate transition.
    UnclaimedPhysicalEndpoint(PathRef),
    /// A physical endpoint disagrees with the aggregate transition.
    PhysicalProgramMismatch(PathRef),
    /// A planned topology shape is not legal for the selected route.
    InvalidPlannedTopology(&'static str),
    /// The materializer cannot derive a new topology from this route/shape.
    TopologyMaterializationMismatch(&'static str),
    /// Two sources require incompatible expectations for one normalized path.
    ReadGuardExpectationConflict(PathRef),
    /// A read guard overlaps a physical endpoint and cannot be independently
    /// observed without weakening the mutation contract.
    ReadGuardEndpointOverlap(PathRef, PathRef),
    /// A read-guard requirement is not in canonical source/path form.
    InvalidReadGuardRequirement(PathRef),
    /// The number of read observations differs from the derived requirements.
    ReadGuardEvidenceCardinality {
        /// Number of derived read-guard requirements.
        expected: usize,
        /// Number of observations supplied by the executor.
        actual: usize,
    },
    /// Read observations are not ordered exactly like their requirements.
    ReadGuardEvidenceOrderMismatch(PathRef),
    /// A read observation does not satisfy its typed requirement.
    ReadGuardMismatch(PathRef),
    /// A catalog rollback claim is malformed or cannot be projected exactly.
    CatalogClaimInvalid(&'static str),
    /// A catalog rollback component does not belong to the claim's game.
    CatalogComponentGameMismatch(ComponentId),
    /// A catalog physical endpoint or preimage disagrees with its rollback claim.
    CatalogPhysicalMismatch(PathRef),
    /// A catalog baseline satisfaction could not be discharged at this boundary.
    CatalogBaselineSatisfactionMismatch(PathRef),
    /// The feature does not own the typed RenoDX ReShade.ini endpoint.
    UnsupportedRenoDxReshadeIniFeature,
    /// The typed RenoDX ReShade.ini endpoint did not occur exactly once.
    InvalidRenoDxReshadeIniCardinality(usize),
    /// The typed RenoDX ReShade.ini endpoint could not be derived from the root.
    InvalidRenoDxReshadeIniPath(PathRef),
    /// The typed RenoDX ReShade.ini transition is inconsistent with the record.
    InvalidRenoDxReshadeIniTransition(&'static str),
    /// The OptiScaler config endpoint occurred with invalid cardinality.
    InvalidOptiScalerConfigCardinality(usize),
    /// The OptiScaler config endpoint is not the exact game-root INI path.
    InvalidOptiScalerConfigPath(PathRef),
    /// The OptiScaler config receipt is not a single Configuration binding.
    InvalidOptiScalerConfigReceipt(PathRef),
    /// The OptiScaler config transition is malformed or conflicts with peers.
    InvalidOptiScalerConfigTransition(&'static str),
    /// A config endpoint was supplied for a NoChange semantic operation.
    OptiScalerConfigNoChange(PathRef),
    /// The config endpoint ordinal does not satisfy the downstream ordering
    /// contract.
    OptiScalerConfigOrder {
        /// Ordinal of the config endpoint.
        config: usize,
        /// Ordinal of the conflicting downstream endpoint.
        downstream: usize,
    },
    /// A required config endpoint is missing for an enable/disable operation.
    MissingOptiScalerConfigTransition,
    /// The DLSS-Fix companion endpoint occurred with invalid cardinality.
    InvalidDlssFixCardinality(usize),
    /// The DLSS-Fix endpoint path differs from its typed projection.
    InvalidDlssFixPath(PathRef),
    /// The DLSS-Fix endpoint operation differs from its claim transition.
    InvalidDlssFixOperation(PathRef),
    /// A RenoDX DLSS-Fix claim carries a source with another role.
    InvalidRenoDxDlssSourceRole(crate::TrackedSourceRole),
    /// A RenoDX DLSS-Fix claim is malformed.
    InvalidRenoDxDlssClaim(&'static str),
    /// A RenoDX DLSS-Fix claim does not match the peer record's exact slots.
    RenoDxDlssPeerMismatch(PathRef),
}

impl fmt::Display for PeerTransitionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyImageIdentity => {
                formatter.write_str("peer image identity must not be empty")
            }
            Self::RemoveHasPostimage(path) => {
                write!(formatter, "remove endpoint carries a postimage: {path}")
            }
            Self::EmptyIntentSet => formatter.write_str("peer route has no endpoint intents"),
            Self::DuplicateEndpoint(path) => write!(formatter, "duplicate peer endpoint: {path}"),
            Self::OverlappingEndpoints(left, right) => {
                write!(formatter, "overlapping peer endpoints: {left} and {right}")
            }
            Self::NoopEndpoint(path) => write!(formatter, "peer endpoint is a no-op: {path}"),
            Self::InvalidEndpointRole(path) => {
                write!(formatter, "invalid endpoint role for route: {path}")
            }
            Self::UnknownEndpointRole(role) => {
                write!(formatter, "unknown peer endpoint role: {role}")
            }
            Self::DuplicateEndpointRole(role) => {
                write!(
                    formatter,
                    "duplicate singleton peer endpoint role: {role:?}"
                )
            }
            Self::InvalidDownstreamCardinality(count) => {
                write!(
                    formatter,
                    "expected one topology downstream endpoint, got {count}"
                )
            }
            Self::CoordinatedOperationMismatch => {
                formatter.write_str("coordinated route operation disagrees with downstream intent")
            }
            Self::InvalidPeerSnapshot(reason) => {
                write!(formatter, "invalid peer snapshot: {reason}")
            }
            Self::InvalidTopologySnapshot(reason) => {
                write!(formatter, "invalid topology snapshot: {reason}")
            }
            Self::ImmutableTopology => {
                formatter.write_str("proxy root and outer link are immutable")
            }
            Self::MissingTopologyImage(reason) => {
                write!(formatter, "missing topology image: {reason}")
            }
            Self::InvalidManagedDownstream(path) => {
                write!(formatter, "invalid managed downstream claim: {path}")
            }
            Self::InvalidManagedModeTransition(path) => {
                write!(formatter, "invalid managed mode transition: {path}")
            }
            Self::ReusedClaimChanged(path) => write!(formatter, "reused claim changed: {path}"),
            Self::GenericTopologyOverlap(path) => {
                write!(
                    formatter,
                    "generic claim overlaps a changing topology endpoint: {path}"
                )
            }
            Self::OperationMismatch(path) => {
                write!(
                    formatter,
                    "endpoint operation does not match snapshot state: {path}"
                )
            }
            Self::EvidenceCardinality { expected, actual } => {
                write!(
                    formatter,
                    "evidence cardinality mismatch: expected {expected}, got {actual}"
                )
            }
            Self::EvidenceOrderMismatch(path) => {
                write!(formatter, "evidence order or intent mismatch: {path}")
            }
            Self::InvalidPreimage(path) => write!(formatter, "invalid endpoint preimage: {path}"),
            Self::InvalidPostimage(path) => write!(formatter, "invalid endpoint postimage: {path}"),
            Self::DigestMismatch(path) => write!(formatter, "endpoint digest mismatch: {path}"),
            Self::LengthMismatch(path) => write!(formatter, "endpoint length mismatch: {path}"),
            Self::PairedImageMismatch(path) => {
                write!(formatter, "paired live/sidecar image mismatch: {path}")
            }
            Self::UnclaimedPhysicalEndpoint(path) => {
                write!(
                    formatter,
                    "physical endpoint is not claimed by the peer transition: {path}"
                )
            }
            Self::PhysicalProgramMismatch(path) => {
                write!(
                    formatter,
                    "physical endpoint disagrees with the peer transition: {path}"
                )
            }
            Self::InvalidPlannedTopology(reason) => {
                write!(formatter, "invalid planned proxy topology: {reason}")
            }
            Self::TopologyMaterializationMismatch(reason) => {
                write!(
                    formatter,
                    "proxy topology materialization mismatch: {reason}"
                )
            }
            Self::ReadGuardExpectationConflict(path) => {
                write!(
                    formatter,
                    "conflicting peer read-guard expectations: {path}"
                )
            }
            Self::ReadGuardEndpointOverlap(guard, endpoint) => write!(
                formatter,
                "peer read guard overlaps endpoint: {guard} and {endpoint}"
            ),
            Self::InvalidReadGuardRequirement(path) => {
                write!(formatter, "invalid peer read-guard requirement: {path}")
            }
            Self::ReadGuardEvidenceCardinality { expected, actual } => write!(
                formatter,
                "read-guard evidence cardinality mismatch: expected {expected}, got {actual}"
            ),
            Self::ReadGuardEvidenceOrderMismatch(path) => {
                write!(formatter, "read-guard evidence order mismatch: {path}")
            }
            Self::ReadGuardMismatch(path) => {
                write!(
                    formatter,
                    "peer read guard does not match live state: {path}"
                )
            }
            Self::CatalogClaimInvalid(reason) => {
                write!(formatter, "invalid catalog rollback claim: {reason}")
            }
            Self::CatalogComponentGameMismatch(id) => {
                write!(formatter, "catalog component belongs to another game: {id}")
            }
            Self::CatalogPhysicalMismatch(path) => {
                write!(formatter, "catalog physical endpoint mismatch: {path}")
            }
            Self::CatalogBaselineSatisfactionMismatch(path) => {
                write!(formatter, "catalog baseline satisfaction mismatch: {path}")
            }
            Self::UnsupportedRenoDxReshadeIniFeature => {
                formatter.write_str("unsupported RenoDX ReShade.ini feature")
            }
            Self::InvalidRenoDxReshadeIniCardinality(count) => write!(
                formatter,
                "invalid RenoDX ReShade.ini endpoint cardinality: expected one, got {count}"
            ),
            Self::InvalidRenoDxReshadeIniPath(path) => {
                write!(
                    formatter,
                    "invalid RenoDX ReShade.ini path derived from: {path}"
                )
            }
            Self::InvalidRenoDxReshadeIniTransition(reason) => {
                write!(formatter, "invalid RenoDX ReShade.ini transition: {reason}")
            }
            Self::InvalidOptiScalerConfigCardinality(count) => write!(
                formatter,
                "invalid OptiScaler config endpoint cardinality: expected one, got {count}"
            ),
            Self::InvalidOptiScalerConfigPath(path) => {
                write!(formatter, "invalid OptiScaler config endpoint path: {path}")
            }
            Self::InvalidOptiScalerConfigReceipt(path) => {
                write!(
                    formatter,
                    "invalid OptiScaler Configuration receipt: {path}"
                )
            }
            Self::InvalidOptiScalerConfigTransition(reason) => {
                write!(formatter, "invalid OptiScaler config transition: {reason}")
            }
            Self::OptiScalerConfigNoChange(path) => {
                write!(
                    formatter,
                    "OptiScaler config NoChange has no physical endpoint: {path}"
                )
            }
            Self::OptiScalerConfigOrder { config, downstream } => write!(
                formatter,
                "OptiScaler config endpoint ordinal {config} violates downstream ordinal {downstream}"
            ),
            Self::MissingOptiScalerConfigTransition => {
                formatter.write_str("missing OptiScaler config transition")
            }
            Self::InvalidDlssFixCardinality(count) => write!(
                formatter,
                "invalid DLSS-Fix endpoint cardinality: expected at most one, got {count}"
            ),
            Self::InvalidDlssFixPath(path) => {
                write!(formatter, "invalid DLSS-Fix endpoint path: {path}")
            }
            Self::InvalidDlssFixOperation(path) => {
                write!(formatter, "invalid DLSS-Fix endpoint operation: {path}")
            }
            Self::InvalidRenoDxDlssSourceRole(role) => {
                write!(formatter, "invalid RenoDX DLSS-Fix source role: {role:?}")
            }
            Self::InvalidRenoDxDlssClaim(reason) => {
                write!(formatter, "invalid RenoDX DLSS-Fix claim: {reason}")
            }
            Self::RenoDxDlssPeerMismatch(path) => {
                write!(formatter, "RenoDX DLSS-Fix peer claim mismatch: {path}")
            }
        }
    }
}

impl Error for PeerTransitionError {}

/// A topology plan whose downstream identity may be supplied by postimage
/// evidence at the storage commit boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlannedGameProxyTopology {
    /// Reuse an already committed topology without rebuilding receipts.
    Exact(GameProxyTopology),
    /// Build one newly-owned downstream receipt from the matching evidence.
    ObservedOwnedDownstream {
        /// Stable topology identifier.
        id: String,
        /// Owning game.
        game_id: GameId,
        /// Immutable root slot.
        root_slot: PathRef,
        /// Immutable outer link.
        outer: ProxyLink,
        /// Planned downstream implementation.
        implementation: ProxyImplementation,
        /// Planned downstream path.
        downstream_path: PathRef,
        /// Return target for the downstream link.
        downstream_origin: PathRef,
        /// Immutable root pre-state.
        root_prestate: ProxyRootPrestate,
        /// Expected postimage digest.
        planned_sha256: Sha256Hash,
        /// Expected postimage length.
        planned_length: u64,
    },
}

impl PlannedGameProxyTopology {
    /// Materializes the only topology identity that may be created from
    /// runtime evidence.  Exact plans are cloned and never receive a new
    /// receipt identity.
    pub fn materialize_from_evidence(
        &self,
        route: ProxyPeerRoute,
        intents: &[PeerEndpointIntent],
        evidence: &[PeerEndpointEvidence],
    ) -> Result<GameProxyTopology, PeerTransitionError> {
        super::validation::validate_intents(route, intents)?;
        super::validation::validate_evidence(intents, evidence)?;
        self.materialize_validated(route, intents, evidence, false)
    }

    /// Materializes topology only after validating the authority-bearing
    /// contract that produced the endpoint evidence.
    pub fn materialize_from_contract_evidence(
        &self,
        contract: &super::PeerTransitionContract,
        evidence: &[PeerEndpointEvidence],
    ) -> Result<GameProxyTopology, PeerTransitionError> {
        contract.validate_intents()?;
        contract.validate_evidence(evidence)?;
        self.materialize_validated(
            contract.route(),
            contract.intents(),
            evidence,
            contract.renodx_reshade_ini_authority().is_some(),
        )
    }

    fn materialize_validated(
        &self,
        route: ProxyPeerRoute,
        intents: &[PeerEndpointIntent],
        evidence: &[PeerEndpointEvidence],
        allow_renodx_reshade_ini: bool,
    ) -> Result<GameProxyTopology, PeerTransitionError> {
        match self {
            Self::Exact(topology) => {
                materialize_exact(topology, route, intents, allow_renodx_reshade_ini)
            }
            Self::ObservedOwnedDownstream { .. } => {
                materialize_observed(self, route, intents, evidence)
            }
        }
    }
}

fn materialize_exact(
    topology: &GameProxyTopology,
    route: ProxyPeerRoute,
    intents: &[PeerEndpointIntent],
    allow_renodx_reshade_ini: bool,
) -> Result<GameProxyTopology, PeerTransitionError> {
    match route {
        ProxyPeerRoute::DurableDisjoint => {
            if intents.iter().any(|intent| {
                intent.role() != PeerEndpointRole::Disjoint
                    && !(allow_renodx_reshade_ini
                        && intent.role() == PeerEndpointRole::RenoDxReshadeIni)
            }) {
                return Err(PeerTransitionError::TopologyMaterializationMismatch(
                    "durable route has topology evidence",
                ));
            }
        }
        ProxyPeerRoute::Coordinated(CoordinatedPeerOperation::Remove) => {
            if topology.downstream.is_some()
                || intents
                    .iter()
                    .filter(|intent| intent.role() == PeerEndpointRole::TopologyDownstream)
                    .count()
                    != 1
            {
                return Err(PeerTransitionError::TopologyMaterializationMismatch(
                    "remove requires exact downstream absence",
                ));
            }
        }
        ProxyPeerRoute::Coordinated(
            CoordinatedPeerOperation::Create | CoordinatedPeerOperation::ReplaceSamePath,
        ) => {
            return Err(PeerTransitionError::TopologyMaterializationMismatch(
                "exact topology cannot materialize coordinated create or replace",
            ));
        }
    }
    topology
        .validate()
        .map_err(|error| PeerTransitionError::InvalidTopologySnapshot(error.to_string()))?;
    Ok(topology.clone())
}

fn materialize_observed(
    plan: &PlannedGameProxyTopology,
    route: ProxyPeerRoute,
    intents: &[PeerEndpointIntent],
    evidence: &[PeerEndpointEvidence],
) -> Result<GameProxyTopology, PeerTransitionError> {
    let PlannedGameProxyTopology::ObservedOwnedDownstream {
        id,
        game_id,
        root_slot,
        outer,
        implementation,
        downstream_path,
        downstream_origin,
        root_prestate,
        planned_sha256,
        planned_length,
    } = plan
    else {
        return Err(PeerTransitionError::TopologyMaterializationMismatch(
            "exact topology cannot materialize observed downstream",
        ));
    };
    if !matches!(
        route,
        ProxyPeerRoute::Coordinated(
            CoordinatedPeerOperation::Create | CoordinatedPeerOperation::ReplaceSamePath
        )
    ) {
        return Err(PeerTransitionError::TopologyMaterializationMismatch(
            "observed downstream requires coordinated create or replace",
        ));
    }
    let downstream = intents
        .iter()
        .enumerate()
        .filter(|(_, intent)| intent.role() == PeerEndpointRole::TopologyDownstream)
        .collect::<Vec<_>>();
    if downstream.len() != 1 {
        return Err(PeerTransitionError::InvalidDownstreamCardinality(
            downstream.len(),
        ));
    }
    if *implementation != ProxyImplementation::ReShade {
        return Err(PeerTransitionError::InvalidPlannedTopology(
            "planned downstream is not the peer host implementation",
        ));
    }
    let (index, intent) = downstream[0];
    if normalized_path_key(intent.path().as_str()) != normalized_path_key(downstream_path.as_str())
        || intent.operation()
            != match route {
                ProxyPeerRoute::Coordinated(CoordinatedPeerOperation::Create) => {
                    PeerEndpointOperation::Create
                }
                ProxyPeerRoute::Coordinated(CoordinatedPeerOperation::ReplaceSamePath) => {
                    PeerEndpointOperation::Replace
                }
                ProxyPeerRoute::DurableDisjoint
                | ProxyPeerRoute::Coordinated(CoordinatedPeerOperation::Remove) => {
                    unreachable!("route checked above")
                }
            }
    {
        return Err(PeerTransitionError::PhysicalProgramMismatch(
            downstream_path.clone(),
        ));
    }
    let post = evidence[index]
        .after()
        .ok_or_else(|| PeerTransitionError::InvalidPostimage(downstream_path.clone()))?;
    if post.sha256() != planned_sha256 {
        return Err(PeerTransitionError::DigestMismatch(downstream_path.clone()));
    }
    if post.length() != *planned_length {
        return Err(PeerTransitionError::LengthMismatch(downstream_path.clone()));
    }
    let receipt = FileReceipt::owned(post.identity(), post.sha256().clone())
        .map_err(|_| PeerTransitionError::InvalidManagedDownstream(downstream_path.clone()))?;
    let topology = GameProxyTopology {
        id: id.to_owned(),
        game_id: game_id.clone(),
        root_slot: root_slot.clone(),
        outer: outer.clone(),
        downstream: Some(ProxyLink {
            implementation: *implementation,
            path: downstream_path.clone(),
            receipt,
        }),
        downstream_origin: Some(downstream_origin.clone()),
        root_prestate: *root_prestate,
    };
    topology
        .validate()
        .map_err(|error| PeerTransitionError::InvalidTopologySnapshot(error.to_string()))?;
    Ok(topology)
}
