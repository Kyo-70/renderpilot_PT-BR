use std::{collections::HashMap, str};

use renderpilot_domain::PathRef;

use super::super::{
    dgvoodoo::{DgVoodooDecision, lower_dgvoodoo_decision},
    effects::{LumaPeerEffectAccumulator, ensure_bytes_match_image},
    snapshot_input::require_absent,
};
use super::{
    ActiveDgVoodooError,
    model::{
        ActiveDgVoodooPlan, ActiveDgVoodooRecordProjection, ActiveDgVoodooSnapshot,
        ActiveDgVoodooTarget, ActiveDgVoodooTargetPayload,
    },
};
use crate::{addons::engine::MergeStrategy, peer_mutation_executor::PeerPathSnapshot};

pub(crate) fn lower_active_dgvoodoo(
    plan: ActiveDgVoodooPlan,
    snapshots: &[ActiveDgVoodooSnapshot<'_>],
    accumulator: &mut LumaPeerEffectAccumulator,
) -> Result<ActiveDgVoodooRecordProjection, ActiveDgVoodooError> {
    let images = index_snapshots(&plan, snapshots)?;
    let decisions = prevalidate_targets(&plan, &images)?;
    let mut projection = ActiveDgVoodooRecordProjection {
        tracked_source: plan.tracked_source,
        ..Default::default()
    };

    for (target, decision) in plan.targets.into_iter().zip(decisions) {
        lower_target(target, decision, &images, accumulator, &mut projection)?;
    }
    Ok(projection)
}

fn index_snapshots<'a>(
    plan: &ActiveDgVoodooPlan,
    snapshots: &'a [ActiveDgVoodooSnapshot<'a>],
) -> Result<HashMap<String, &'a PeerPathSnapshot>, ActiveDgVoodooError> {
    let expected = plan
        .observation_paths()
        .iter()
        .map(|path| (renderpilot_domain::normalized_path_key(path.as_str()), path))
        .collect::<HashMap<_, _>>();
    let mut indexed = HashMap::with_capacity(expected.len());

    for entry in snapshots {
        let key = renderpilot_domain::normalized_path_key(entry.path.as_str());
        let Some(expected_path) = expected.get(&key) else {
            return Err(ActiveDgVoodooError::UnexpectedSnapshot(entry.path.clone()));
        };
        if expected_path.as_str() != entry.path.as_str() {
            return Err(ActiveDgVoodooError::SnapshotPathAlias {
                expected: (*expected_path).clone(),
                supplied: entry.path.clone(),
            });
        }
        if indexed.insert(key, entry.image).is_some() {
            return Err(ActiveDgVoodooError::DuplicateSnapshot(entry.path.clone()));
        }
    }

    for path in plan.observation_paths() {
        let key = renderpilot_domain::normalized_path_key(path.as_str());
        if !indexed.contains_key(&key) {
            return Err(ActiveDgVoodooError::MissingSnapshot(path.clone()));
        }
    }
    Ok(indexed)
}

#[derive(Debug)]
enum ValidatedTarget {
    Runtime,
    ConfigCreate(Vec<u8>),
    ConfigAcquire(Vec<u8>),
    ConfigUnchanged,
}

fn prevalidate_targets(
    plan: &ActiveDgVoodooPlan,
    images: &HashMap<String, &PeerPathSnapshot>,
) -> Result<Vec<ValidatedTarget>, ActiveDgVoodooError> {
    plan.targets
        .iter()
        .map(|target| prevalidate_target(target, images))
        .collect()
}

fn prevalidate_target(
    target: &ActiveDgVoodooTarget,
    images: &HashMap<String, &PeerPathSnapshot>,
) -> Result<ValidatedTarget, ActiveDgVoodooError> {
    let live_snapshot = image_for(&target.live, images);
    let sidecar_snapshot = image_for(&target.sidecar, images);
    require_absent(&target.sidecar, sidecar_snapshot)?;

    match &target.kind {
        ActiveDgVoodooTargetPayload::Runtime { .. } => {
            require_absent(&target.live, live_snapshot)?;
            Ok(ValidatedTarget::Runtime)
        }
        ActiveDgVoodooTargetPayload::Config { default, sections } => {
            let strategy = MergeStrategy::IniSetKeys {
                sections: sections.clone(),
            };
            let current = match live_snapshot {
                PeerPathSnapshot::Absent => {
                    let base = str::from_utf8(default).map_err(|_| {
                        ActiveDgVoodooError::InvalidConfigEncoding(target.live.clone())
                    })?;
                    return Ok(ValidatedTarget::ConfigCreate(
                        strategy.apply(base).into_bytes(),
                    ));
                }
                PeerPathSnapshot::File(_) => {
                    let current = live_snapshot.bytes().ok_or_else(|| {
                        ActiveDgVoodooError::SnapshotImageMismatch(target.live.clone())
                    })?;
                    let file = live_snapshot.file().ok_or_else(|| {
                        ActiveDgVoodooError::SnapshotImageMismatch(target.live.clone())
                    })?;
                    ensure_bytes_match_image(&target.live, current, file, false).map_err(|_| {
                        ActiveDgVoodooError::SnapshotImageMismatch(target.live.clone())
                    })?;
                    str::from_utf8(current).map_err(|_| {
                        ActiveDgVoodooError::InvalidConfigEncoding(target.live.clone())
                    })?
                }
            };
            let desired = strategy.apply(current).into_bytes();
            if desired == current.as_bytes() {
                Ok(ValidatedTarget::ConfigUnchanged)
            } else {
                Ok(ValidatedTarget::ConfigAcquire(desired))
            }
        }
    }
}

fn lower_target(
    target: ActiveDgVoodooTarget,
    decision: ValidatedTarget,
    images: &HashMap<String, &PeerPathSnapshot>,
    accumulator: &mut LumaPeerEffectAccumulator,
    projection: &mut ActiveDgVoodooRecordProjection,
) -> Result<(), ActiveDgVoodooError> {
    let live_path = target.live;
    let sidecar_path = target.sidecar;
    let live_snapshot = image_for(&live_path, images);
    let sidecar_snapshot = image_for(&sidecar_path, images);

    match (target.kind, decision) {
        (ActiveDgVoodooTargetPayload::Runtime { bytes }, ValidatedTarget::Runtime)
        | (ActiveDgVoodooTargetPayload::Config { .. }, ValidatedTarget::ConfigCreate(bytes)) => {
            lower_dgvoodoo_decision(
                DgVoodooDecision::CreateManaged {
                    live_path: &live_path,
                    live_snapshot,
                    prepared_bytes: bytes,
                },
                accumulator,
            )?;
            projection.created_files.push(live_path);
        }
        (ActiveDgVoodooTargetPayload::Config { .. }, ValidatedTarget::ConfigAcquire(bytes)) => {
            lower_dgvoodoo_decision(
                DgVoodooDecision::AcquireManaged {
                    live_path: &live_path,
                    sidecar_path: &sidecar_path,
                    live_snapshot,
                    sidecar_snapshot,
                    prepared_bytes: bytes,
                },
                accumulator,
            )?;
            projection.created_files.push(live_path.clone());
            projection.backed_up_files.push(live_path);
        }
        (ActiveDgVoodooTargetPayload::Config { .. }, ValidatedTarget::ConfigUnchanged) => {}
        _ => {
            return Err(ActiveDgVoodooError::SnapshotImageMismatch(live_path));
        }
    }
    Ok(())
}

fn image_for<'a>(
    path: &PathRef,
    images: &'a HashMap<String, &'a PeerPathSnapshot>,
) -> &'a PeerPathSnapshot {
    images
        .get(&renderpilot_domain::normalized_path_key(path.as_str()))
        .copied()
        .expect("validated dgVoodoo snapshot set")
}
