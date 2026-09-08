//! Opaque storage runtime for peer mutation commits.
//!
//! The runtime is the only storage-facing owner of a peer commit permit.  A
//! permit is minted from a freshly prepared durable row and is bound to the
//! runtime instance that minted it; the ordinary `SqliteStorage` handle never
//! exposes a peer commit entry point.

pub(crate) mod aggregate;
mod ancestor_binding;
mod catalog;
mod file_manifest_binding;
mod manifest;
mod metadata_aggregate;
mod optiscaler_config;
mod optiscaler_journal_aggregate;
mod permit;
mod read_guards;
mod recovery;
mod renodx_reshade_ini;
mod shared_roots;
mod validation;

#[cfg(test)]
mod catalog_tests;
#[cfg(test)]
mod dlss_tests;
#[cfg(test)]
mod read_guard_path_tests;
#[cfg(test)]
mod read_guard_tests;
#[cfg(test)]
mod recovery_tests;
#[cfg(test)]
mod renodx_reshade_ini_tests;
#[cfg(test)]
mod shared_recovery_tests;
#[cfg(test)]
mod tests;

pub use aggregate::{
    AggregateAfter, AggregateBefore, AggregateGeneration, CommittedAggregate,
    GameAggregateMutation, PlannedAggregateAfter,
};
pub use metadata_aggregate::{
    CommittedMetadataAggregate, MetadataAggregatePreparation, MetadataAggregateTransition,
    PreparedMetadataAggregateCommitPermit,
};
pub use optiscaler_journal_aggregate::{
    CommittedOptiScalerJournalAggregate, OptiScalerJournalAggregateBegin,
    OptiScalerJournalAggregateCommit, PendingFileMutationRecoveryCandidate,
    PreparedOptiScalerJournalAggregate, PreparingOptiScalerJournalAggregate,
    RecoveringOptiScalerJournalAggregate,
};
pub use permit::{
    PeerCommitPreparation, PeerStorageRuntime, PreparedPeerCommitPermit,
    SharedPeerCommitPreparation,
};

pub use recovery::{
    PeerRecoveryAncestor, PeerRecoveryEndpoint, PeerRecoveryExecutionClass, PeerRecoveryImage,
    PeerRecoveryProgram,
};

pub(crate) use permit::{domain_error, read_file_manifest, read_shared_manifest};

pub(crate) use manifest::{
    ParsedPeerProgram, parse_file_peer_program_for_feature, parse_manifest, parse_peer_program,
    parse_peer_program_with_renodx_dlss, sha256_bytes,
};

/// Validates a strict format-1 peer-program envelope from durable storage.
pub fn validate_peer_program_manifest(
    manifest_json: &str,
) -> renderpilot_application::AppResult<()> {
    let manifest = parse_manifest(manifest_json, "peer manifest")?;
    let program = parse_peer_program(&manifest, "peer manifest")?;
    reject_typed_peer_endpoint(&program, "peer manifest")
}

/// Validates a peer-program envelope intended for a file-scoped row.
pub fn validate_file_peer_program_manifest(
    manifest_json: &str,
) -> renderpilot_application::AppResult<()> {
    let manifest = parse_manifest(manifest_json, "peer manifest")?;
    let program = parse_peer_program(&manifest, "peer manifest")?;
    reject_typed_peer_endpoint(&program, "file peer manifest")?;
    validation::validate_program_scope(
        &program,
        false,
        "file peer manifest cannot use shared execution class",
    )
}

/// Validates a peer-program envelope intended for a game-shared row.
pub fn validate_shared_peer_program_manifest(
    manifest_json: &str,
) -> renderpilot_application::AppResult<()> {
    let manifest = parse_manifest(manifest_json, "shared peer manifest")?;
    let program = parse_peer_program(&manifest, "shared peer manifest")?;
    reject_typed_peer_endpoint(&program, "shared peer manifest")?;
    validation::validate_program_scope(
        &program,
        true,
        "shared peer manifest must use shared execution class",
    )
}

/// Validates a file-scoped peer manifest that carries the typed RenoDX
/// ReShade.ini endpoint authority. This is preflight only: it does not mint a
/// permit or authorize any file mutation.
pub fn validate_file_peer_program_manifest_with_renodx_reshade_ini(
    feature: &str,
    canonical_game_root: &str,
    authority: &renderpilot_domain::RenoDxReshadeIniAuthority,
    manifest_json: &str,
) -> renderpilot_application::AppResult<()> {
    let manifest = parse_manifest(manifest_json, "peer manifest")?;
    let program = parse_file_peer_program_for_feature(&manifest, feature, "peer manifest")?;
    validation::validate_program_scope(
        &program,
        false,
        "file peer manifest cannot use shared execution class",
    )?;
    file_manifest_binding::bind(&manifest, &program)?;
    let canonical_game_root =
        read_guards::bind_canonical_game_root(canonical_game_root, program.roots())?;
    renodx_reshade_ini::bind_preparation(feature, &canonical_game_root, &program, Some(authority))?
        .ok_or_else(|| {
            renderpilot_application::AppError::invalid_input(
                "RenoDX ReShade.ini preflight requires a typed endpoint",
            )
        })?;
    Ok(())
}

/// Validates the one active RenoDX proxy manifest which atomically advances
/// OptiScaler's exact `Plugins.LoadReshade` configuration receipt. This is a
/// preflight-only companion to ordinary peer recovery: it neither mints a
/// permit nor grants generic peers a typed OptiScaler endpoint.
pub fn validate_file_peer_program_manifest_with_renodx_optiscaler_config(
    feature: &str,
    canonical_game_root: &str,
    renodx_reshade_ini: Option<&renderpilot_domain::RenoDxReshadeIniAuthority>,
    before_state: &renderpilot_domain::OptiScalerInstallState,
    projection: &renderpilot_domain::ExactOptiConfigProjection,
    manifest_json: &str,
) -> renderpilot_application::AppResult<()> {
    let manifest = parse_manifest(manifest_json, "peer manifest")?;
    let program = parse_file_peer_program_for_feature(&manifest, feature, "peer manifest")?;
    validation::validate_program_scope(
        &program,
        false,
        "file peer manifest cannot use shared execution class",
    )?;
    file_manifest_binding::bind(&manifest, &program)?;
    let canonical_game_root =
        read_guards::bind_canonical_game_root(canonical_game_root, program.roots())?;
    renodx_reshade_ini::bind_preparation(
        feature,
        &canonical_game_root,
        &program,
        renodx_reshade_ini,
    )?;
    optiscaler_config::validate_preflight(
        feature,
        &canonical_game_root,
        &program,
        before_state,
        projection,
    )?;
    Ok(())
}

/// Validates a file-scoped peer manifest carrying the typed RenoDX DLSS-Fix
/// projection. This is preflight only: peer claims are checked again against
/// the exact before/after records while minting the permit.
pub fn validate_file_peer_program_manifest_with_renodx_dlss(
    feature: &str,
    canonical_game_root: &str,
    authority: Option<&renderpilot_domain::RenoDxReshadeIniAuthority>,
    projection: &renderpilot_domain::RenoDxDlssProjection,
    manifest_json: &str,
) -> renderpilot_application::AppResult<()> {
    if !renderpilot_domain::mutation_features::is_renodx_dlss_fix_feature(feature) {
        return Err(renderpilot_application::AppError::invalid_input(
            "RenoDX DLSS projection requires a DLSS-Fix feature",
        ));
    }
    let manifest = parse_manifest(manifest_json, "peer manifest")?;
    let program = parse_file_peer_program_for_feature(&manifest, feature, "peer manifest")?;
    validation::validate_program_scope(
        &program,
        false,
        "file peer manifest cannot use shared execution class",
    )?;
    file_manifest_binding::bind(&manifest, &program)?;
    let canonical_game_root =
        read_guards::bind_canonical_game_root(canonical_game_root, program.roots())?;
    let authority =
        renodx_reshade_ini::bind_preparation(feature, &canonical_game_root, &program, authority)?;
    if program.intents().is_empty() && authority.is_some() {
        return Err(renderpilot_application::AppError::invalid_input(
            "claim-only RenoDX DLSS preparation cannot carry ReShade.ini authority",
        ));
    }
    read_guards::validate_dlss_projection_manifest_binding(
        program.roots(),
        program.intents(),
        program.before(),
        projection,
    )
}

fn reject_typed_peer_endpoint(
    program: &ParsedPeerProgram,
    context: &str,
) -> renderpilot_application::AppResult<()> {
    if program.renodx_reshade_ini_intent().is_some()
        || program
            .intents()
            .iter()
            .any(|intent| intent.role() == renderpilot_domain::PeerEndpointRole::OptiScalerConfig)
    {
        return Err(renderpilot_application::AppError::invalid_input(format!(
            "{context} cannot use a typed RenoDX/OptiScaler endpoint"
        )));
    }
    Ok(())
}

/// Validates and returns the immutable storage-owned recovery projection for
/// one file peer mutation.  A valid outer object without a peer program is
/// deliberately represented as `None`; every malformed or partial peer
/// program is an error.
pub fn validated_file_peer_recovery_program(
    mutation_id: &str,
    manifest_json: &str,
) -> renderpilot_application::AppResult<Option<PeerRecoveryProgram>> {
    validated_file_peer_recovery_program_inner(mutation_id, manifest_json, None)
}

/// Validates and returns the immutable storage-owned recovery projection for
/// one file peer mutation, including a typed RenoDX ReShade.ini authority when
/// the durable program carries that endpoint.
pub fn validated_file_peer_recovery_program_for_feature(
    mutation_id: &str,
    feature: &str,
    manifest_json: &str,
) -> renderpilot_application::AppResult<Option<PeerRecoveryProgram>> {
    validated_file_peer_recovery_program_inner(mutation_id, manifest_json, Some(feature))
}

fn validated_file_peer_recovery_program_inner(
    mutation_id: &str,
    manifest_json: &str,
    feature: Option<&str>,
) -> renderpilot_application::AppResult<Option<PeerRecoveryProgram>> {
    let manifest = parse_manifest(manifest_json, "file peer recovery manifest")?;
    if !manifest.is_object() {
        return Err(renderpilot_application::AppError::storage_failed(
            "file peer recovery manifest must be an object",
        ));
    }
    if manifest.get("peer_program").is_none() {
        return Ok(None);
    }

    let program = match feature {
        Some(feature) => {
            parse_file_peer_program_for_feature(&manifest, feature, "file peer recovery manifest")?
        }
        None => parse_peer_program(&manifest, "file peer recovery manifest")?,
    };
    validation::validate_program_binding(&program, mutation_id, false)?;
    if feature.is_none()
        && (program.renodx_reshade_ini_intent().is_some()
            || program.intents().iter().any(|intent| {
                intent.role() == renderpilot_domain::PeerEndpointRole::OptiScalerConfig
            }))
    {
        return Err(renderpilot_application::AppError::invalid_input(
            "typed RenoDX/OptiScaler recovery requires a feature-bound entrypoint",
        ));
    }
    let binding = file_manifest_binding::bind(&manifest, &program)?;
    if feature.is_some_and(|feature| {
        renderpilot_domain::mutation_features::is_renodx_dlss_fix_feature(feature)
    }) && program.intents().is_empty()
    {
        if program.execution_class() != "ordinary" || binding.transaction_dir.is_none() {
            return Err(renderpilot_application::AppError::storage_failed(
                "RenoDX DLSS claim-only recovery manifest is incomplete",
            ));
        }
        return Ok(None);
    }
    let execution_class = match (binding.format, program.execution_class()) {
        (1, "ordinary") => recovery::PeerRecoveryExecutionClass::ordinary(),
        (2, "retryable") => recovery::PeerRecoveryExecutionClass::retryable(),
        _ => {
            return Err(renderpilot_application::AppError::storage_failed(
                "file peer recovery format does not match execution class",
            ));
        }
    };
    let transaction_dir = binding.transaction_dir.clone().ok_or_else(|| {
        renderpilot_application::AppError::storage_failed(
            "file peer recovery manifest transaction_dir is missing",
        )
    })?;
    let renodx_reshade_ini = feature
        .map(|feature| {
            let first_root = binding.roots.first().ok_or_else(|| {
                renderpilot_application::AppError::storage_failed(
                    "file peer recovery manifest must seal a canonical game root",
                )
            })?;
            let first_root = read_guards::strict_absolute_path(
                first_root,
                "file peer recovery canonical game root",
            )?;
            optiscaler_config::bind_recovery(feature, &first_root, &program)?;
            renodx_reshade_ini::bind_recovery(feature, &first_root, &program)
        })
        .transpose()?
        .flatten();
    Ok(Some(recovery::PeerRecoveryProgram::from_validated(
        execution_class,
        binding.roots.clone(),
        transaction_dir,
        &program,
        binding,
        renodx_reshade_ini,
    )?))
}

/// Validates one full durable shared-Vulkan manifest and its independently
/// persisted root capability envelope for recovery. The outer manifest is
/// checked only for storage-owned fields; participant semantics remain owned
/// by orchestration.
pub fn validate_shared_peer_recovery_program(
    mutation_id: &str,
    feature: &str,
    manifest_json: &str,
    root_capabilities_json: &str,
) -> renderpilot_application::AppResult<()> {
    let manifest = parse_manifest(manifest_json, "shared peer recovery manifest")?;
    let object = manifest.as_object().ok_or_else(|| {
        renderpilot_application::AppError::storage_failed(
            "shared peer recovery manifest must be an object",
        )
    })?;
    let persisted_feature = object
        .get("feature")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            renderpilot_application::AppError::storage_failed(
                "shared peer recovery manifest feature is missing",
            )
        })?;
    if persisted_feature != feature {
        return Err(renderpilot_application::AppError::invalid_input(
            "shared peer recovery manifest feature does not match its durable row",
        ));
    }
    if object.get("peer_program").is_none() {
        return Err(renderpilot_application::AppError::storage_failed(
            "shared peer recovery manifest must carry a peer_program",
        ));
    }

    let program = parse_peer_program(&manifest, "shared peer recovery manifest")?;
    validation::validate_program_binding(&program, mutation_id, true)?;
    if program
        .intents()
        .iter()
        .any(|intent| intent.role() == renderpilot_domain::PeerEndpointRole::TopologyDownstream)
    {
        return Err(renderpilot_application::AppError::invalid_input(
            "shared peer recovery program cannot carry a topology downstream endpoint",
        ));
    }
    let roots = shared_roots::bind_persisted_json(root_capabilities_json, program.roots())?;
    match roots.canonical_game_root() {
        Some(game_root) => {
            renodx_reshade_ini::bind_recovery(feature, game_root, &program)?;
            if let Some(typed_ordinal) = program.intents().iter().position(|intent| {
                intent.role() == renderpilot_domain::PeerEndpointRole::RenoDxReshadeIni
            }) {
                let expected_guard =
                    format!("{}:reshade.ini", program.roots()[0]).to_ascii_lowercase();
                let actual_guards = program.read_guards().get(typed_ordinal).ok_or_else(|| {
                    renderpilot_application::AppError::storage_failed(
                        "typed RenoDX ReShade.ini endpoint is missing read-guard storage",
                    )
                })?;
                if actual_guards.len() != 1
                    || actual_guards[0].as_str().to_ascii_lowercase() != expected_guard
                {
                    return Err(renderpilot_application::AppError::invalid_input(
                        "typed RenoDX ReShade.ini endpoint must use the game-0 capability",
                    ));
                }
            }
        }
        None if program.renodx_reshade_ini_intent().is_some() => {
            return Err(renderpilot_application::AppError::invalid_input(
                "typed RenoDX ReShade.ini recovery requires game-0 root authority",
            ));
        }
        None => {}
    }
    Ok(())
}
