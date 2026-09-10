use renderpilot_domain::{
    AddonKind, CoordinatedPeerOperation, FileOwnership, GameProxyTopology, PeerEndpointRole,
    PlannedGameProxyTopology, ProxyImplementation, ProxyPeerRoute, normalized_path_key,
};

use crate::ServiceError;
use crate::peer_mutation_executor::{
    EndpointExpectation, EndpointPostcondition, ExactEndpointProgram,
};

pub(crate) fn classify_active_route(
    peer_kind: AddonKind,
    before_topology: &GameProxyTopology,
    planned_after_topology: &PlannedGameProxyTopology,
    program: &ExactEndpointProgram,
) -> Result<ProxyPeerRoute, ServiceError> {
    if !matches!(peer_kind, AddonKind::Luma | AddonKind::RenoDx) {
        return Err(crate::failed(
            "active peer planner accepts only a Luma or RenoDX peer",
        ));
    }
    before_topology
        .validate()
        .map_err(|error| crate::failed(format!("invalid active proxy topology: {error}")))?;
    if before_topology.outer.implementation != ProxyImplementation::OptiScaler {
        return Err(crate::failed(
            "active peer planner requires an OptiScaler outer topology",
        ));
    }

    let topology_endpoints = program
        .endpoints()
        .iter()
        .filter(|endpoint| endpoint.role() == PeerEndpointRole::TopologyDownstream)
        .collect::<Vec<_>>();

    match planned_after_topology {
        PlannedGameProxyTopology::Exact(after) if after == before_topology => {
            if !topology_endpoints.is_empty() {
                return Err(crate::failed(
                    "unchanged active topology cannot carry a downstream endpoint transition",
                ));
            }
            Ok(ProxyPeerRoute::DurableDisjoint)
        }
        PlannedGameProxyTopology::ObservedOwnedDownstream {
            downstream_path,
            planned_sha256,
            ..
        } => {
            let Some(endpoint) = single_topology_endpoint(&topology_endpoints)? else {
                return Err(crate::failed(
                    "coordinated active topology requires exactly one downstream endpoint",
                ));
            };
            require_path(endpoint.path(), downstream_path)?;
            match (
                &before_topology.downstream,
                endpoint.before(),
                endpoint.after(),
            ) {
                (None, EndpointExpectation::Absent, EndpointPostcondition::File(digest)) => {
                    if digest != planned_sha256 {
                        return Err(crate::failed(
                            "coordinated create digest differs from planned downstream",
                        ));
                    }
                    Ok(ProxyPeerRoute::Coordinated(
                        CoordinatedPeerOperation::Create,
                    ))
                }
                (
                    Some(existing),
                    EndpointExpectation::File(before),
                    EndpointPostcondition::File(digest),
                ) if same_path(&existing.path, downstream_path)
                    && (existing.receipt.ownership() == FileOwnership::Owned
                        || (existing.receipt.ownership() == FileOwnership::Reused
                            && before.digest() == existing.receipt.digest()))
                    && digest == planned_sha256 =>
                {
                    Ok(ProxyPeerRoute::Coordinated(
                        CoordinatedPeerOperation::ReplaceSamePath,
                    ))
                }
                (Some(existing), _, _) if same_path(&existing.path, downstream_path) => {
                    Err(crate::failed(
                        "coordinated downstream replace has an invalid ownership or endpoint shape",
                    ))
                }
                (Some(_), _, _) => Err(crate::failed(
                    "coordinated downstream replace changed its physical path",
                )),
                (None, _, _) => Err(crate::failed(
                    "coordinated create has an invalid downstream endpoint shape",
                )),
            }
        }
        PlannedGameProxyTopology::Exact(after) if after.downstream.is_none() => {
            after.validate().map_err(|error| {
                crate::failed(format!("invalid planned active proxy topology: {error}"))
            })?;
            let Some(existing) = before_topology.downstream.as_ref() else {
                return Err(crate::failed(
                    "coordinated remove requires an existing downstream",
                ));
            };
            let Some(endpoint) = single_topology_endpoint(&topology_endpoints)? else {
                return Err(crate::failed(
                    "coordinated remove requires exactly one downstream endpoint",
                ));
            };
            require_path(endpoint.path(), &existing.path)?;
            if existing.receipt.ownership() != FileOwnership::Owned
                || !matches!(endpoint.before(), EndpointExpectation::File(_))
                || !matches!(
                    endpoint.after(),
                    EndpointPostcondition::Absent | EndpointPostcondition::File(_)
                )
            {
                return Err(crate::failed(
                    "coordinated remove requires an owned downstream file preimage and absent or file restoration postcondition",
                ));
            }
            Ok(ProxyPeerRoute::Coordinated(
                CoordinatedPeerOperation::Remove,
            ))
        }
        PlannedGameProxyTopology::Exact(_) => Err(crate::failed(
            "active peer topology changed in an unsupported shape",
        )),
    }
}

fn single_topology_endpoint<'a>(
    endpoints: &[&'a crate::peer_mutation_executor::ExactEndpoint],
) -> Result<Option<&'a crate::peer_mutation_executor::ExactEndpoint>, ServiceError> {
    match endpoints {
        [] => Ok(None),
        [endpoint] => Ok(Some(*endpoint)),
        _ => Err(crate::failed(
            "peer program contains multiple topology downstream endpoints",
        )),
    }
}

fn require_path(
    left: &renderpilot_domain::PathRef,
    right: &renderpilot_domain::PathRef,
) -> Result<(), ServiceError> {
    if same_path(left, right) {
        Ok(())
    } else {
        Err(crate::failed(format!(
            "peer topology endpoint path mismatch: {} vs {}",
            left.as_str(),
            right.as_str()
        )))
    }
}

fn same_path(left: &renderpilot_domain::PathRef, right: &renderpilot_domain::PathRef) -> bool {
    normalized_path_key(left.as_str()) == normalized_path_key(right.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::peer_mutation_executor::VerifiedPeerFile;
    use renderpilot_domain::{
        FileReceipt, GameId, PathRef, ProxyLink, ProxyRootPrestate, Sha256Hash,
    };

    fn hash(byte: char) -> Sha256Hash {
        let byte = match byte {
            'a'..='f' => byte,
            _ => 'f',
        };
        Sha256Hash::new(byte.to_string().repeat(64)).expect("hash")
    }

    fn topology(root: &std::path::Path, downstream: Option<(bool, &str)>) -> GameProxyTopology {
        let root_slot = path(&root.join("dxgi.dll"));
        let outer_digest = hash('o');
        let downstream = downstream.map(|(owned, name)| ProxyLink {
            implementation: ProxyImplementation::ReShade,
            path: path(&root.join(name)),
            receipt: if owned {
                FileReceipt::owned("reshade", hash('a')).expect("owned receipt")
            } else {
                FileReceipt::reused("reshade", hash('a')).expect("reused receipt")
            },
        });
        let downstream_origin = downstream.as_ref().map(|_| root_slot.clone());
        GameProxyTopology {
            id: "optiscaler:route-test".to_owned(),
            game_id: GameId::new("manual:route-test").expect("game id"),
            root_slot: root_slot.clone(),
            outer: ProxyLink {
                implementation: ProxyImplementation::OptiScaler,
                path: root_slot,
                receipt: FileReceipt::owned("optiscaler", outer_digest).expect("outer receipt"),
            },
            downstream,
            downstream_origin,
            root_prestate: ProxyRootPrestate::Absent,
        }
    }

    fn path(path: &std::path::Path) -> PathRef {
        PathRef::new(path.to_string_lossy().replace('\\', "/")).expect("path")
    }

    fn endpoint(
        path: &PathRef,
        before: EndpointExpectation,
        after: EndpointPostcondition,
        role: PeerEndpointRole,
    ) -> crate::peer_mutation_executor::ExactEndpoint {
        crate::peer_mutation_executor::ExactEndpoint::new(path.clone(), role, before, after)
    }

    fn program(
        endpoints: Vec<crate::peer_mutation_executor::ExactEndpoint>,
    ) -> ExactEndpointProgram {
        ExactEndpointProgram::new(endpoints).expect("program")
    }

    fn observed(before: &GameProxyTopology, path: &PathRef) -> PlannedGameProxyTopology {
        PlannedGameProxyTopology::ObservedOwnedDownstream {
            id: before.id.clone(),
            game_id: before.game_id.clone(),
            root_slot: before.root_slot.clone(),
            outer: before.outer.clone(),
            implementation: ProxyImplementation::ReShade,
            downstream_path: path.clone(),
            downstream_origin: before.root_slot.clone(),
            root_prestate: before.root_prestate,
            planned_sha256: hash('b'),
            planned_length: 2,
        }
    }

    #[test]
    fn classifies_create_replace_remove_and_disjoint_routes() {
        let root = tempfile::tempdir().expect("root");
        let before_outer = topology(root.path(), None);
        let host = path(&root.path().join("ReShade64.dll"));
        let create = program(vec![endpoint(
            &host,
            EndpointExpectation::Absent,
            EndpointPostcondition::File(hash('b')),
            PeerEndpointRole::TopologyDownstream,
        )]);
        assert_eq!(
            classify_active_route(
                AddonKind::Luma,
                &before_outer,
                &observed(&before_outer, &host),
                &create
            )
            .expect("create"),
            ProxyPeerRoute::Coordinated(CoordinatedPeerOperation::Create)
        );

        let before_owned = topology(root.path(), Some((true, "ReShade64.dll")));
        let replace = program(vec![endpoint(
            &host,
            EndpointExpectation::File(
                VerifiedPeerFile::new_with_length("reshade-id".to_owned(), hash('a'), 1)
                    .expect("before"),
            ),
            EndpointPostcondition::File(hash('b')),
            PeerEndpointRole::TopologyDownstream,
        )]);
        assert_eq!(
            classify_active_route(
                AddonKind::RenoDx,
                &before_owned,
                &observed(&before_owned, &host),
                &replace
            )
            .expect("replace"),
            ProxyPeerRoute::Coordinated(CoordinatedPeerOperation::ReplaceSamePath)
        );

        let after_outer = topology(root.path(), None);
        let remove = program(vec![endpoint(
            &host,
            EndpointExpectation::File(
                VerifiedPeerFile::new_with_length("reshade-id".to_owned(), hash('a'), 1)
                    .expect("before"),
            ),
            EndpointPostcondition::Absent,
            PeerEndpointRole::TopologyDownstream,
        )]);
        assert_eq!(
            classify_active_route(
                AddonKind::Luma,
                &before_owned,
                &PlannedGameProxyTopology::Exact(after_outer),
                &remove
            )
            .expect("remove"),
            ProxyPeerRoute::Coordinated(CoordinatedPeerOperation::Remove)
        );

        let restored = program(vec![endpoint(
            &host,
            EndpointExpectation::File(
                VerifiedPeerFile::new_with_length("reshade-id".to_owned(), hash('a'), 1)
                    .expect("before"),
            ),
            EndpointPostcondition::File(hash('b')),
            PeerEndpointRole::TopologyDownstream,
        )]);
        assert_eq!(
            classify_active_route(
                AddonKind::Luma,
                &before_owned,
                &PlannedGameProxyTopology::Exact(topology(root.path(), None)),
                &restored,
            )
            .expect("present-baseline restoration"),
            ProxyPeerRoute::Coordinated(CoordinatedPeerOperation::Remove)
        );

        let malformed_preimage = program(vec![endpoint(
            &host,
            EndpointExpectation::Absent,
            EndpointPostcondition::File(hash('b')),
            PeerEndpointRole::TopologyDownstream,
        )]);
        assert!(
            classify_active_route(
                AddonKind::Luma,
                &before_owned,
                &PlannedGameProxyTopology::Exact(topology(root.path(), None)),
                &malformed_preimage,
            )
            .is_err()
        );

        let disjoint = program(vec![endpoint(
            &path(&root.path().join("luma.addon")),
            EndpointExpectation::Absent,
            EndpointPostcondition::File(hash('c')),
            PeerEndpointRole::Disjoint,
        )]);
        assert_eq!(
            classify_active_route(
                AddonKind::Luma,
                &before_owned,
                &PlannedGameProxyTopology::Exact(before_owned.clone()),
                &disjoint,
            )
            .expect("disjoint"),
            ProxyPeerRoute::DurableDisjoint
        );
    }

    #[test]
    fn rejects_reused_downstream_wrong_path_and_multiple_downstreams() {
        let root = tempfile::tempdir().expect("root");
        let outer = topology(root.path(), None);
        let disjoint = program(vec![endpoint(
            &path(&root.path().join("strict.addon")),
            EndpointExpectation::Absent,
            EndpointPostcondition::File(hash('s')),
            PeerEndpointRole::Disjoint,
        )]);
        assert!(
            classify_active_route(
                AddonKind::OptiScaler,
                &outer,
                &PlannedGameProxyTopology::Exact(outer.clone()),
                &disjoint,
            )
            .is_err()
        );

        let reused = topology(root.path(), Some((false, "ReShade64.dll")));
        let host = path(&root.path().join("ReShade64.dll"));
        let replacement = program(vec![endpoint(
            &host,
            EndpointExpectation::File(
                VerifiedPeerFile::new_with_length("id".to_owned(), hash('a'), 1).expect("before"),
            ),
            EndpointPostcondition::File(hash('b')),
            PeerEndpointRole::TopologyDownstream,
        )]);
        assert_eq!(
            classify_active_route(
                AddonKind::Luma,
                &reused,
                &observed(&reused, &host),
                &replacement
            )
            .expect("reused same-path replacement"),
            ProxyPeerRoute::Coordinated(CoordinatedPeerOperation::ReplaceSamePath)
        );

        let wrong_reused_preimage = program(vec![endpoint(
            &host,
            EndpointExpectation::File(
                VerifiedPeerFile::new_with_length("id".to_owned(), hash('x'), 1).expect("before"),
            ),
            EndpointPostcondition::File(hash('b')),
            PeerEndpointRole::TopologyDownstream,
        )]);
        assert!(
            classify_active_route(
                AddonKind::Luma,
                &reused,
                &observed(&reused, &host),
                &wrong_reused_preimage
            )
            .is_err()
        );

        let owned = topology(root.path(), Some((true, "ReShade64.dll")));
        let wrong_path = path(&root.path().join("other.dll"));
        let wrong = program(vec![endpoint(
            &wrong_path,
            EndpointExpectation::File(
                VerifiedPeerFile::new_with_length("id".to_owned(), hash('a'), 1).expect("before"),
            ),
            EndpointPostcondition::File(hash('b')),
            PeerEndpointRole::TopologyDownstream,
        )]);
        assert!(
            classify_active_route(
                AddonKind::Luma,
                &owned,
                &observed(&owned, &wrong_path),
                &wrong
            )
            .is_err()
        );

        let second = path(&root.path().join("second.dll"));
        let multiple = program(vec![
            endpoint(
                &host,
                EndpointExpectation::Absent,
                EndpointPostcondition::File(hash('b')),
                PeerEndpointRole::TopologyDownstream,
            ),
            endpoint(
                &second,
                EndpointExpectation::Absent,
                EndpointPostcondition::File(hash('c')),
                PeerEndpointRole::TopologyDownstream,
            ),
        ]);
        assert!(
            classify_active_route(AddonKind::Luma, &outer, &observed(&outer, &host), &multiple)
                .is_err()
        );
    }
}
