use super::super::super::*;
use super::context::BindingContext;

pub(super) fn apply_auxiliary_preservations(
    context: &mut BindingContext<'_>,
    auxiliary: &[OptiScalerAuxiliaryPreservation],
) -> AppResult<()> {
    for preservation in auxiliary {
        let source_key = normalized_path_key(preservation.source.as_str());
        let destination_key = normalized_path_key(preservation.destination.as_str());
        if source_key.is_empty() || destination_key.is_empty() || source_key == destination_key {
            return Err(renderpilot_application::AppError::invalid_input(
                "OptiScaler auxiliary recovery requires distinct non-empty paths",
            ));
        }
        if !context.auxiliary_sources.insert(source_key.clone())
            || !context
                .auxiliary_destinations
                .insert(destination_key.clone())
            || context.auxiliary_destinations.contains(&source_key)
            || context.auxiliary_sources.contains(&destination_key)
            || context.before_receipts.contains_key(&destination_key)
            || context.after_receipts.contains_key(&destination_key)
            || context.before_directories.contains_key(&destination_key)
            || context.after_directories.contains_key(&destination_key)
            || context
                .peer
                .peer_relocation
                .as_ref()
                .is_some_and(|relocation| {
                    let source = normalized_path_key(relocation.source.as_str());
                    let destination = normalized_path_key(relocation.destination.as_str());
                    source == source_key
                        || destination == source_key
                        || source == destination_key
                        || destination == destination_key
                })
        {
            return Err(renderpilot_application::AppError::invalid_input(
                "OptiScaler auxiliary recovery path collides with another aggregate authority",
            ));
        }
        let Some((_, prior)) = context.before_receipts.get(&source_key) else {
            return Err(renderpilot_application::AppError::invalid_input(
                "OptiScaler auxiliary preservation source has no exact prior receipt",
            ));
        };
        let current = &preservation.source_current;
        let destination = &preservation.destination_receipt;
        if !is_owned(prior)
            || !is_owned(current)
            || !is_owned(destination)
            || !same_receipt_identity(prior, current)
            || destination.digest() != current.digest()
        {
            return Err(renderpilot_application::AppError::invalid_input(
                "OptiScaler auxiliary preservation does not carry exact owned source and destination receipts",
            ));
        }
        context
            .owned_preservations
            .push(pending_file_mutations::OptiScalerOwnedPreservation {
                source: preservation.source.as_str().to_owned(),
                prior: prior.clone(),
                current: current.clone(),
            });
        context.known_paths.insert(source_key);
        context.known_paths.insert(destination_key);
        // The destination is authorized by the typed preservation entry, not
        // by an independent claim. Keeping it out of `paths` prevents one
        // write from being consumed twice by two authorities.
    }
    Ok(())
}
