use renderpilot_application::{AppError, AppResult};
use renderpilot_domain::{
    ExactOptiConfigProjection, GameId, GameProxyTopology, InstalledAddon,
    PeerCatalogPhysicalContract, PeerEndpointEvidence, PeerTransitionContext,
    PeerTransitionContract, PlannedGameProxyTopology, ProxyPeerRoute, RenoDxReshadeIniAuthority,
};
use rusqlite::{OptionalExtension, Transaction};

use super::{
    ParsedPeerProgram, PreparedPeerCommitPermit, parse_manifest, parse_peer_program,
    parse_peer_program_with_renodx_dlss, sha256_bytes,
};
use crate::error::storage_error;

/// Validates the file manifest's complete outer/inner projection through its
/// dedicated binding module.  Permit preparation retains this API so shared
/// Vulkan validation remains unchanged.
pub(super) fn validate_file_manifest_binding(
    manifest: &serde_json::Value,
    program: &ParsedPeerProgram,
) -> AppResult<()> {
    super::file_manifest_binding::bind(manifest, program).map(|_| ())
}

pub(super) fn validate_common_preparation(
    id: &str,
    feature: &str,
    manifest_json: &str,
) -> AppResult<()> {
    if id.trim().is_empty() {
        return Err(AppError::invalid_input(
            "peer mutation id must not be empty",
        ));
    }
    validate_feature(feature)?;
    let manifest = parse_manifest(manifest_json, "peer manifest")?;
    if manifest.get("peer_program").is_none() {
        return Err(AppError::invalid_input(
            "peer manifest must carry a peer_program",
        ));
    }
    Ok(())
}

fn validate_feature(feature: &str) -> AppResult<()> {
    let allowed = matches!(
        renderpilot_domain::mutation_features::feature_owner(feature),
        Some(
            renderpilot_domain::mutation_features::MutationFeatureOwner::Luma
                | renderpilot_domain::mutation_features::MutationFeatureOwner::RenoDx
        )
    );
    if !allowed {
        return Err(AppError::invalid_input(format!(
            "feature '{feature}' cannot mint a peer commit permit"
        )));
    }
    Ok(())
}

pub(super) fn validate_program_binding(
    program: &ParsedPeerProgram,
    mutation_id: &str,
    shared: bool,
) -> AppResult<()> {
    if program.transaction_owner() != mutation_id {
        return Err(AppError::invalid_input(
            "peer program transaction owner does not match mutation id",
        ));
    }
    validate_program_scope(
        program,
        shared,
        "peer program execution class does not match mutation scope",
    )?;
    Ok(())
}

pub(super) fn validate_program_scope(
    program: &ParsedPeerProgram,
    shared: bool,
    mismatch_message: &str,
) -> AppResult<()> {
    let valid_class = if shared {
        program.execution_class() == "shared"
    } else {
        matches!(program.execution_class(), "ordinary" | "retryable")
    };
    if !valid_class {
        return Err(AppError::invalid_input(mismatch_message));
    }
    Ok(())
}

/// Aggregate images and sealed program used to derive one peer contract.
pub(super) struct PeerContractInput<'a> {
    pub(super) before_peer: Option<&'a InstalledAddon>,
    pub(super) after_peer: Option<&'a InstalledAddon>,
    pub(super) before_topology: Option<&'a GameProxyTopology>,
    pub(super) planned_after_topology: Option<&'a PlannedGameProxyTopology>,
    pub(super) route: ProxyPeerRoute,
    pub(super) program: &'a ParsedPeerProgram,
    pub(super) renodx_reshade_ini: Option<RenoDxReshadeIniAuthority>,
}

pub(super) fn derive_contract(input: PeerContractInput<'_>) -> AppResult<PeerTransitionContract> {
    derive_contract_with_catalog(input, None)
}

pub(super) fn derive_contract_with_catalog(
    input: PeerContractInput<'_>,
    catalog: Option<&PeerCatalogPhysicalContract>,
) -> AppResult<PeerTransitionContract> {
    if catalog.is_some() && input.renodx_reshade_ini.is_some() {
        return Err(AppError::invalid_input(
            "catalog projection cannot accompany RenoDX ReShade.ini authority",
        ));
    }
    let contract = match input.renodx_reshade_ini {
        Some(authority) => PeerTransitionContract::derive_physical_with_renodx_reshade_ini(
            input.before_peer,
            input.after_peer,
            input.before_topology,
            input.planned_after_topology,
            input.route,
            authority,
            input.program.intents().to_vec(),
        ),
        None => PeerTransitionContract::derive_physical_with_catalog(
            input.before_peer,
            input.after_peer,
            input.before_topology,
            input.planned_after_topology,
            input.route,
            input.program.intents().to_vec(),
            catalog,
        ),
    }
    .map_err(super::domain_error)?;
    contract
        .validate_preimages(input.program.before())
        .map_err(super::domain_error)?;
    Ok(contract)
}

pub(super) fn derive_contract_with_renodx_dlss(
    input: PeerContractInput<'_>,
    projection: renderpilot_domain::RenoDxDlssProjection,
) -> AppResult<PeerTransitionContract> {
    let contract = PeerTransitionContract::derive_physical_with_renodx_reshade_ini_and_dlss(
        PeerTransitionContext::new(
            input.before_peer,
            input.after_peer,
            input.before_topology,
            input.planned_after_topology,
            input.route,
        ),
        input.renodx_reshade_ini,
        input.program.intents().to_vec(),
        projection,
    )
    .map_err(super::domain_error)?;
    contract
        .validate_preimages(input.program.before())
        .map_err(super::domain_error)?;
    Ok(contract)
}

pub(super) fn derive_contract_with_renodx_optiscaler_config(
    input: PeerContractInput<'_>,
    optiscaler_config: ExactOptiConfigProjection,
) -> AppResult<PeerTransitionContract> {
    let contract =
        PeerTransitionContract::derive_physical_with_renodx_reshade_ini_and_optiscaler_config(
            PeerTransitionContext::new(
                input.before_peer,
                input.after_peer,
                input.before_topology,
                input.planned_after_topology,
                input.route,
            ),
            input.renodx_reshade_ini,
            optiscaler_config,
            input.program.intents().to_vec(),
        )
        .map_err(super::domain_error)?;
    contract
        .validate_preimages(input.program.before())
        .map_err(super::domain_error)?;
    Ok(contract)
}

pub(super) fn derive_catalog_physical(
    claim: Option<&renderpilot_domain::PeerCatalogRollbackClaim>,
    before_peer: Option<&InstalledAddon>,
    after_peer: Option<&InstalledAddon>,
    program: &ParsedPeerProgram,
) -> AppResult<Option<PeerCatalogPhysicalContract>> {
    claim
        .map(|claim| {
            PeerCatalogPhysicalContract::derive(
                claim,
                before_peer,
                after_peer,
                program.intents(),
                program.before(),
            )
            .map_err(super::domain_error)
        })
        .transpose()
}

pub(super) fn validate_sealed_evidence(
    permit: &PreparedPeerCommitPermit,
    evidence: &[PeerEndpointEvidence],
) -> AppResult<()> {
    permit
        .contract()
        .validate_evidence(evidence)
        .map_err(super::domain_error)?;
    if evidence.len() != permit.program().intents().len() {
        return Err(AppError::storage_failed(
            "peer evidence cardinality differs from sealed program",
        ));
    }
    for (index, observed) in evidence.iter().enumerate() {
        if observed.intent() != &permit.program().intents()[index]
            || observed.before() != permit.program().before()[index].as_ref()
        {
            return Err(AppError::storage_failed(
                "peer evidence differs from sealed O1/O2 program",
            ));
        }
    }
    Ok(())
}

pub(super) fn materialize_topology(
    permit: &PreparedPeerCommitPermit,
    evidence: &[PeerEndpointEvidence],
) -> AppResult<Option<GameProxyTopology>> {
    permit
        .planned_after_topology()
        .map(|planned| {
            planned
                .materialize_from_contract_evidence(permit.contract(), evidence)
                .map_err(super::domain_error)
        })
        .transpose()
}

pub(super) fn validate_file_seal(
    transaction: &Transaction<'_>,
    permit: &PreparedPeerCommitPermit,
) -> AppResult<()> {
    let row: Option<(String, String, Option<String>, String)> = transaction
        .query_row(
            "SELECT game_id, feature, subject_id, state
             FROM pending_file_mutations WHERE id = ?1",
            [permit.mutation_id()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()
        .map_err(storage_error)?;
    let Some((game_id, feature, subject_id, state)) = row else {
        return Err(AppError::storage_failed("peer Prepared row is missing"));
    };
    if game_id != permit.game_id().as_str()
        || feature != permit.feature()
        || subject_id != permit.subject_id().map(str::to_owned)
        || state != "prepared"
    {
        return Err(AppError::storage_failed(
            "peer Prepared row feature, subject, or state changed before commit",
        ));
    }
    let current_manifest = super::read_file_manifest(transaction, permit.mutation_id())?;
    validate_manifest_seal(&current_manifest, permit, "peer manifest")
}

pub(super) fn validate_shared_seal(
    transaction: &Transaction<'_>,
    permit: &PreparedPeerCommitPermit,
) -> AppResult<()> {
    let row: Option<(String, String, Option<String>, String, String)> = transaction
        .query_row(
            "SELECT resource_key, scope, game_id, feature, state
             FROM pending_shared_vulkan_mutations
             WHERE resource_key = ?1 AND id = ?2",
            rusqlite::params![
                crate::repositories::pending_shared_vulkan_mutations::RESOURCE_KEY,
                permit.mutation_id(),
            ],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .optional()
        .map_err(storage_error)?;
    let Some((resource_key, scope, game_id, feature, state)) = row else {
        return Err(AppError::storage_failed(
            "shared peer Prepared row is missing",
        ));
    };
    if resource_key != crate::repositories::pending_shared_vulkan_mutations::RESOURCE_KEY
        || scope != "game_shared"
        || game_id.as_deref() != Some(permit.game_id().as_str())
        || feature != permit.feature()
        || state != "prepared"
    {
        return Err(AppError::storage_failed(
            "shared peer Prepared row resource, scope, owner, feature, or state changed before commit",
        ));
    }
    let current_manifest = super::read_shared_manifest(transaction, permit.mutation_id())?;
    validate_manifest_seal(&current_manifest, permit, "shared peer manifest")
}

fn validate_manifest_seal(
    current_manifest: &str,
    permit: &PreparedPeerCommitPermit,
    context: &str,
) -> AppResult<()> {
    if sha256_bytes(current_manifest.as_bytes()) != permit.fingerprint().manifest_sha256 {
        return Err(AppError::storage_failed(format!(
            "{context} seal changed before commit"
        )));
    }
    let value = parse_manifest(current_manifest, context)?;
    let program = if permit.contract().renodx_dlss_projection().is_some() {
        parse_peer_program_with_renodx_dlss(&value, context)?
    } else {
        parse_peer_program(&value, context)?
    };
    if program.seal() != permit.program().seal()
        || program.roots() != permit.program().roots()
        || program.transaction_owner() != permit.program().transaction_owner()
        || program.execution_class() != permit.program().execution_class()
    {
        return Err(AppError::storage_failed(format!(
            "{context} program seal changed before commit"
        )));
    }
    Ok(())
}

pub(super) fn validate_preparing_file_row(
    transaction: &Transaction<'_>,
    id: &str,
    game_id: &GameId,
    feature: &str,
    subject_id: Option<&str>,
) -> AppResult<()> {
    let row: Option<(String, String, String, Option<String>)> = transaction
        .query_row(
            "SELECT game_id, feature, state, subject_id
             FROM pending_file_mutations WHERE id = ?1",
            [id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()
        .map_err(storage_error)?;
    let Some((row_game, row_feature, state, row_subject)) = row else {
        return Err(AppError::storage_failed(format!(
            "peer preparation row '{id}' is missing"
        )));
    };
    if row_game != game_id.as_str()
        || row_feature != feature
        || state != "preparing"
        || row_subject.as_deref() != subject_id
    {
        return Err(AppError::storage_failed(
            "peer preparation row does not match its exact request",
        ));
    }
    Ok(())
}

pub(super) fn validate_preparing_shared_row(
    transaction: &Transaction<'_>,
    id: &str,
    feature: &str,
    game_id: &GameId,
) -> AppResult<()> {
    let row: Option<(String, String, String, Option<String>)> = transaction
        .query_row(
            "SELECT id, scope, state, game_id
             FROM pending_shared_vulkan_mutations
             WHERE resource_key = ?1 AND id = ?2 AND feature = ?3",
            rusqlite::params![
                crate::repositories::pending_shared_vulkan_mutations::RESOURCE_KEY,
                id,
                feature,
            ],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()
        .map_err(storage_error)?;
    let Some((row_id, scope, state, row_game)) = row else {
        return Err(AppError::storage_failed(format!(
            "shared peer preparation row '{id}' is missing"
        )));
    };
    if row_id != id
        || scope != "game_shared"
        || state != "preparing"
        || row_game.as_deref() != Some(game_id.as_str())
    {
        return Err(AppError::storage_failed(
            "shared peer preparation row does not match its exact request",
        ));
    }
    Ok(())
}
