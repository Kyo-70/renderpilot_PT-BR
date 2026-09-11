use std::path::Path;

use renderpilot_domain::{AddonKind, InstalledAddon};

use crate::addons::renodx::peer::RenoDxActiveUpdateComposition;
use crate::addons::renodx::use_cases::commands::update::active::{
    ActiveLoweredUpdate, ActiveUpdatePhase1,
};
use crate::{Context, ServiceError};

pub(super) struct ActiveOrdinaryUpdateCommit<'a> {
    pub(super) context: &'a Context,
    pub(super) game_id: &'a renderpilot_domain::GameId,
    pub(super) safety: &'a crate::GameMutationSafetyPermits,
    pub(super) guard: crate::game_mutation_lock::GameMutationGuard,
    pub(super) phase1: ActiveUpdatePhase1,
    pub(super) phase3: ActiveUpdatePhase1,
    pub(super) lowered: ActiveLoweredUpdate,
}

pub(super) fn commit(
    ActiveOrdinaryUpdateCommit {
        context,
        game_id,
        safety,
        guard,
        phase1,
        phase3,
        lowered,
    }: ActiveOrdinaryUpdateCommit<'_>,
) -> Result<InstalledAddon, ServiceError> {
    let composition = lowered.composition();
    match composition {
        RenoDxActiveUpdateComposition::Noop => Ok(phase3.record().clone()),
        RenoDxActiveUpdateComposition::Metadata(metadata) => {
            let after = metadata.after_peer().clone();
            context.peer_mutation_executor().commit_metadata_aggregate(
                &guard,
                phase1.record(),
                &after,
                Some(phase3.topology()),
            )?;
            stamp_addon_mtime(&phase3, lowered.addon_mtime());
            Ok(after)
        }
        RenoDxActiveUpdateComposition::Physical(physical) => {
            let after = physical.after_peer().clone();
            let before_topology = phase1.topology().clone();
            let planned_topology = physical.planned_topology().clone();
            let game_root = phase3.root_seal().canonical_game_root().to_path_buf();
            let payload_root = phase3.root_seal().payload_root().map(Path::to_path_buf);
            crate::FileSafetyAuthority::new().authorize_game_commit(
                context,
                crate::addons::mutation_features::RENODX_UPDATE,
                &guard,
                safety.game(),
                || {
                    let package =
                        crate::addons::peer_lifecycle::package::PeerMutationPackage::plan_active(
                            crate::addons::peer_lifecycle::package::PeerMutationRequest {
                                peer_kind: AddonKind::RenoDx,
                                before_peer: Some(phase1.record()),
                                after_peer: Some(&after),
                                before_topology: &before_topology,
                                planned_after_topology: &planned_topology,
                                program: physical.program().clone(),
                                payloads: physical.payloads().to_vec(),
                                game_root,
                                payload_root,
                                component_set: None,
                                baseline_mutations: &[],
                                catalog_claim: None,
                            },
                        )?;
                    let prepared = context
                        .peer_mutation_executor()
                        .prepare_ordinary_file_peer(
                            context,
                            &guard,
                            crate::addons::mutation_features::RENODX_UPDATE,
                            Some(game_id.as_str()),
                            package,
                        )?;
                    let mut changes = crate::addons::engine::InstallChanges::default();
                    let applied = prepared.apply(&mut changes)?;
                    changes.sync_touched_dirs();
                    applied.commit()?;
                    stamp_addon_mtime(&phase3, lowered.addon_mtime());
                    Ok(after)
                },
            )
        }
    }
}

fn stamp_addon_mtime(phase3: &ActiveUpdatePhase1, mtime: Option<&str>) {
    if let Some(mtime) = mtime {
        crate::fs::stamp_mtime_best_effort(
            Path::new(phase3.addon_path().as_str()),
            Some(mtime),
            None,
        );
    }
}
