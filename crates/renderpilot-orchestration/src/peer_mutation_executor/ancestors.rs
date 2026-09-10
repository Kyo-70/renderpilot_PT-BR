use std::{
    collections::BTreeSet,
    error::Error,
    fmt,
    path::{Component, Path, PathBuf},
};

use renderpilot_domain::PathRef;
use serde::{Deserialize, Serialize};

use super::{
    EndpointExpectation, EndpointObservation, EndpointPostcondition, ExactEndpointProgram,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PeerAncestor {
    path: PathBuf,
    wire_path: PathRef,
    consumers: Vec<usize>,
}

/// Typed root-manifest projection of the exact ancestor plan. The path uses
/// canonical [`PathRef`] spelling, while recovery retains the endpoint
/// ordinals that caused it to be created.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PeerAncestorManifestEntry {
    path: String,
    consumer_ordinals: Vec<usize>,
}

impl PeerAncestorManifestEntry {
    pub(crate) fn path(&self) -> &str {
        &self.path
    }

    #[cfg(test)]
    pub(crate) fn consumer_ordinals(&self) -> &[usize] {
        &self.consumer_ordinals
    }
}

impl PeerAncestor {
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    #[cfg(test)]
    pub(crate) fn consumers(&self) -> &[usize] {
        &self.consumers
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PeerAncestorPlan {
    ancestors: Vec<PeerAncestor>,
    endpoint_chains: Vec<Vec<usize>>,
}

impl PeerAncestorPlan {
    pub(crate) fn derive(
        program: &ExactEndpointProgram,
        before: &[EndpointObservation],
        authorized_roots: &[PathBuf],
    ) -> Result<Self, PeerAncestorPlanError> {
        if before.len() != program.endpoints().len() {
            return Err(PeerAncestorPlanError::EvidenceCardinality {
                expected: program.endpoints().len(),
                actual: before.len(),
            });
        }

        let roots = validate_roots(authorized_roots)?;
        let endpoint_keys = program
            .endpoints()
            .iter()
            .map(|endpoint| path_key(Path::new(endpoint.path().as_str())))
            .collect::<BTreeSet<_>>();
        let mut candidates = Vec::<Candidate>::new();
        let mut endpoint_candidate_keys = Vec::with_capacity(program.endpoints().len());

        for (ordinal, (endpoint, observed)) in program.endpoints().iter().zip(before).enumerate() {
            validate_before(endpoint.before(), observed, endpoint.path().as_str())?;
            let endpoint_path = Path::new(endpoint.path().as_str());
            if !endpoint_path.is_absolute() {
                return Err(PeerAncestorPlanError::InvalidEndpointPath(
                    endpoint.path().as_str().to_owned(),
                ));
            }
            let Some((root_index, root)) = roots
                .iter()
                .enumerate()
                .find(|(_, root)| is_within_root(endpoint_path, root))
            else {
                return Err(PeerAncestorPlanError::OutsideAuthorizedRoot(
                    endpoint.path().as_str().to_owned(),
                ));
            };
            if same_path_components(endpoint_path, root) {
                return Err(PeerAncestorPlanError::EndpointOverlapsRoot(
                    endpoint.path().as_str().to_owned(),
                ));
            }

            let operation = operation(endpoint.before(), endpoint.after())?;
            let parent_paths = parent_paths(endpoint_path, root)?;
            if let Some(overlap) = parent_paths
                .into_iter()
                .find(|path| endpoint_keys.contains(&path_key(path)))
            {
                return Err(PeerAncestorPlanError::EndpointAncestorOverlap(overlap));
            }
            let missing = inspect_parent_chain(endpoint_path, root, &operation)?;
            let mut chain = Vec::with_capacity(missing.len());
            for path in missing {
                let depth = relative_components(&path, root)?.len();
                let key = path_key(&path);
                if let Some(index) = candidates.iter().position(|candidate| candidate.key == key) {
                    let candidate = &mut candidates[index];
                    candidate.consumers.insert(ordinal);
                    candidate.root_index = candidate.root_index.min(root_index);
                    candidate.depth = candidate.depth.min(depth + 1);
                } else {
                    candidates.push(Candidate {
                        key: key.clone(),
                        path,
                        root_index,
                        depth: depth + 1,
                        consumers: BTreeSet::from([ordinal]),
                    });
                }
                chain.push(key);
            }
            endpoint_candidate_keys.push(chain);
        }

        candidates.sort_by(|left, right| {
            (left.root_index, left.depth, &left.key).cmp(&(
                right.root_index,
                right.depth,
                &right.key,
            ))
        });

        let mut key_to_index = std::collections::HashMap::with_capacity(candidates.len());
        for (index, candidate) in candidates.iter().enumerate() {
            key_to_index.insert(candidate.key.clone(), index);
        }
        let ancestors = candidates
            .into_iter()
            .map(|candidate| {
                let wire_path =
                    PathRef::from_canonical_native_absolute(&candidate.path).map_err(|_| {
                        PeerAncestorPlanError::InvalidEndpointPath(
                            candidate.path.to_string_lossy().into_owned(),
                        )
                    })?;
                Ok(PeerAncestor {
                    path: candidate.path,
                    wire_path,
                    consumers: candidate.consumers.into_iter().collect(),
                })
            })
            .collect::<Result<Vec<_>, PeerAncestorPlanError>>()?;
        let endpoint_chains = endpoint_candidate_keys
            .into_iter()
            .map(|keys| {
                keys.into_iter()
                    .map(|key| key_to_index[key.as_str()])
                    .collect()
            })
            .collect();

        Ok(Self {
            ancestors,
            endpoint_chains,
        })
    }

    pub(crate) fn ancestors(&self) -> &[PeerAncestor] {
        &self.ancestors
    }

    pub(crate) fn endpoint_chain(&self, ordinal: usize) -> Option<&[usize]> {
        self.endpoint_chains.get(ordinal).map(Vec::as_slice)
    }

    pub(crate) fn manifest_entries(&self) -> Vec<PeerAncestorManifestEntry> {
        self.ancestors
            .iter()
            .map(|ancestor| PeerAncestorManifestEntry {
                path: ancestor.wire_path.as_str().to_owned(),
                consumer_ordinals: ancestor.consumers.clone(),
            })
            .collect()
    }
}

struct Candidate {
    key: String,
    path: PathBuf,
    root_index: usize,
    depth: usize,
    consumers: BTreeSet<usize>,
}

enum Operation {
    Create,
    Replace,
    Remove,
}

fn operation(
    before: &EndpointExpectation,
    after: &EndpointPostcondition,
) -> Result<Operation, PeerAncestorPlanError> {
    match (before, after) {
        (EndpointExpectation::Absent, EndpointPostcondition::File(_)) => Ok(Operation::Create),
        (EndpointExpectation::File(_), EndpointPostcondition::File(_)) => Ok(Operation::Replace),
        (EndpointExpectation::File(_), EndpointPostcondition::Absent) => Ok(Operation::Remove),
        _ => Err(PeerAncestorPlanError::InvalidEndpointTransition),
    }
}

fn validate_before(
    expected: &EndpointExpectation,
    observed: &EndpointObservation,
    path: &str,
) -> Result<(), PeerAncestorPlanError> {
    let matches = match (expected, observed) {
        (EndpointExpectation::Absent, EndpointObservation::Absent) => true,
        (EndpointExpectation::File(expected), EndpointObservation::File(observed)) => {
            expected == observed
        }
        _ => false,
    };
    if matches {
        Ok(())
    } else {
        Err(PeerAncestorPlanError::BeforeObservationMismatch(
            path.to_owned(),
        ))
    }
}

fn validate_roots(roots: &[PathBuf]) -> Result<Vec<PathBuf>, PeerAncestorPlanError> {
    if roots.is_empty() {
        return Err(PeerAncestorPlanError::NoAuthorizedRoots);
    }
    let mut seen = BTreeSet::new();
    let mut validated = Vec::with_capacity(roots.len());
    for root in roots {
        if !root.is_absolute() {
            return Err(PeerAncestorPlanError::InvalidRoot(root.clone()));
        }
        let canonical = PathRef::from_canonical_native_absolute(root)
            .map_err(|_| PeerAncestorPlanError::InvalidRoot(root.clone()))?;
        let canonical = PathBuf::from(canonical.as_str());
        let key = path_key(&canonical);
        if !seen.insert(key) {
            return Err(PeerAncestorPlanError::DuplicateRoot(root.clone()));
        }
        crate::fs::VerifiedDir::open_absolute_components(root, None)
            .map_err(|_| PeerAncestorPlanError::UnreachableRoot(root.clone()))?;
        validated.push(canonical);
    }
    Ok(validated)
}

fn inspect_parent_chain(
    endpoint: &Path,
    root: &Path,
    operation: &Operation,
) -> Result<Vec<PathBuf>, PeerAncestorPlanError> {
    let suffix = relative_components(endpoint, root)?;
    if suffix.is_empty() {
        return Err(PeerAncestorPlanError::EndpointOverlapsRoot(
            endpoint.to_string_lossy().into_owned(),
        ));
    }
    let parent_components = &suffix[..suffix.len() - 1];
    let mut authority = crate::fs::VerifiedDir::open_absolute_components(root, None)
        .map_err(|_| PeerAncestorPlanError::UnreachableRoot(root.to_owned()))?;
    let mut current = root.to_owned();
    let mut missing = Vec::new();

    for (index, component) in parent_components.iter().enumerate() {
        let leaf = crate::fs::LeafName::parse(component.as_os_str())
            .map_err(|_| PeerAncestorPlanError::UnsafeParent(current.clone()))?;
        let candidate = current.join(component);
        let observed = authority
            .observe_leaf(&leaf)
            .map_err(|_| PeerAncestorPlanError::UnsafeParent(candidate.clone()))?;
        match observed {
            None if matches!(operation, Operation::Create) => {
                missing.extend(parent_components[index..].iter().scan(
                    current,
                    |path, component| {
                        *path = path.join(component);
                        Some(path.clone())
                    },
                ));
                break;
            }
            None => return Err(PeerAncestorPlanError::MissingParent(candidate)),
            Some(observation) if observation.is_directory() => {
                authority = crate::fs::VerifiedDir::open_absolute_components(&candidate, None)
                    .map_err(|_| PeerAncestorPlanError::UnsafeParent(candidate.clone()))?;
                current = candidate;
            }
            Some(_) => return Err(PeerAncestorPlanError::UnsafeParent(candidate)),
        }
    }
    Ok(missing)
}

fn parent_paths(endpoint: &Path, root: &Path) -> Result<Vec<PathBuf>, PeerAncestorPlanError> {
    let suffix = relative_components(endpoint, root)?;
    if suffix.is_empty() {
        return Err(PeerAncestorPlanError::EndpointOverlapsRoot(
            endpoint.to_string_lossy().into_owned(),
        ));
    }
    let mut current = root.to_owned();
    Ok(suffix[..suffix.len() - 1]
        .iter()
        .map(|component| {
            current.push(component);
            current.clone()
        })
        .collect())
}

fn relative_components(
    endpoint: &Path,
    root: &Path,
) -> Result<Vec<std::ffi::OsString>, PeerAncestorPlanError> {
    let mut endpoint_components = endpoint.components();
    for root_component in root.components() {
        match endpoint_components.next() {
            Some(endpoint_component) if components_match(&root_component, &endpoint_component) => {}
            _ => {
                return Err(PeerAncestorPlanError::OutsideAuthorizedRoot(
                    endpoint.to_string_lossy().into_owned(),
                ));
            }
        }
    }
    let mut suffix = Vec::new();
    for component in endpoint_components {
        match component {
            Component::Normal(component) => suffix.push(component.to_os_string()),
            _ => {
                return Err(PeerAncestorPlanError::InvalidEndpointPath(
                    endpoint.to_string_lossy().into_owned(),
                ));
            }
        }
    }
    Ok(suffix)
}

fn path_key(path: &Path) -> String {
    let mut key = path.to_string_lossy().replace('\\', "/");
    while key.len() > 1 && key.ends_with('/') {
        if cfg!(windows) && key.len() == 3 && key.as_bytes()[1] == b':' {
            break;
        }
        key.pop();
    }
    if cfg!(windows) {
        renderpilot_domain::normalized_path_key(&key)
    } else {
        key
    }
}

fn is_within_root(path: &Path, root: &Path) -> bool {
    let mut path = path.components();
    let mut root = root.components();
    loop {
        match (path.next(), root.next()) {
            (Some(p), Some(r)) => {
                if !components_match(&r, &p) {
                    return false;
                }
            }
            (_, None) => return true,
            (None, Some(_)) => return false,
        }
    }
}

fn same_path_components(left: &Path, right: &Path) -> bool {
    let mut left = left.components();
    let mut right = right.components();
    loop {
        match (left.next(), right.next()) {
            (Some(l), Some(r)) => {
                if !components_match(&l, &r) {
                    return false;
                }
            }
            (None, None) => return true,
            _ => return false,
        }
    }
}

fn components_match(left: &Component<'_>, right: &Component<'_>) -> bool {
    if cfg!(windows) {
        renderpilot_domain::normalized_path_key(&left.as_os_str().to_string_lossy())
            == renderpilot_domain::normalized_path_key(&right.as_os_str().to_string_lossy())
    } else {
        left.as_os_str() == right.as_os_str()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PeerAncestorPlanError {
    NoAuthorizedRoots,
    InvalidRoot(PathBuf),
    DuplicateRoot(PathBuf),
    UnreachableRoot(PathBuf),
    EvidenceCardinality { expected: usize, actual: usize },
    InvalidEndpointPath(String),
    OutsideAuthorizedRoot(String),
    EndpointOverlapsRoot(String),
    EndpointAncestorOverlap(PathBuf),
    BeforeObservationMismatch(String),
    InvalidEndpointTransition,
    MissingParent(PathBuf),
    UnsafeParent(PathBuf),
}

impl fmt::Display for PeerAncestorPlanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl Error for PeerAncestorPlanError {}

#[cfg(test)]
mod tests;
