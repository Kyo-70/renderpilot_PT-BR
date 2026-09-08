use super::super::super::peer_conversion::ensure_exact_luma_out_managed_to_native;
use super::super::super::*;
use super::super::PeerTopologyDirection;
use super::super::relocation::peer_topology_direction;
use super::context::{BindingContext, ReusedAggregateRole};

pub(super) fn apply_peer_relocation(context: &mut BindingContext<'_>) -> AppResult<()> {
    let Some(relocation) = context.peer.peer_relocation else {
        return Ok(());
    };
    let direction = peer_topology_direction(context.before_topology, context.after_topology);
    if relocation.direction != direction {
        return Err(renderpilot_application::AppError::invalid_input(
            "OptiScaler peer relocation direction does not match its topology transition",
        ));
    }
    let luma_out = direction == PeerTopologyDirection::OutOfOptiTopology
        && matches!(
            context.peer.mutation,
            OptiScalerPeerMutation::Replace { before, .. } if before.kind() == AddonKind::Luma
        );
    let luma_native_projection = matches!(
        context.peer.mutation,
        OptiScalerPeerMutation::Replace { before, after }
            if before.kind() == AddonKind::Luma
                && after.kind() == AddonKind::Luma
                && before.managed_files().iter().any(|file| {
                    normalized_path_key(file.path().as_str())
                        == normalized_path_key(relocation.source.as_str())
                })
                && !after.managed_files().iter().any(|file| {
                    let key = normalized_path_key(file.path().as_str());
                    key == normalized_path_key(relocation.source.as_str())
                        || key == normalized_path_key(relocation.destination.as_str())
                })
    );
    if luma_native_projection && !luma_out {
        return Err(renderpilot_application::AppError::invalid_input(
            "Luma managed-to-native peer conversion is valid only when leaving OptiScaler topology",
        ));
    }
    if luma_out {
        let OptiScalerPeerMutation::Replace { before, after } = context.peer.mutation else {
            return Err(renderpilot_application::AppError::invalid_input(
                "Luma OptiScaler uninstall peer relocation requires an exact peer replacement",
            ));
        };
        ensure_exact_luma_out_managed_to_native(before, after, relocation)?;
    }
    let source_key = normalized_path_key(relocation.source.as_str());
    let destination_key = normalized_path_key(relocation.destination.as_str());
    let source = context
        .before_receipts
        .get(&source_key)
        .map(|(_, receipt)| receipt)
        .or_else(|| {
            context
                .after_receipts
                .get(&destination_key)
                .map(|(_, receipt)| receipt)
        })
        .ok_or_else(|| {
            renderpilot_application::AppError::invalid_input(
                "OptiScaler relocation source has no exact receipt",
            )
        })?;
    let destination = if direction == PeerTopologyDirection::OutOfOptiTopology {
        // The old OptiScaler outer at the return slot is a different file.
        // Its deletion transition restores the exact peer source receipt;
        // never compare that source against the outer's proxy digest.
        source
    } else {
        context
            .after_receipts
            .get(&destination_key)
            .map(|(_, receipt)| receipt)
            .or_else(|| {
                context
                    .before_receipts
                    .get(&source_key)
                    .map(|(_, receipt)| receipt)
            })
            .ok_or_else(|| {
                renderpilot_application::AppError::invalid_input(
                    "OptiScaler relocation destination has no exact receipt",
                )
            })?
    };
    if source.digest() != &relocation.sha256
        || destination.digest() != &relocation.sha256
        || !same_receipt_identity_digest_ownership(source, destination)
    {
        return Err(renderpilot_application::AppError::invalid_input(
            "OptiScaler relocation changes receipt identity, digest, or ownership",
        ));
    }
    let source_transition = match context.after_receipts.get(&source_key) {
        Some((_, installed)) if installed != source => {
            if installed.ownership() != FileOwnership::Owned {
                return Err(renderpilot_application::AppError::invalid_input(
                    "OptiScaler relocated root claim must carry Owned custody",
                ));
            }
            pending_file_mutations::OptiScalerBoundTransition::RelocateThenClaim {
                source: source.clone(),
                destination: destination.clone(),
                installed: installed.clone(),
            }
        }
        _ => pending_file_mutations::OptiScalerBoundTransition::Relocate {
            source: source.clone(),
            destination: destination.clone(),
        },
    };
    insert_path(
        &mut context.paths,
        relocation.source.as_str(),
        source_transition,
    )?;

    if let OptiScalerPeerMutation::Replace { before, after } = context.peer.mutation {
        let before_sidecars = peer_owned_sidecar_path_map(Some(before));
        let after_sidecars = peer_owned_sidecar_path_map(Some(after));
        let source_sidecar = format!("{}.bak", relocation.source.as_str());
        let destination_sidecar = format!("{}.bak", relocation.destination.as_str());
        let source_sidecar_key = normalized_path_key(&source_sidecar);
        let destination_sidecar_key = normalized_path_key(&destination_sidecar);
        if luma_out {
            let source_digest = before_sidecars
                .get(&source_sidecar_key)
                .map(|(_, digest)| digest.clone());
            let destination_claimed = after
                .backed_up_files()
                .iter()
                .any(|path| normalized_path_key(path.as_str()) == destination_key);
            match (
                source_digest,
                destination_claimed,
                before_sidecars.get(&destination_sidecar_key),
                after_sidecars.get(&source_sidecar_key),
                after_sidecars.get(&destination_sidecar_key),
            ) {
                (Some(source_digest), true, None, None, None) => {
                    let transition =
                        pending_file_mutations::OptiScalerBoundTransition::RelocatePeerBaseline {
                            digest: source_digest,
                        };
                    insert_path(&mut context.paths, &source_sidecar, transition.clone())?;
                    insert_path(&mut context.paths, &destination_sidecar, transition)?;
                    context.relocation_keys.insert(source_sidecar_key);
                    context.relocation_keys.insert(destination_sidecar_key);
                }
                (None, false, None, None, None) => {}
                _ => {
                    return Err(renderpilot_application::AppError::invalid_input(
                        "Luma reverse peer sidecar closure changed its exact baseline path or custody",
                    ));
                }
            }
        } else {
            match (
                before_sidecars.get(&source_sidecar_key),
                before_sidecars.get(&destination_sidecar_key),
                after_sidecars.get(&source_sidecar_key),
                after_sidecars.get(&destination_sidecar_key),
            ) {
                (Some((_, source_digest)), None, None, Some((_, destination_digest)))
                    if source_digest == destination_digest =>
                {
                    let transition =
                        pending_file_mutations::OptiScalerBoundTransition::RelocatePeerBaseline {
                            digest: source_digest.clone(),
                        };
                    insert_path(&mut context.paths, &source_sidecar, transition.clone())?;
                    insert_path(&mut context.paths, &destination_sidecar, transition)?;
                    context.relocation_keys.insert(source_sidecar_key);
                    context.relocation_keys.insert(destination_sidecar_key);
                }
                (None, None, None, None) => {}
                _ => {
                    return Err(renderpilot_application::AppError::invalid_input(
                        "OptiScaler peer sidecar relocation changed its exact baseline path or digest",
                    ));
                }
            }
        }
    }

    let destination_transition = match context.before_receipts.get(&destination_key) {
        Some((_, prior)) if prior.ownership() == FileOwnership::Owned => {
            pending_file_mutations::OptiScalerBoundTransition::RemoveOwnedFile {
                installed: prior.clone(),
                restoration: Some(destination.clone()),
                allow_absent: true,
            }
        }
        Some((_, prior))
            if prior.ownership() == FileOwnership::Reused
                && context.reused_role(&destination_key)
                    == Some(ReusedAggregateRole::TopologyOuter) =>
        {
            pending_file_mutations::OptiScalerBoundTransition::RemoveReusedFile {
                installed: prior.clone(),
                restoration: Some(destination.clone()),
                allow_absent: true,
            }
        }
        Some((_, _)) => {
            return Err(renderpilot_application::AppError::invalid_input(
                "OptiScaler relocation destination is not a removable outer receipt",
            ));
        }
        None => pending_file_mutations::OptiScalerBoundTransition::Relocate {
            source: source.clone(),
            destination: destination.clone(),
        },
    };
    insert_path(
        &mut context.paths,
        relocation.destination.as_str(),
        destination_transition,
    )?;
    context.relocation_keys.insert(source_key);
    context.relocation_keys.insert(destination_key);
    Ok(())
}
