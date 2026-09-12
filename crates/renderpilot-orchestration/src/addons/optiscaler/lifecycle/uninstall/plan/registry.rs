use super::*;

pub(in crate::addons::optiscaler::lifecycle::uninstall) fn validate_producer_registry(
    precommit: &[UninstallStep],
) -> Result<(), ServiceError> {
    let mut producers = HashMap::<String, (usize, String)>::new();
    let mut main_relocation = None;
    for (index, step) in precommit.iter().enumerate() {
        match step {
            UninstallStep::RelocatePeer { .. } => {
                if main_relocation.replace(index).is_some() {
                    return Err(failed("OptiScaler uninstall has duplicate peer relocation"));
                }
            }
            UninstallStep::RelocatePeerSidecar { .. }
                if main_relocation != Some(index.saturating_sub(1)) =>
            {
                return Err(failed(
                    "OptiScaler peer sidecar relocation must immediately follow main relocation",
                ));
            }
            _ => {}
        }
        for (endpoint_index, path) in step.mutation_paths().into_iter().enumerate() {
            let key = crate::paths::normalized_key(path);
            let label = if matches!(
                step,
                UninstallStep::RelocatePeer { .. } | UninstallStep::RelocatePeerSidecar { .. }
            ) && endpoint_index == 1
            {
                "RelocatePeer.destination".to_owned()
            } else if matches!(step, UninstallStep::RestoreRetainedFsrOriginal { .. })
                && endpoint_index == 1
            {
                "RestoreRetainedFsrOriginal.destination".to_owned()
            } else {
                step_label(step)
            };
            if let Some((prior, prior_label)) = producers.get(&key) {
                let allowed_root_handoff = prior_label == "DeleteTopologyOuter"
                    && label == "RelocatePeer.destination"
                    && *prior + 1 == index;
                let allowed_retained_fsr_handoff = prior_label == "DeleteOwned"
                    && label == "RestoreRetainedFsrOriginal.destination"
                    && *prior + 1 == index;
                if !allowed_root_handoff && !allowed_retained_fsr_handoff {
                    return Err(failed(format!(
                        "OptiScaler uninstall has duplicate producer for {} ({} at {}, {} at {})",
                        path.display(),
                        prior_label,
                        prior,
                        label,
                        index
                    )));
                }
            } else {
                producers.insert(key, (index, label));
            }
        }
    }
    Ok(())
}

fn step_label(step: &UninstallStep) -> String {
    match step {
        UninstallStep::CreateDirectory { .. } => "CreateDirectory".to_owned(),
        UninstallStep::PreserveConfiguration { .. } => "PreserveConfiguration".to_owned(),
        UninstallStep::RestoreConfiguration { .. } => "RestoreConfiguration".to_owned(),
        UninstallStep::DeleteOwned { .. } => "DeleteOwned".to_owned(),
        UninstallStep::DeleteReusedArtifact { .. } => "DeleteReusedArtifact".to_owned(),
        UninstallStep::VerifyNoMutation { .. } => "VerifyNoMutation".to_owned(),
        UninstallStep::DeleteTopologyOuter { .. } => "DeleteTopologyOuter".to_owned(),
        UninstallStep::RestoreRetainedFsrOriginal { .. } => "RestoreRetainedFsrOriginal".to_owned(),
        UninstallStep::RelocatePeer { .. } => "RelocatePeer".to_owned(),
        UninstallStep::RelocatePeerSidecar { .. } => "RelocatePeerSidecar".to_owned(),
    }
}
