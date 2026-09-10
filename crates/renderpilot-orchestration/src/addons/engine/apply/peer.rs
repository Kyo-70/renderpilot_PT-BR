//! Exact peer topology planning and endpoint application.

use std::path::Path;

use renderpilot_domain::{AddonKind, PathRef, Sha256Hash, normalized_path_key};

use super::super::errors::invalid;
use crate::ServiceError;
use crate::addons::engine::{Action, InstallChanges, PeerMutationPlan};
use crate::peer_mutation_executor::{
    EndpointEvidence, EndpointExpectation, EndpointObservation, EndpointPostcondition,
    ExactEndpoint, ExactEndpointProgram, PeerAncestorAuthorities, PeerPathObservation,
    VerifiedPeerFile, observe_peer_path, observe_peer_path_state,
};

/// Immutable result of the peer topology preflight.
///
/// The endpoint program and its native O1 observations are captured together
/// before any durable reservation.  Consumers must carry this value through
/// preparation and apply; observing a second, independent "before" state is
/// not a substitute for the retained native identity proof.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PeerMutationPreflight {
    program: ExactEndpointProgram,
    before: Vec<EndpointObservation>,
}

impl PeerMutationPreflight {
    pub(crate) fn program(&self) -> &ExactEndpointProgram {
        &self.program
    }

    pub(crate) fn before(&self) -> &[EndpointObservation] {
        &self.before
    }

    pub(crate) fn paths(&self) -> impl Iterator<Item = &PathRef> {
        self.program.endpoints().iter().map(ExactEndpoint::path)
    }

    /// Verifies that the plan has exactly the program captured by this
    /// preflight.  The comparison includes endpoint order, roles, paths and
    /// native pre/post contracts.
    pub(crate) fn matches_plan(&self, plan: &PeerMutationPlan) -> bool {
        self.program == *plan.program()
            && self.before.len() == plan.program().endpoints().len()
            && plan
                .program()
                .endpoints()
                .iter()
                .zip(self.before.iter())
                .all(|(endpoint, before)| verify_peer_expectation(endpoint, before).is_ok())
    }

    /// Rechecks the live endpoints against the exact retained O1 observations
    /// immediately before a caller's first write.
    pub(crate) fn verify_current(&self) -> Result<(), ServiceError> {
        for (endpoint, expected) in self.program.endpoints().iter().zip(self.before.iter()) {
            let actual = observe_peer_path(endpoint.path())?;
            if &actual != expected {
                return Err(crate::failed(format!(
                    "peer endpoint preflight drifted before apply: {}",
                    endpoint.path().as_str()
                )));
            }
            verify_peer_expectation(endpoint, &actual)?;
        }
        Ok(())
    }

    /// Validates a caller-provided path list against the flat ordered program.
    /// This is used by durable engines that must not silently broaden or
    /// reorder the endpoint set after preflight.
    pub(crate) fn matches_paths<I>(&self, paths: I) -> bool
    where
        I: IntoIterator<Item = PathRef>,
    {
        let actual: Vec<String> = paths
            .into_iter()
            .map(|path| normalized_path_key(path.as_str()))
            .collect();
        let expected: Vec<String> = self
            .paths()
            .map(|path| normalized_path_key(path.as_str()))
            .collect();
        actual == expected
    }
}

/// Performs the unlocked, no-follow preflight required by a topology route.
///
/// The caller must run this before reserving a durable row.  In particular,
/// an absent nested parent is a topology conflict, not permission to create a
/// new directory as an incidental side effect of the route.
pub(crate) fn preflight_peer_mutation(
    plan: &PeerMutationPlan,
    peer_kind: AddonKind,
) -> Result<PeerMutationPreflight, ServiceError> {
    let preimages = collect_peer_preimages(plan)
        .map_err(|_| ServiceError::peer_topology_conflict(peer_kind))?;
    for (endpoint, observed) in plan.program().endpoints().iter().zip(preimages.iter()) {
        verify_peer_expectation(endpoint, observed)
            .map_err(|_| ServiceError::peer_topology_conflict(peer_kind))?;
    }
    Ok(PeerMutationPreflight {
        program: plan.program().clone(),
        before: preimages,
    })
}

/// Applies an exact peer endpoint program without creating a live rollback
/// sidecar.  All endpoint preimages are checked before the first write; the
/// supplied [`InstallChanges`] is the synchronous journal and its actions are
/// reversible without `.bak` files.
pub(crate) fn apply_peer_mutation(
    plan: &PeerMutationPlan,
    preflight: &PeerMutationPreflight,
    ancestor_authorities: &PeerAncestorAuthorities,
    changes: &mut InstallChanges,
) -> Result<Vec<EndpointEvidence>, ServiceError> {
    if !preflight.matches_plan(plan) {
        return Err(crate::failed(
            "peer apply plan differs from its immutable preflight",
        ));
    }
    let before = preflight.before();

    let mut evidence = Vec::with_capacity(plan.program().endpoints().len());
    for ((endpoint, payload), observed) in plan
        .program()
        .endpoints()
        .iter()
        .zip(plan.payloads())
        .zip(before.iter())
    {
        let current = match ancestor_authorities
            .observe_created_endpoint(Path::new(endpoint.path().as_str()))?
        {
            Some(current) => current,
            None => observe_peer_path_state(endpoint.path())?,
        };
        verify_exact_peer_observation(endpoint, observed, &current)?;
        let after = apply_peer_endpoint(endpoint, payload.as_deref(), current, changes)?;
        verify_peer_postcondition(endpoint, &after)?;
        evidence.push(EndpointEvidence::new(
            endpoint.role(),
            endpoint.path().clone(),
            observed,
            after,
        ));
    }
    Ok(evidence)
}

fn collect_peer_preimages(
    plan: &PeerMutationPlan,
) -> Result<Vec<EndpointObservation>, ServiceError> {
    let mut preimages = Vec::with_capacity(plan.program().endpoints().len());
    for endpoint in plan.program().endpoints() {
        match observe_peer_path_state(endpoint.path())? {
            crate::peer_mutation_executor::PeerPathObservation::Absent { .. } => {
                preimages.push(EndpointObservation::Absent);
            }
            crate::peer_mutation_executor::PeerPathObservation::File {
                bytes, observation, ..
            } => {
                let digest = observation.digest.clone().ok_or_else(|| {
                    crate::failed(format!(
                        "peer preflight file has no digest: {}",
                        endpoint.path().as_str()
                    ))
                })?;
                let digest =
                    Sha256Hash::new(digest).map_err(|error| crate::failed(error.to_string()))?;
                let length = bytes.len() as u64;
                let file = VerifiedPeerFile::new_with_length(observation.identity, digest, length)
                    .map_err(|error| crate::failed(error.to_string()))?;
                preimages.push(EndpointObservation::File(file));
            }
        }
    }
    Ok(preimages)
}

fn verify_peer_expectation(
    endpoint: &ExactEndpoint,
    actual: &EndpointObservation,
) -> Result<(), ServiceError> {
    match (endpoint.before(), actual) {
        (EndpointExpectation::Absent, EndpointObservation::Absent) => Ok(()),
        (EndpointExpectation::File(expected), EndpointObservation::File(actual))
            if expected.identity() == actual.identity && expected.digest() == &actual.digest =>
        {
            Ok(())
        }
        _ => Err(invalid(format!(
            "peer endpoint preimage changed before apply: {}",
            endpoint.path().as_str()
        ))),
    }
}

fn verify_peer_postcondition(
    endpoint: &ExactEndpoint,
    actual: &EndpointObservation,
) -> Result<(), ServiceError> {
    match (endpoint.after(), actual) {
        (EndpointPostcondition::Absent, EndpointObservation::Absent) => Ok(()),
        (EndpointPostcondition::File(expected), EndpointObservation::File(actual))
            if expected == &actual.digest =>
        {
            Ok(())
        }
        _ => Err(invalid(format!(
            "peer endpoint postimage mismatch: {}",
            endpoint.path().as_str()
        ))),
    }
}

fn verify_exact_peer_observation(
    endpoint: &ExactEndpoint,
    expected: &EndpointObservation,
    actual: &PeerPathObservation,
) -> Result<(), ServiceError> {
    let matches = match (expected, actual) {
        (EndpointObservation::Absent, PeerPathObservation::Absent { .. }) => true,
        (
            EndpointObservation::File(expected),
            PeerPathObservation::File {
                bytes, observation, ..
            },
        ) => {
            let Ok(length) = u64::try_from(bytes.len()) else {
                return Err(invalid(format!(
                    "peer endpoint length overflow: {}",
                    endpoint.path().as_str()
                )));
            };
            observation.kind == crate::fs::EntryKind::File
                && observation.identity == expected.identity()
                && observation.digest.as_deref() == Some(expected.digest().as_str())
                && length == expected.length()
        }
        _ => false,
    };
    if matches {
        Ok(())
    } else {
        Err(invalid(format!(
            "peer endpoint exact observation drifted before apply: {}",
            endpoint.path().as_str()
        )))
    }
}

fn peer_path_observation_to_endpoint(
    observed: &PeerPathObservation,
) -> Result<EndpointObservation, ServiceError> {
    match observed {
        PeerPathObservation::Absent { .. } => Ok(EndpointObservation::Absent),
        PeerPathObservation::File {
            bytes, observation, ..
        } => entry_observation_to_endpoint(observation, bytes.len()),
    }
}

fn entry_observation_to_endpoint(
    observation: &crate::fs::EntryObservation,
    length: usize,
) -> Result<EndpointObservation, ServiceError> {
    let digest = observation
        .digest
        .as_deref()
        .ok_or_else(|| invalid("peer endpoint observation has no regular-file digest"))?;
    let digest =
        renderpilot_domain::Sha256Hash::new(digest).map_err(|error| invalid(error.to_string()))?;
    let length = u64::try_from(length)
        .map_err(|error| invalid(format!("peer endpoint length overflow: {error}")))?;
    let file = crate::peer_mutation_executor::VerifiedPeerFile::new_with_length(
        observation.identity.clone(),
        digest,
        length,
    )
    .map_err(|error| invalid(error.to_string()))?;
    Ok(EndpointObservation::File(file))
}

fn apply_peer_endpoint(
    endpoint: &ExactEndpoint,
    payload: Option<&[u8]>,
    current: PeerPathObservation,
    changes: &mut InstallChanges,
) -> Result<EndpointObservation, ServiceError> {
    let path = Path::new(endpoint.path().as_str());
    match (current, endpoint.after(), payload) {
        (
            PeerPathObservation::Absent {
                parent: retained_parent,
            },
            EndpointPostcondition::File(_),
            Some(bytes),
        ) => {
            let (parent, leaf) = retained_parent.ok_or_else(|| {
                crate::failed(format!(
                    "peer endpoint parent authority was not retained: {}",
                    path.display()
                ))
            })?;
            let expected_post = create_peer_file_no_replace(&parent, &leaf, path, bytes)?;
            let endpoint = entry_observation_to_endpoint(&expected_post, bytes.len())?;
            changes.actions.push(Action::PeerCreated {
                path: path.to_path_buf(),
                expected_post,
                parent,
                leaf,
            });
            Ok(endpoint)
        }
        (
            PeerPathObservation::File {
                parent,
                leaf,
                bytes: original_bytes,
                observation,
            },
            EndpointPostcondition::File(_),
            Some(bytes),
        ) => {
            // This is intentionally an identity-bound in-place update.  A
            // plain retained-parent rename would still replace whichever
            // object currently occupies the leaf; it is not an identity CAS.
            // The durable transaction supplies before-image recovery, while
            // this operation preserves the observed directory-entry identity.
            // It is deliberately not advertised as atomically visible.
            let expected_post = parent.overwrite_regular_file(&leaf, &observation, bytes)?;
            let endpoint = entry_observation_to_endpoint(&expected_post, bytes.len())?;
            changes.actions.push(Action::PeerReplaced {
                path: path.to_path_buf(),
                original_bytes,
                expected_post,
                parent,
                leaf,
            });
            Ok(endpoint)
        }
        (
            PeerPathObservation::File {
                parent,
                leaf,
                bytes: original_bytes,
                observation,
            },
            EndpointPostcondition::Absent,
            None,
        ) => {
            parent.remove_exact(
                &leaf,
                &observation,
                crate::fs::AuthorityMode::CooperativeSameUid,
            )?;
            changes.actions.push(Action::PeerRemoved {
                path: path.to_path_buf(),
                original_bytes,
                parent,
                leaf,
            });
            Ok(EndpointObservation::Absent)
        }
        (observed @ PeerPathObservation::Absent { .. }, EndpointPostcondition::Absent, None)
        | (observed @ PeerPathObservation::File { .. }, EndpointPostcondition::File(_), None) => {
            peer_path_observation_to_endpoint(&observed)
        }
        _ => Err(invalid(format!(
            "peer endpoint operation/payload mismatch: {}",
            endpoint.path().as_str()
        ))),
    }
}

fn create_peer_file_no_replace(
    parent: &crate::fs::VerifiedDir,
    leaf: &crate::fs::LeafName,
    path: &Path,
    bytes: &[u8],
) -> Result<crate::fs::EntryObservation, ServiceError> {
    match parent.create_file_no_replace(
        leaf,
        bytes,
        crate::fs::AuthorityMode::CooperativeSameUid,
    )? {
        crate::fs::CreateFileNoReplace::Created { observation, .. }
            if observation.kind == crate::fs::EntryKind::File =>
        {
            Ok(observation)
        }
        crate::fs::CreateFileNoReplace::Created { .. } => Err(invalid(format!(
            "peer endpoint is not a regular file after creation: {}",
            path.display()
        ))),
        crate::fs::CreateFileNoReplace::Occupied => Err(invalid(format!(
            "peer endpoint became occupied before creation: {}",
            path.display()
        ))),
    }
}
