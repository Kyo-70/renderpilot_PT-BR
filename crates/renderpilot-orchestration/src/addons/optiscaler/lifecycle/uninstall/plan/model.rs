use super::*;

#[derive(Debug, Clone)]
pub(in crate::addons::optiscaler::lifecycle::uninstall) struct UninstallFsPlan {
    pub(in crate::addons::optiscaler::lifecycle::uninstall) precommit: Vec<UninstallStep>,
    pub(in crate::addons::optiscaler::lifecycle::uninstall) postcommit_directories:
        Vec<renderpilot_domain::OptiScalerDirectoryReceipt>,
    pub(in crate::addons::optiscaler::lifecycle::uninstall) peer_transition:
        Option<adoption::PeerHostTransitionPlan>,
    pub(in crate::addons::optiscaler::lifecycle::uninstall) preserved_paths: Vec<PathBuf>,
}

#[derive(Debug, Clone)]
pub(in crate::addons::optiscaler::lifecycle::uninstall) enum UninstallStep {
    CreateDirectory {
        path: PathBuf,
    },
    PreserveConfiguration {
        plan: ConfigPreservationPlan,
    },
    RestoreConfiguration {
        path: PathBuf,
        receipt: FileReceipt,
        bytes: Vec<u8>,
    },
    DeleteOwned {
        path: PathBuf,
        receipt: FileReceipt,
    },
    DeleteReusedArtifact {
        path: PathBuf,
        receipt: FileReceipt,
    },
    VerifyNoMutation {
        path: PathBuf,
        preimage: VerifyPreimage,
    },
    DeleteTopologyOuter {
        path: PathBuf,
        receipt: FileReceipt,
    },
    /// Restores the game-owned AMD FSR DLL retained in its deterministic
    /// original backup.  This is intentionally distinct from peer topology
    /// relocation: the source is external game provenance, never an add-on.
    RestoreRetainedFsrOriginal {
        original_backup: PathBuf,
        target: PathBuf,
        original: FileReceipt,
    },
    RelocatePeer {
        source: PathBuf,
        destination: PathBuf,
        receipt: FileReceipt,
    },
    RelocatePeerSidecar {
        source: PathBuf,
        destination: PathBuf,
        receipt: FileReceipt,
    },
}

#[derive(Debug, Clone)]
pub(in crate::addons::optiscaler::lifecycle::uninstall) enum VerifyPreimage {
    Exact(FileReceipt),
    ReusedConfiguration(FileReceipt),
    Absent,
}

impl UninstallStep {
    pub(in crate::addons::optiscaler::lifecycle::uninstall) fn operation(
        &self,
    ) -> crate::file_mutation::optiscaler::OptiScalerPlannedOperation {
        use crate::file_mutation::optiscaler::{
            OptiScalerPlannedOperation as Operation, PlannedParticipant, PlannedPreimage,
        };
        let exact = |path: &Path, receipt: &FileReceipt| PlannedParticipant {
            path: path.to_path_buf(),
            preimage: match receipt.ownership() {
                FileOwnership::Owned => PlannedPreimage::Exact {
                    current: receipt.clone(),
                    prior_owned: Some(receipt.clone()),
                },
                FileOwnership::Reused => PlannedPreimage::ExactReused {
                    current: receipt.clone(),
                    authority:
                        crate::file_mutation::optiscaler::ReusedMutationAuthority::ObservationOnly,
                },
            },
        };
        let verify = |path: &Path, preimage: &VerifyPreimage| {
            PlannedParticipant {
            path: path.to_path_buf(),
            preimage: match preimage {
                VerifyPreimage::Exact(receipt) => match receipt.ownership() {
                    FileOwnership::Owned => PlannedPreimage::Exact {
                        current: receipt.clone(),
                        prior_owned: None,
                    },
                    FileOwnership::Reused => PlannedPreimage::ExactReused {
                        current: receipt.clone(),
                        authority: crate::file_mutation::optiscaler::ReusedMutationAuthority::ObservationOnly,
                    },
                },
                VerifyPreimage::ReusedConfiguration(receipt) => PlannedPreimage::ExactReused {
                    current: receipt.clone(),
                    authority: crate::file_mutation::optiscaler::ReusedMutationAuthority::ObservationOnly,
                },
                VerifyPreimage::Absent => PlannedPreimage::Absent,
            },
        }
        };
        match self {
            Self::CreateDirectory { path } => Operation::CreateDirectory(PlannedParticipant {
                path: path.clone(),
                preimage: PlannedPreimage::Absent,
            }),
            Self::PreserveConfiguration { plan } => Operation::Write(PlannedParticipant {
                path: plan.destination.clone(),
                preimage: PlannedPreimage::Absent,
            }),
            Self::RestoreConfiguration { path, receipt, .. } => {
                Operation::Write(exact(path, receipt))
            }
            Self::DeleteOwned { path, receipt } => Operation::Delete(exact(path, receipt)),
            Self::DeleteReusedArtifact { path, receipt } => {
                Operation::Delete(PlannedParticipant {
                    path: path.clone(),
                    preimage: PlannedPreimage::ExactReused {
                        current: receipt.clone(),
                        authority: crate::file_mutation::optiscaler::ReusedMutationAuthority::OptiScalerArtifact,
                    },
                })
            }
            Self::DeleteTopologyOuter { path, receipt } => Operation::Delete(PlannedParticipant {
                path: path.clone(),
                preimage: match receipt.ownership() {
                    FileOwnership::Owned => PlannedPreimage::Exact {
                        current: receipt.clone(),
                        prior_owned: Some(receipt.clone()),
                    },
                    FileOwnership::Reused => PlannedPreimage::ExactReused {
                        current: receipt.clone(),
                        authority: crate::file_mutation::optiscaler::ReusedMutationAuthority::OptiScalerArtifact,
                    },
                },
            }),
            Self::VerifyNoMutation { path, preimage } => Operation::Verify(verify(path, preimage)),
            Self::RelocatePeer {
                source,
                destination,
                receipt,
            }
            | Self::RelocatePeerSidecar {
                source,
                destination,
                receipt,
            } => Operation::Relocate {
                source: PlannedParticipant {
                    path: source.clone(),
                    preimage: match receipt.ownership() {
                        FileOwnership::Owned => PlannedPreimage::Exact {
                            current: receipt.clone(),
                            prior_owned: Some(receipt.clone()),
                        },
                        FileOwnership::Reused => PlannedPreimage::ExactReused {
                            current: receipt.clone(),
                            authority: crate::file_mutation::optiscaler::ReusedMutationAuthority::RelocationSource,
                        },
                    },
                },
                destination: PlannedParticipant {
                    path: destination.clone(),
                    preimage: PlannedPreimage::Absent,
                },
            },
            Self::RestoreRetainedFsrOriginal {
                original_backup,
                target,
                original,
            } => Operation::Relocate {
                source: PlannedParticipant {
                    path: original_backup.clone(),
                    preimage: PlannedPreimage::ExactReused {
                        current: original.clone(),
                        authority: crate::file_mutation::optiscaler::ReusedMutationAuthority::RelocationSource,
                    },
                },
                destination: PlannedParticipant {
                    path: target.clone(),
                    preimage: PlannedPreimage::Absent,
                },
            },
        }
    }

    pub(in crate::addons::optiscaler::lifecycle::uninstall) fn mutation_paths(&self) -> Vec<&Path> {
        match self {
            Self::CreateDirectory { .. } | Self::VerifyNoMutation { .. } => Vec::new(),
            Self::PreserveConfiguration { plan } => vec![&plan.destination],
            Self::RestoreConfiguration { path, .. }
            | Self::DeleteOwned { path, .. }
            | Self::DeleteReusedArtifact { path, .. }
            | Self::DeleteTopologyOuter { path, .. } => vec![path],
            Self::RelocatePeer {
                source,
                destination,
                ..
            }
            | Self::RelocatePeerSidecar {
                source,
                destination,
                ..
            } => vec![source, destination],
            Self::RestoreRetainedFsrOriginal {
                original_backup,
                target,
                ..
            } => vec![original_backup, target],
        }
    }

    pub(in crate::addons::optiscaler::lifecycle::uninstall) fn touched_paths(&self) -> Vec<&Path> {
        match self {
            Self::CreateDirectory { path }
            | Self::DeleteOwned { path, .. }
            | Self::DeleteReusedArtifact { path, .. }
            | Self::DeleteTopologyOuter { path, .. }
            | Self::VerifyNoMutation { path, .. }
            | Self::RestoreConfiguration { path, .. } => vec![path],
            Self::PreserveConfiguration { plan } => vec![&plan.destination, &plan.source],
            Self::RelocatePeer {
                source,
                destination,
                ..
            }
            | Self::RelocatePeerSidecar {
                source,
                destination,
                ..
            } => vec![source, destination],
            Self::RestoreRetainedFsrOriginal {
                original_backup,
                target,
                ..
            } => vec![original_backup, target],
        }
    }
}
