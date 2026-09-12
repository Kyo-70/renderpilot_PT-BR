//! Closed, ordered physical peer program used by filesystem execution.
//!
//! This module is the orchestration-side lowering boundary.  It contains no
//! storage handle or mutation id; durable ownership stays in the executor and
//! the domain transition contract remains the authority for commit evidence.

use std::path::{Path, PathBuf};
use std::{error::Error, fmt};

use renderpilot_domain::{CapabilityToken, PathRef, PeerEndpointRole, Sha256Hash};
use serde::Serialize;

use super::ancestors::PeerAncestorPlan;

/// Closed execution class for a file-peer ceremony. The serde spelling is the
/// storage wire contract; adapters cannot introduce a new class at runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PeerExecutionClass {
    Ordinary,
}

/// The only serializable representation accepted by the durable peer
/// validators.  Adapters never construct JSON or pass untyped values: this
/// envelope is lowered from the already validated endpoint program immediately
/// before the Preparing row is sealed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct PeerProgramEnvelope {
    format: u8,
    transaction_owner: String,
    execution_class: PeerExecutionClass,
    roots: Vec<String>,
    stage: Vec<String>,
    custody: Vec<String>,
    created_ancestors: Vec<String>,
    endpoints: Vec<PeerProgramEndpoint>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct PeerProgramEndpoint {
    ordinal: usize,
    path: String,
    role: &'static str,
    operation: &'static str,
    planned_sha256: Option<String>,
    planned_length: Option<u64>,
    before: Option<PeerProgramImage>,
    read_guards: Vec<String>,
    subtree_publishes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct PeerProgramImage {
    identity: String,
    sha256: String,
    length: u64,
}

/// Lowers the native O1 program and payload lengths into format 1.
/// Capability tokens are deliberately derived from the ordered endpoint set,
/// so the serialized ceremony is deterministic and cannot grow an implicit
/// filesystem participant between preflight and apply.
pub(crate) fn build_peer_program(
    mutation_id: &str,
    execution_class: PeerExecutionClass,
    program: &ExactEndpointProgram,
    before: &[EndpointObservation],
    planned_lengths: &[Option<u64>],
    ancestor_plan: &PeerAncestorPlan,
    authorized_roots: &[std::path::PathBuf],
) -> Result<PeerProgramEnvelope, PeerRouteError> {
    if mutation_id.trim().is_empty() {
        return Err(PeerRouteError::InvalidIdentity);
    }
    if before.len() != program.endpoints().len()
        || planned_lengths.len() != program.endpoints().len()
    {
        return Err(PeerRouteError::EvidenceCardinality {
            expected: program.endpoints().len(),
            actual: before.len(),
        });
    }

    if authorized_roots.is_empty() {
        return Err(PeerRouteError::InvalidPath(
            "peer program has no authorized roots".to_owned(),
        ));
    }
    let roots = render_peer_roots(authorized_roots)?;
    let stage = std::collections::BTreeSet::new();
    let mut custody = std::collections::BTreeSet::new();
    let created_ancestors = ancestor_plan
        .ancestors()
        .iter()
        .map(|ancestor| {
            PathRef::from_canonical_native_absolute(ancestor.path())
                .map_err(|error| PeerRouteError::InvalidPath(error.to_string()))
                .and_then(|path| {
                    CapabilityToken::from_path(&path, &roots)
                        .map(|token| token.as_str().to_owned())
                        .map_err(|error| PeerRouteError::InvalidPath(error.to_string()))
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut endpoints = Vec::with_capacity(program.endpoints().len());

    for (ordinal, ((endpoint, observed), planned_length)) in program
        .endpoints()
        .iter()
        .zip(before)
        .zip(planned_lengths)
        .enumerate()
    {
        let path = endpoint.path().as_str().to_owned();
        let capability = CapabilityToken::from_path(endpoint.path(), &roots)
            .map(|token| token.as_str().to_owned())
            .map_err(|error| PeerRouteError::InvalidPath(error.to_string()))?;
        if matches!(observed, EndpointObservation::File(_)) {
            custody.insert(capability.clone());
        }
        let (operation, planned_sha256, planned_length) = match endpoint.after() {
            EndpointPostcondition::Absent => {
                if planned_length.is_some() {
                    return Err(PeerRouteError::UnexpectedPayload(endpoint.path().clone()));
                }
                ("remove", None, None)
            }
            EndpointPostcondition::File(digest) => {
                let Some(length) = *planned_length else {
                    return Err(PeerRouteError::MissingPayload(endpoint.path().clone()));
                };
                (
                    "replace_or_create",
                    Some(digest.as_str().to_owned()),
                    Some(length),
                )
            }
        };
        let operation = match endpoint.before() {
            EndpointExpectation::Absent if operation == "replace_or_create" => "create",
            EndpointExpectation::File(_) if operation == "replace_or_create" => "replace",
            EndpointExpectation::File(_) if operation == "remove" => "remove",
            _ => return Err(PeerRouteError::InvalidPath(path)),
        };
        let subtree_publishes = ancestor_plan
            .endpoint_chain(ordinal)
            .ok_or_else(|| PeerRouteError::EvidenceCardinality {
                expected: program.endpoints().len(),
                actual: ordinal,
            })?
            .iter()
            .map(|ancestor_index| {
                ancestor_plan
                    .ancestors()
                    .get(*ancestor_index)
                    .ok_or_else(|| PeerRouteError::InvalidPath(path.clone()))
                    .and_then(|ancestor| {
                        PathRef::from_canonical_native_absolute(ancestor.path())
                            .map_err(|error| PeerRouteError::InvalidPath(error.to_string()))
                            .and_then(|ancestor| {
                                CapabilityToken::from_path(&ancestor, &roots)
                                    .map(|token| token.as_str().to_owned())
                                    .map_err(|error| PeerRouteError::InvalidPath(error.to_string()))
                            })
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        endpoints.push(PeerProgramEndpoint {
            ordinal,
            path,
            role: role_name(endpoint.role()),
            operation,
            planned_sha256,
            planned_length,
            before: image_from_expectation(endpoint.before()),
            read_guards: vec![capability],
            subtree_publishes,
        });
    }

    Ok(PeerProgramEnvelope {
        format: 1,
        transaction_owner: mutation_id.to_owned(),
        execution_class,
        roots,
        stage: stage.into_iter().collect(),
        custody: custody.into_iter().collect(),
        created_ancestors,
        endpoints,
    })
}

pub(crate) fn render_peer_root(path: &Path) -> Result<String, PeerRouteError> {
    PathRef::from_canonical_native_absolute(path)
        .map(|path| path.as_str().to_owned())
        .map_err(|error| PeerRouteError::InvalidPath(error.to_string()))
}

pub(crate) fn render_peer_roots(paths: &[PathBuf]) -> Result<Vec<String>, PeerRouteError> {
    paths.iter().map(|path| render_peer_root(path)).collect()
}

fn image_from_expectation(expectation: &EndpointExpectation) -> Option<PeerProgramImage> {
    match expectation {
        EndpointExpectation::Absent => None,
        EndpointExpectation::File(image) => Some(PeerProgramImage {
            identity: image.identity().to_owned(),
            sha256: image.digest().as_str().to_owned(),
            length: image.length(),
        }),
    }
}

fn role_name(role: PeerEndpointRole) -> &'static str {
    match role {
        PeerEndpointRole::Disjoint => "disjoint",
        PeerEndpointRole::TopologyDownstream => "topology_downstream",
        PeerEndpointRole::RenoDxReshadeIni => "renodx_reshade_ini",
        PeerEndpointRole::OptiScalerConfig => "optiscaler_config",
        PeerEndpointRole::DlssFix => "dlss_fix",
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct VerifiedPeerFile {
    pub(crate) identity: String,
    pub(crate) digest: Sha256Hash,
    pub(crate) length: u64,
}

impl VerifiedPeerFile {
    pub(crate) fn new_with_length(
        identity: String,
        digest: Sha256Hash,
        length: u64,
    ) -> Result<Self, PeerRouteError> {
        if identity.trim().is_empty() || identity.contains('\0') {
            return Err(PeerRouteError::InvalidIdentity);
        }
        Ok(Self {
            identity,
            digest,
            length,
        })
    }

    pub(crate) fn identity(&self) -> &str {
        &self.identity
    }
    pub(crate) fn digest(&self) -> &Sha256Hash {
        &self.digest
    }
    pub(crate) fn length(&self) -> u64 {
        self.length
    }

    /// Compares an in-memory payload with this sealed image without touching
    /// the filesystem or changing the authority represented by the image.
    pub(crate) fn matches_content_bytes(&self, bytes: &[u8]) -> bool {
        u64::try_from(bytes.len()).ok() == Some(self.length)
            && renderpilot_detection::sha256_bytes(bytes).is_ok_and(|digest| digest == self.digest)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum EndpointObservation {
    Absent,
    File(VerifiedPeerFile),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum EndpointExpectation {
    Absent,
    File(VerifiedPeerFile),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum EndpointPostcondition {
    Absent,
    File(Sha256Hash),
}

impl EndpointPostcondition {
    /// Builds the file postcondition for an in-memory payload.
    pub(crate) fn for_file_bytes(bytes: &[u8]) -> Result<Self, PeerRouteError> {
        renderpilot_detection::sha256_bytes(bytes)
            .map(Self::File)
            .map_err(|_| PeerRouteError::InvalidIdentity)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExactEndpoint {
    path: PathRef,
    role: PeerEndpointRole,
    before: EndpointExpectation,
    after: EndpointPostcondition,
}

impl ExactEndpoint {
    pub(crate) fn new(
        path: PathRef,
        role: PeerEndpointRole,
        before: EndpointExpectation,
        after: EndpointPostcondition,
    ) -> Self {
        Self {
            path,
            role,
            before,
            after,
        }
    }

    pub(crate) fn path(&self) -> &PathRef {
        &self.path
    }
    pub(crate) fn role(&self) -> PeerEndpointRole {
        self.role
    }
    pub(crate) fn before(&self) -> &EndpointExpectation {
        &self.before
    }
    pub(crate) fn after(&self) -> &EndpointPostcondition {
        &self.after
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExactEndpointProgram {
    endpoints: Vec<ExactEndpoint>,
}

impl ExactEndpointProgram {
    pub(crate) fn new(mut endpoints: Vec<ExactEndpoint>) -> Result<Self, PeerRouteError> {
        if endpoints.is_empty() {
            return Err(PeerRouteError::EmptyProgram);
        }
        let mut paths = std::collections::BTreeSet::new();
        for endpoint in &mut endpoints {
            endpoint.path =
                PathRef::from_canonical_native_absolute(Path::new(endpoint.path.as_str()))
                    .map_err(|error| PeerRouteError::InvalidPath(error.to_string()))?;
            if !paths.insert(renderpilot_domain::normalized_path_key(
                endpoint.path.as_str(),
            )) {
                return Err(PeerRouteError::DuplicatePath(endpoint.path.clone()));
            }
        }
        Ok(Self { endpoints })
    }

    /// Constructs the sole endpoint-free peer program admitted by
    /// orchestration. The caller must still bind a typed RenoDX DLSS
    /// projection through the specialized package and storage preparation
    /// APIs; generic package construction never calls this constructor.
    pub(crate) fn renodx_dlss_claim_only() -> Self {
        Self {
            endpoints: Vec::new(),
        }
    }

    /// Adds one endpoint while preserving the same canonical path and
    /// duplicate checks as initial program construction. The typed
    /// RenoDX/OptiScaler companion appends its enable transition after host
    /// acquisition, so the domain contract can enforce the required order.
    pub(crate) fn append(self, endpoint: ExactEndpoint) -> Result<Self, PeerRouteError> {
        let mut endpoints = self.endpoints;
        endpoints.push(endpoint);
        Self::new(endpoints)
    }

    /// Inserts one endpoint before every existing effect. The typed
    /// RenoDX/OptiScaler companion uses this only for its disable transition,
    /// which must happen before the downstream host is released.
    pub(crate) fn prepend(self, endpoint: ExactEndpoint) -> Result<Self, PeerRouteError> {
        let mut endpoints = self.endpoints;
        endpoints.insert(0, endpoint);
        Self::new(endpoints)
    }

    pub(crate) fn endpoints(&self) -> &[ExactEndpoint] {
        &self.endpoints
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EndpointEvidence {
    role: PeerEndpointRole,
    path: PathRef,
    before: EndpointObservation,
    after: EndpointObservation,
}

impl EndpointEvidence {
    pub(crate) fn new(
        role: PeerEndpointRole,
        path: PathRef,
        before: &EndpointObservation,
        after: EndpointObservation,
    ) -> Self {
        Self {
            role,
            path,
            before: before.clone(),
            after,
        }
    }

    pub(crate) fn path(&self) -> &PathRef {
        &self.path
    }
    pub(crate) fn before(&self) -> &EndpointObservation {
        &self.before
    }
    pub(crate) fn after(&self) -> &EndpointObservation {
        &self.after
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PeerRouteError {
    EmptyProgram,
    DuplicatePath(PathRef),
    EvidenceCardinality { expected: usize, actual: usize },
    InvalidIdentity,
    InvalidPath(String),
    PlannedDigestMismatch,
    MissingPayload(PathRef),
    UnexpectedPayload(PathRef),
}

impl fmt::Display for PeerRouteError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyProgram => formatter.write_str("peer program is empty"),
            Self::DuplicatePath(path) => write!(formatter, "duplicate peer path {}", path.as_str()),
            Self::EvidenceCardinality { expected, actual } => {
                write!(
                    formatter,
                    "peer evidence cardinality {actual}, expected {expected}"
                )
            }
            Self::InvalidIdentity => formatter.write_str("peer identity is empty or contains NUL"),
            Self::InvalidPath(error) => write!(formatter, "invalid peer path: {error}"),
            Self::PlannedDigestMismatch => {
                formatter.write_str("peer payload digest differs from its plan")
            }
            Self::MissingPayload(path) => {
                write!(formatter, "peer payload is missing for {}", path.as_str())
            }
            Self::UnexpectedPayload(path) => write!(
                formatter,
                "peer payload is unexpected for {}",
                path.as_str()
            ),
        }
    }
}

impl Error for PeerRouteError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capability_token_uses_canonical_declared_drive_root() {
        let roots = ["C:/Games/Example".to_owned()];
        let path = PathRef::parse_exact("C:/Games/Example/ReShade64.dll").expect("path");

        assert_eq!(
            CapabilityToken::from_path(&path, &roots)
                .expect("capability")
                .as_str(),
            "C:/Games/Example:reshade64.dll"
        );
    }

    #[test]
    fn capability_token_uses_canonical_declared_unc_root() {
        let roots = ["//server/share/Game".to_owned()];
        let path = PathRef::parse_exact("//server/share/Game/ReShade64.dll").expect("path");

        assert_eq!(
            CapabilityToken::from_path(&path, &roots)
                .expect("capability")
                .as_str(),
            "//server/share/Game:reshade64.dll"
        );
    }

    #[test]
    fn capability_token_rejects_sibling_prefix() {
        let roots = ["C:/Games/Game".to_owned()];
        let path = PathRef::parse_exact("C:/Games/Game2/ReShade64.dll").expect("path");

        assert!(CapabilityToken::from_path(&path, &roots).is_err());
    }

    #[test]
    fn content_helpers_match_exact_digest_and_length() {
        let bytes = b"peer payload";
        let digest = renderpilot_detection::sha256_bytes(bytes).expect("digest");
        let image = VerifiedPeerFile::new_with_length(
            "peer-file".to_owned(),
            digest.clone(),
            u64::try_from(bytes.len()).expect("length"),
        )
        .expect("image");

        assert!(image.matches_content_bytes(bytes));
        assert!(!image.matches_content_bytes(b"peer payloaD"));
        assert!(!image.matches_content_bytes(b"short"));
        assert_eq!(
            EndpointPostcondition::for_file_bytes(bytes).expect("postcondition"),
            EndpointPostcondition::File(digest)
        );
    }

    #[test]
    fn envelope_uses_authorized_root_and_exact_postimage_shape() {
        let root = tempfile::tempdir().expect("root");
        let path = PathRef::new(
            root.path()
                .join("ReShade64.dll")
                .to_string_lossy()
                .replace('\\', "/"),
        )
        .expect("path");
        let roots = [root.path().to_owned()];
        let digest = Sha256Hash::new("a".repeat(64)).expect("digest");
        let program = ExactEndpointProgram::new(vec![ExactEndpoint::new(
            path,
            PeerEndpointRole::Disjoint,
            EndpointExpectation::Absent,
            EndpointPostcondition::File(digest),
        )])
        .expect("program");
        let ancestor_plan =
            PeerAncestorPlan::derive(&program, &[EndpointObservation::Absent], &roots)
                .expect("ancestor plan");
        let envelope = build_peer_program(
            "mutation-1",
            PeerExecutionClass::Ordinary,
            &program,
            &[EndpointObservation::Absent],
            &[Some(7)],
            &ancestor_plan,
            &roots,
        )
        .expect("envelope");
        let peer_json = serde_json::to_string(&envelope).expect("json");
        assert!(peer_json.contains(r#""execution_class":"ordinary""#));
        let manifest = format!(r#"{{"peer_program":{peer_json}}}"#);
        renderpilot_storage_sqlite::validate_file_peer_program_manifest(&manifest)
            .expect("peer manifest");
        assert!(envelope.stage.is_empty());
        assert!(envelope.created_ancestors.is_empty());
        let root_token = root.path().to_string_lossy().replace('\\', "/");
        let expected_relative = if renderpilot_domain::durable_wire::is_windows_path(&root_token) {
            "reshade64.dll"
        } else {
            "ReShade64.dll"
        };
        assert_eq!(
            envelope.endpoints[0].read_guards,
            [format!("{root_token}:{expected_relative}")]
        );
    }

    #[test]
    fn envelope_lowers_typed_renodx_reshade_ini_role_without_generic_aliasing() {
        let root = tempfile::tempdir().expect("root");
        let path = PathRef::new(
            root.path()
                .join("ReShade.ini")
                .to_string_lossy()
                .replace('\\', "/"),
        )
        .expect("path");
        let digest = Sha256Hash::new("b".repeat(64)).expect("digest");
        let program = ExactEndpointProgram::new(vec![ExactEndpoint::new(
            path,
            PeerEndpointRole::RenoDxReshadeIni,
            EndpointExpectation::Absent,
            EndpointPostcondition::File(digest),
        )])
        .expect("program");
        let ancestor_plan = PeerAncestorPlan::derive(
            &program,
            &[EndpointObservation::Absent],
            &[root.path().to_owned()],
        )
        .expect("ancestor plan");
        let envelope = build_peer_program(
            "mutation-renodx-ini",
            PeerExecutionClass::Ordinary,
            &program,
            &[EndpointObservation::Absent],
            &[Some(1)],
            &ancestor_plan,
            &[root.path().to_owned()],
        )
        .expect("envelope");

        let json = serde_json::to_string(&envelope).expect("json");
        assert!(json.contains(r#""role":"renodx_reshade_ini""#));
        assert!(!json.contains(r#""role":"disjoint""#));
    }

    #[test]
    fn envelope_lowers_shared_ancestors_parent_first_per_endpoint() {
        let root = tempfile::tempdir().expect("root");
        let first = root.path().join("nested/deeper/one.dll");
        let second = root.path().join("nested/deeper/two.dll");
        let path = |path: &std::path::Path| {
            PathRef::new(path.to_string_lossy().replace('\\', "/")).expect("path")
        };
        let digest = Sha256Hash::new("a".repeat(64)).expect("digest");
        let program = ExactEndpointProgram::new(vec![
            ExactEndpoint::new(
                path(&first),
                PeerEndpointRole::Disjoint,
                EndpointExpectation::Absent,
                EndpointPostcondition::File(digest.clone()),
            ),
            ExactEndpoint::new(
                path(&second),
                PeerEndpointRole::Disjoint,
                EndpointExpectation::Absent,
                EndpointPostcondition::File(digest),
            ),
        ])
        .expect("program");
        let before = [EndpointObservation::Absent, EndpointObservation::Absent];
        let ancestor_plan = PeerAncestorPlan::derive(&program, &before, &[root.path().to_owned()])
            .expect("ancestor plan");
        let root_token = root.path().to_string_lossy().replace('\\', "/");
        let envelope = build_peer_program(
            "mutation-ancestors",
            PeerExecutionClass::Ordinary,
            &program,
            &before,
            &[Some(1), Some(2)],
            &ancestor_plan,
            &[root.path().to_owned()],
        )
        .expect("envelope");

        assert_eq!(
            envelope.created_ancestors,
            vec![
                format!("{root_token}:nested"),
                format!("{root_token}:nested/deeper")
            ]
        );
        assert_eq!(
            envelope.endpoints[0].subtree_publishes,
            envelope.created_ancestors.clone()
        );
        assert_eq!(
            envelope.endpoints[1].subtree_publishes,
            envelope.created_ancestors
        );
        assert!(envelope.stage.is_empty());
    }
}
