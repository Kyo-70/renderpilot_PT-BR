fn build_single_effect(
    scope: &MutationScope,
    participant: &PlannedParticipant,
    wanted: OptiScalerAction,
    absent_dirs: &HashSet<String>,
    repeated: bool,
) -> Result<DomainOperationEffect, ServiceError> {
    super::scope::require_path_in_scope(&participant.path, scope)?;
    let before = planned_before(participant, wanted, absent_dirs, repeated)?;
    let endpoint = DomainOperationEndpoint::new(
        DomainEndpoint::Single,
        participant.path.to_string_lossy().into_owned(),
        preimage_for(participant, &before, None),
        JournalAfter::Pending,
    )
    .map_err(|error| crate::failed(error.to_string()))?;
    Ok(match wanted {
        OptiScalerAction::Write => DomainOperationEffect::Write(
            DomainWriteEffect::new(endpoint, DomainWriteState::Planned)
                .map_err(|error| crate::failed(error.to_string()))?,
        ),
        OptiScalerAction::Delete => DomainOperationEffect::Delete(
            DomainDeleteEffect::new(endpoint, DomainDeleteState::Planned)
                .map_err(|error| crate::failed(error.to_string()))?,
        ),
        OptiScalerAction::Verify => DomainOperationEffect::Verify(
            DomainVerifyEffect::new(endpoint, DomainVerifyState::Planned)
                .map_err(|error| crate::failed(error.to_string()))?,
        ),
        OptiScalerAction::CreateDirectory => DomainOperationEffect::CreateDirectory(
            DomainCreateDirectoryEffect::new(endpoint, DomainCreateDirectoryState::Planned)
                .map_err(|error| crate::failed(error.to_string()))?,
        ),
        _ => return Err(crate::failed("invalid single action")),
    })
}

fn build_relocate_effect(
    scope: &MutationScope,
    source: &PlannedParticipant,
    destination: &PlannedParticipant,
    absent_dirs: &HashSet<String>,
    source_repeated: bool,
    destination_repeated: bool,
) -> Result<DomainOperationEffect, ServiceError> {
    if crate::paths::same_path(&source.path, &destination.path) {
        return Err(crate::failed(
            "relocation source and destination must be distinct",
        ));
    }
    if !matches!(destination.preimage, PlannedPreimage::Absent) {
        return Err(crate::failed(
            "relocation destination requires an explicit absent preimage",
        ));
    }
    super::scope::require_path_in_scope(&source.path, scope)?;
    super::scope::require_path_in_scope(&destination.path, scope)?;
    let source_before = planned_before(
        source,
        OptiScalerAction::Relocate,
        absent_dirs,
        source_repeated,
    )?;
    // A relocation destination is required to be absent.  Evaluate it with
    // the absence-capable creation contract; the native no-replace move still
    // enforces that exact preimage at execution time.
    let destination_before = planned_before(
        destination,
        OptiScalerAction::CreateDirectory,
        absent_dirs,
        destination_repeated,
    )?;
    let source_endpoint = DomainOperationEndpoint::new(
        DomainEndpoint::Source,
        source.path.to_string_lossy().into_owned(),
        preimage_for(source, &source_before, None),
        JournalAfter::Pending,
    )
    .map_err(|error| crate::failed(error.to_string()))?;
    let destination_endpoint = DomainOperationEndpoint::new(
        DomainEndpoint::Destination,
        destination.path.to_string_lossy().into_owned(),
        preimage_for(destination, &destination_before, None),
        JournalAfter::Pending,
    )
    .map_err(|error| crate::failed(error.to_string()))?;
    DomainRelocateEffect::new(
        source_endpoint,
        destination_endpoint,
        DomainRelocateState::Planned,
    )
    .map(|effect| DomainOperationEffect::Relocate(Box::new(effect)))
    .map_err(|error| crate::failed(error.to_string()))
}
