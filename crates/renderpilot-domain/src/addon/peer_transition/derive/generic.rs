//! Derivation of generic created/backed peer claims.

use std::collections::{BTreeMap, BTreeSet};

use crate::PathRef;

use super::super::claims::{PeerSnapshot, RetainedGenericMutation, managed_sidecar_path};
use super::super::model::{PeerEndpointIntent, PeerEndpointRole, PeerTransitionError};
use super::super::reconcile::DerivedEndpoint;
use super::helpers::add_endpoint;

pub(super) type PhysicalProgramIndex = BTreeMap<String, usize>;

pub(super) fn index_physical_program(
    physical_program: &[PeerEndpointIntent],
) -> PhysicalProgramIndex {
    physical_program
        .iter()
        .enumerate()
        .map(|(index, intent)| (crate::normalized_path_key(intent.path().as_str()), index))
        .collect()
}

pub(super) fn derive(
    before: &PeerSnapshot,
    after: &PeerSnapshot,
    physical_program: &[PeerEndpointIntent],
    physical_index: &PhysicalProgramIndex,
    endpoints: &mut Vec<DerivedEndpoint>,
) -> Result<(), PeerTransitionError> {
    let backed_keys = before
        .backed
        .keys()
        .chain(after.backed.keys())
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    for key in backed_keys {
        let before_backed = before.backed.get(key);
        let after_backed = after.backed.get(key);
        let path = after_backed.or(before_backed).expect("union key exists");
        if before_backed.is_some() && !before.created.contains_key(key)
            || after_backed.is_some() && !after.created.contains_key(key)
        {
            return Err(PeerTransitionError::InvalidPeerSnapshot(
                "generic backed claim must retain its live claim",
            ));
        }
        let live_changed = before_backed.is_some() != after_backed.is_some()
            || before.created.contains_key(key) != after.created.contains_key(key);
        if live_changed {
            add_endpoint(
                endpoints,
                PeerEndpointIntent::replace(path.clone(), PeerEndpointRole::Disjoint, None, None)?,
                super::super::EndpointGuard::default(),
            );
        } else if before_backed.is_some()
            && let Some(index) = physical_index.get(key)
        {
            let physical = &physical_program[*index];
            if RetainedGenericMutation::from_stable_claim(before, after, physical).is_some() {
                add_endpoint(
                    endpoints,
                    physical.clone(),
                    super::super::EndpointGuard::default(),
                );
            }
        }
        if before_backed.is_some() != after_backed.is_some() {
            let sidecar = managed_sidecar_path(path)?;
            let intent = if after_backed.is_some() {
                PeerEndpointIntent::create(sidecar, PeerEndpointRole::Disjoint, None, None)?
            } else {
                PeerEndpointIntent::remove(sidecar, PeerEndpointRole::Disjoint)?
            };
            add_endpoint(endpoints, intent, super::super::EndpointGuard::default());
        }
    }

    let created_keys = before
        .created
        .keys()
        .chain(after.created.keys())
        .filter(|key| !before.backed.contains_key(*key) && !after.backed.contains_key(*key))
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    for key in created_keys {
        let before_created = before.created.get(key);
        let after_created = after.created.get(key);
        if before_created.is_some() == after_created.is_some() {
            if before_created.is_some()
                && let Some(index) = physical_index.get(key)
            {
                let physical = &physical_program[*index];
                if RetainedGenericMutation::from_stable_claim(before, after, physical).is_some() {
                    add_endpoint(
                        endpoints,
                        physical.clone(),
                        super::super::EndpointGuard::default(),
                    );
                }
            }
            continue;
        }
        let path: PathRef = after_created
            .or(before_created)
            .expect("union key exists")
            .clone();
        let intent = if after_created.is_some() {
            PeerEndpointIntent::create(path, PeerEndpointRole::Disjoint, None, None)?
        } else {
            PeerEndpointIntent::remove(path, PeerEndpointRole::Disjoint)?
        };
        add_endpoint(endpoints, intent, super::super::EndpointGuard::default());
    }
    Ok(())
}
