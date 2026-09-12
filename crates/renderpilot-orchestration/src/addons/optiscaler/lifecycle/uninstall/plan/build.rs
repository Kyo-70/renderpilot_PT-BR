use super::*;

pub(in crate::addons::optiscaler::lifecycle::uninstall) fn build_uninstall_plan(
    state: &OptiScalerInstallState,
    topology: &GameProxyTopology,
    peer_transition: Option<adoption::PeerHostTransitionPlan>,
    other_claim_paths: &HashSet<String>,
) -> Result<UninstallFsPlan, ServiceError> {
    topology
        .validate()
        .map_err(|error| failed(format!("OptiScaler topology is invalid: {error}")))?;
    super::super::validate_release_file_receipt(state, topology)?;
    let mut mutations = Vec::new();
    let mut preserved_paths = Vec::new();
    let mut final_verifies = Vec::new();
    let mut recovery_dirs = Vec::new();
    let managed_root = Path::new(state.target_dir.as_str());
    for receipt in state
        .release_files
        .iter()
        .filter(|receipt| receipt.role == OptiScalerFileRole::Configuration)
        .chain(
            state
                .release_files
                .iter()
                .filter(|receipt| receipt.role != OptiScalerFileRole::Configuration),
        )
    {
        let path = PathBuf::from(receipt.path.as_str());
        let key = crate::paths::normalized_key(&path);
        if let Some((_, custody_path, original)) = receipt.baseline.retained_original() {
            if other_claim_paths.contains(&key) {
                return Err(failed(format!(
                    "OptiScaler retained AMD FSR target is claimed by another add-on: {}",
                    path.display()
                )));
            }
            let original_backup = PathBuf::from(custody_path.as_str());
            if other_claim_paths.contains(&crate::paths::normalized_key(&original_backup)) {
                return Err(failed(format!(
                    "OptiScaler retained AMD FSR original backup is claimed by another add-on: {}",
                    original_backup.display()
                )));
            }
            match observe_managed_receipt(managed_root, &path, &receipt.installed)? {
                ReceiptObservation::Exact(live) => mutations.push(UninstallStep::DeleteOwned {
                    path: path.clone(),
                    receipt: live,
                }),
                ReceiptObservation::Absent => mutations.push(UninstallStep::VerifyNoMutation {
                    path: path.clone(),
                    preimage: VerifyPreimage::Absent,
                }),
            }
            let original = require_receipt(&original_backup, original)?;
            mutations.push(UninstallStep::RestoreRetainedFsrOriginal {
                original_backup,
                target: path,
                original,
            });
            continue;
        }
        let observation = if receipt.role == OptiScalerFileRole::Configuration
            && receipt.installed.ownership() == FileOwnership::Reused
        {
            observe_managed_reused_configuration(managed_root, &path)?
        } else {
            observe_managed_receipt(managed_root, &path, &receipt.installed)?
        };
        if other_claim_paths.contains(&key) {
            preserve_observation(path, observation, &mut preserved_paths, &mut final_verifies);
            continue;
        }
        match (receipt.role, receipt.installed.ownership(), observation) {
            (
                OptiScalerFileRole::Configuration,
                FileOwnership::Reused,
                ReceiptObservation::Exact(live),
            ) => {
                preserved_paths.push(path.clone());
                final_verifies.push((path, VerifyPreimage::ReusedConfiguration(live)));
            }
            (
                OptiScalerFileRole::Configuration,
                FileOwnership::Owned,
                ReceiptObservation::Exact(live),
            ) => match state.configuration_baseline() {
                renderpilot_domain::OptiScalerConfigurationBaseline::Present {
                    bytes: baseline_bytes,
                    ..
                } => {
                    let preservation = ConfigPreservationPlan::for_game_with_receipt(
                        &state.game_id,
                        &path,
                        live.digest().clone(),
                        live.clone(),
                        true,
                    )?;
                    recovery_dirs.extend(missing_recovery_directories(&preservation.destination)?);
                    mutations.push(UninstallStep::PreserveConfiguration { plan: preservation });
                    mutations.push(UninstallStep::RestoreConfiguration {
                        path,
                        receipt: live,
                        bytes: baseline_bytes.clone(),
                    });
                }
                renderpilot_domain::OptiScalerConfigurationBaseline::Absent => {
                    mutations.push(UninstallStep::DeleteOwned {
                        path,
                        receipt: live,
                    });
                }
            },
            (
                OptiScalerFileRole::Runtime,
                FileOwnership::Owned,
                ReceiptObservation::Exact(live),
            ) => mutations.push(UninstallStep::DeleteOwned {
                path,
                receipt: live,
            }),
            (
                OptiScalerFileRole::Runtime,
                FileOwnership::Reused,
                ReceiptObservation::Exact(live),
            ) => mutations.push(UninstallStep::DeleteReusedArtifact {
                path,
                receipt: live,
            }),
            (_, _, ReceiptObservation::Absent) => {
                final_verifies.push((path, VerifyPreimage::Absent));
            }
        }
    }
    for binding in &state.runtime_bindings {
        let path = PathBuf::from(binding.path.as_str());
        let key = crate::paths::normalized_key(&path);
        let observation = observe_managed_receipt(managed_root, &path, &binding.installed)?;
        if other_claim_paths.contains(&key) {
            preserve_observation(path, observation, &mut preserved_paths, &mut final_verifies);
        } else {
            match observation {
                ReceiptObservation::Exact(live) => match live.ownership() {
                    FileOwnership::Owned => mutations.push(UninstallStep::DeleteOwned {
                        path,
                        receipt: live,
                    }),
                    FileOwnership::Reused => mutations.push(UninstallStep::DeleteReusedArtifact {
                        path,
                        receipt: live,
                    }),
                },
                ReceiptObservation::Absent => {
                    final_verifies.push((path, VerifyPreimage::Absent));
                }
            }
        }
    }

    let root = PathBuf::from(topology.root_slot.as_str());
    match observe_receipt(&root, &topology.outer.receipt)? {
        ReceiptObservation::Exact(outer_live) => {
            mutations.push(UninstallStep::DeleteTopologyOuter {
                path: root.clone(),
                receipt: outer_live,
            });
        }
        ReceiptObservation::Absent => mutations.push(UninstallStep::VerifyNoMutation {
            path: root.clone(),
            preimage: VerifyPreimage::Absent,
        }),
    }

    if let Some(downstream) = &topology.downstream {
        // The downstream is outside OptiScaler's file set.  It may nevertheless
        // be `Owned` by the RenoDX/Luma peer after that peer repaired its host
        // while OptiScaler was outermost.  In that case its record must be
        // relocated in the same aggregate; accepting the file move without the
        // corresponding peer transition would strand ownership at the old path.
        if downstream.receipt.ownership() == FileOwnership::Owned && peer_transition.is_none() {
            return Err(failed(
                "owned proxy downstream relocation requires a peer receipt transition",
            ));
        }
        let relocation = crate::addons::proxy_chain::planned_peer_host_relocation(topology)
            .ok_or_else(|| failed("proxy topology has no concrete downstream relocation"))?;
        let source = PathBuf::from(relocation.from.as_str());
        let destination = PathBuf::from(relocation.to.as_str());
        let source_live = require_receipt(&source, &downstream.receipt)?;
        if !crate::paths::same_path(&destination, &root)
            && maybe_exact_receipt_from_live(&destination, FileOwnership::Reused)?.is_some()
        {
            return Err(failed(format!(
                "cannot restore the downstream peer because its original slot is occupied: {}",
                destination.display()
            )));
        }
        if let Some(transition) = &peer_transition
            && (!crate::paths::same_path(Path::new(transition.from.as_str()), &source)
                || !crate::paths::same_path(Path::new(transition.to.as_str()), &destination)
                || transition.live_sha256 != *source_live.digest())
        {
            return Err(failed(
                "peer relocation changed while OptiScaler uninstall was being planned",
            ));
        }
        if let Some(transition) = &peer_transition
            && transition.destination_ownership() != downstream.receipt.ownership()
        {
            return Err(failed(
                "peer relocation custody differs from the persisted downstream receipt",
            ));
        }
        if !matches!(
            mutations.last(),
            Some(
                UninstallStep::DeleteTopologyOuter { path, .. }
                    | UninstallStep::VerifyNoMutation {
                        path,
                        preimage: VerifyPreimage::Absent,
                    },
            ) if crate::paths::same_path(path, &root)
        ) {
            return Err(failed(
                "OptiScaler outer absence must be established immediately before peer relocation",
            ));
        }
        mutations.push(UninstallStep::RelocatePeer {
            source,
            destination,
            receipt: source_live,
        });
        if let Some(transition) = &peer_transition
            && let Some(sidecar) = &transition.sidecar
        {
            mutations.push(UninstallStep::RelocatePeerSidecar {
                source: sidecar.source.clone(),
                destination: sidecar.destination.clone(),
                receipt: require_receipt(&sidecar.source, &sidecar.source_receipt)?,
            });
        }
    } else if topology.root_prestate != ProxyRootPrestate::Absent {
        return Err(failed(
            "proxy topology has a relocated root pre-state without a downstream peer",
        ));
    }

    let mut precommit = recovery_dirs
        .into_iter()
        .map(|path| UninstallStep::CreateDirectory { path })
        .collect::<Vec<_>>();
    precommit.extend(mutations);
    precommit.extend(
        final_verifies
            .into_iter()
            .map(|(path, preimage)| UninstallStep::VerifyNoMutation { path, preimage }),
    );
    validate_producer_registry(&precommit)?;
    let postcommit_directories = canonical_postcommit_directories(&state.directory_receipts)?;
    validate_directory_emptiness(&postcommit_directories, &precommit)?;
    Ok(UninstallFsPlan {
        precommit,
        postcommit_directories,
        peer_transition,
        preserved_paths,
    })
}

fn preserve_observation(
    path: PathBuf,
    observation: ReceiptObservation,
    preserved_paths: &mut Vec<PathBuf>,
    final_verifies: &mut Vec<(PathBuf, VerifyPreimage)>,
) {
    match observation {
        ReceiptObservation::Exact(live) => {
            preserved_paths.push(path.clone());
            final_verifies.push((path, VerifyPreimage::Exact(live)));
        }
        ReceiptObservation::Absent => final_verifies.push((path, VerifyPreimage::Absent)),
    }
}
