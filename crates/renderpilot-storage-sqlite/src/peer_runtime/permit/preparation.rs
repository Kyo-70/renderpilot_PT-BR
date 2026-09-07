//! Preparing-row validation and permit minting.

use std::sync::Arc;

use renderpilot_application::{AppError, AppResult};
use rusqlite::named_params;

use super::super::manifest::{
    parse_manifest, parse_peer_program, parse_peer_program_with_renodx_dlss,
};
use super::super::read_guards;
use super::super::renodx_reshade_ini;
use super::super::shared_roots;
use super::super::validation::{
    PeerContractInput, derive_catalog_physical, derive_contract, derive_contract_with_catalog,
    derive_contract_with_renodx_dlss, validate_common_preparation, validate_file_manifest_binding,
    validate_preparing_file_row, validate_preparing_shared_row, validate_program_binding,
};
use super::fingerprint::{
    ensure_identity_stable, invalidate_catalog_for_prepared, read_file_fingerprint,
    read_shared_fingerprint,
};
use super::types::{
    PeerCommitPreparation, PeerStorageRuntime, PermitValues, PreparedPeerCommitPermit,
    PreparedRowBinding, SharedPeerCommitPreparation,
};
use crate::peer_runtime::catalog::OwnedCatalogProjection;
use crate::{error::storage_error, sqlite_clock};
use renderpilot_domain::RenoDxDlssProjection;
use renderpilot_domain::{ExactOptiConfigProjection, OptiScalerInstallState};

impl PeerStorageRuntime {
    /// Completes a Preparing game-file row and mints a permit in that same
    /// transaction after validating the closed physical contract.
    pub fn finish_file_peer_preparation(
        &self,
        preparation: PeerCommitPreparation<'_>,
    ) -> AppResult<PreparedPeerCommitPermit> {
        self.finish_file_peer_preparation_scoped(preparation, None)
    }

    /// Completes the narrow RenoDX peer route which changes exactly one
    /// OptiScaler configuration receipt alongside its normal peer/topology
    /// commit. Generic peer preparation never reaches this branch.
    pub fn finish_file_peer_preparation_with_renodx_optiscaler_config(
        &self,
        preparation: PeerCommitPreparation<'_>,
        before_state: &OptiScalerInstallState,
        projection: &ExactOptiConfigProjection,
    ) -> AppResult<PreparedPeerCommitPermit> {
        self.finish_file_peer_preparation_scoped(preparation, Some((before_state, projection)))
    }

    fn finish_file_peer_preparation_scoped(
        &self,
        preparation: PeerCommitPreparation<'_>,
        optiscaler_config_input: Option<(&OptiScalerInstallState, &ExactOptiConfigProjection)>,
    ) -> AppResult<PreparedPeerCommitPermit> {
        validate_common_preparation(
            preparation.mutation_id,
            preparation.feature,
            preparation.manifest_json,
        )?;
        let manifest = parse_manifest(preparation.manifest_json, "peer manifest")?;
        let program = parse_peer_program(&manifest, "peer manifest")?;
        validate_program_binding(&program, preparation.mutation_id, false)?;
        validate_file_manifest_binding(&manifest, &program)?;
        let renodx_reshade_ini = renodx_reshade_ini::bind_preparation(
            preparation.feature,
            &read_guards::bind_canonical_game_root(
                preparation.canonical_game_root,
                program.roots(),
            )?,
            &program,
            preparation.renodx_reshade_ini,
        )?;
        let optiscaler_config = optiscaler_config_input
            .map(|(before_state, projection)| {
                let before_topology = preparation.before_topology.ok_or_else(|| {
                    AppError::invalid_input(
                        "OptiScaler configuration companion requires a sealed before topology",
                    )
                })?;
                super::super::optiscaler_config::bind_preparation(
                    preparation.feature,
                    &read_guards::bind_canonical_game_root(
                        preparation.canonical_game_root,
                        program.roots(),
                    )?,
                    &program,
                    before_topology,
                    before_state,
                    projection,
                )
            })
            .transpose()?;
        if (renodx_reshade_ini.is_some() || optiscaler_config.is_some())
            && (preparation.component_set.is_some()
                || !preparation.baseline_mutations.is_empty()
                || preparation.catalog_claim.is_some())
        {
            return Err(AppError::invalid_input(
                "typed RenoDX peer preparation cannot carry a catalog projection",
            ));
        }

        self.storage.with_transaction(|transaction| {
            validate_preparing_file_row(
                transaction,
                preparation.mutation_id,
                preparation.game_id,
                preparation.feature,
                preparation.subject_id,
            )?;
            let catalog = super::super::catalog::bind_preparation(
                transaction,
                preparation.game_id,
                preparation.before_peer,
                preparation.catalog_claim,
                preparation.component_set,
                preparation.baseline_mutations,
            )?;
            let physical_catalog = derive_catalog_physical(
                catalog.catalog_claim(),
                preparation.before_peer,
                preparation.after_peer,
                &program,
            )?;
            let companions = match optiscaler_config.as_ref() {
                Some(opti) => read_guards::ReadGuardCompanions {
                    catalog: None,
                    catalog_claim: None,
                    renodx_reshade_ini: renodx_reshade_ini.as_ref(),
                    dlss: None,
                    optiscaler_config: Some(&opti.projection),
                },
                None => read_guards::ReadGuardCompanions {
                    catalog: physical_catalog.as_ref(),
                    catalog_claim: catalog.catalog_claim(),
                    renodx_reshade_ini: renodx_reshade_ini.as_ref(),
                    dlss: None,
                    optiscaler_config: None,
                },
            };
            let read_guards = read_guards::bind_initial(read_guards::InitialReadGuardInput {
                canonical_game_root: preparation.canonical_game_root,
                transition: read_guards::ReadGuardTransition {
                    sealed_roots: program.roots(),
                    before_peer: preparation.before_peer,
                    after_peer: preparation.after_peer,
                    before_topology: preparation.before_topology,
                    planned_after_topology: preparation.planned_after_topology,
                    route: preparation.route,
                    intents: program.intents(),
                    program_before: &[],
                },
                initial_evidence: preparation.initial_read_guards,
                companions,
            })?;
            let contract = match optiscaler_config.as_ref() {
                Some(opti) => {
                    super::super::validation::derive_contract_with_renodx_optiscaler_config(
                        PeerContractInput {
                            before_peer: preparation.before_peer,
                            after_peer: preparation.after_peer,
                            before_topology: preparation.before_topology,
                            planned_after_topology: preparation.planned_after_topology,
                            route: preparation.route,
                            program: &program,
                            renodx_reshade_ini,
                        },
                        opti.projection.clone(),
                    )?
                }
                None => derive_contract_with_catalog(
                    PeerContractInput {
                        before_peer: preparation.before_peer,
                        after_peer: preparation.after_peer,
                        before_topology: preparation.before_topology,
                        planned_after_topology: preparation.planned_after_topology,
                        route: preparation.route,
                        program: &program,
                        renodx_reshade_ini,
                    },
                    physical_catalog.as_ref(),
                )?,
            };
            let values = PermitValues::from_file(
                preparation,
                program,
                contract,
                read_guards,
                catalog,
                optiscaler_config.clone(),
            );
            let previous = read_file_fingerprint(transaction, &values.mutation_id)?
                .ok_or_else(|| AppError::storage_failed("peer preparation row disappeared"))?;
            let now_ms = sqlite_clock::now_ms(transaction)?;
            let changed = transaction
                .execute(
                    "UPDATE pending_file_mutations
                     SET state = 'prepared', manifest_json = :manifest_json,
                         updated_at = :now_ms
                     WHERE id = :id AND state = 'preparing'",
                    named_params! {
                        ":id": values.mutation_id.as_str(),
                        ":manifest_json": values.manifest_json.as_str(),
                        ":now_ms": now_ms,
                    },
                )
                .map_err(storage_error)?;
            if changed != 1 {
                return Err(AppError::storage_failed(
                    "peer preparation changed before Prepared transition",
                ));
            }
            invalidate_catalog_for_prepared(transaction, &values.game_id, &values.mutation_id)?;
            let fingerprint = read_file_fingerprint(transaction, &values.mutation_id)?
                .ok_or_else(|| AppError::storage_failed("peer Prepared row disappeared"))?;
            ensure_identity_stable(previous, fingerprint)?;
            Ok(values.into_permit(
                Arc::clone(&self.instance),
                PreparedRowBinding::File,
                fingerprint,
            ))
        })
    }

    /// Completes a file row for the narrow RenoDX DLSS-Fix projection.  This
    /// branch may accept an otherwise unchanged manifest with no physical
    /// endpoints, but only alongside the validated projection supplied by the
    /// caller.  The generic parser and preparation path remain strict.
    pub fn finish_file_peer_preparation_with_renodx_dlss(
        &self,
        preparation: PeerCommitPreparation<'_>,
        projection: RenoDxDlssProjection,
    ) -> AppResult<PreparedPeerCommitPermit> {
        validate_common_preparation(
            preparation.mutation_id,
            preparation.feature,
            preparation.manifest_json,
        )?;
        if !renderpilot_domain::mutation_features::is_renodx_dlss_fix_feature(preparation.feature) {
            return Err(AppError::invalid_input(
                "RenoDX DLSS projection requires a DLSS-Fix feature",
            ));
        }
        if preparation.component_set.is_some()
            || !preparation.baseline_mutations.is_empty()
            || preparation.catalog_claim.is_some()
        {
            return Err(AppError::invalid_input(
                "RenoDX DLSS projection cannot carry a catalog projection",
            ));
        }
        let manifest = parse_manifest(preparation.manifest_json, "peer manifest")?;
        let program = parse_peer_program_with_renodx_dlss(&manifest, "peer manifest")?;
        validate_program_binding(&program, preparation.mutation_id, false)?;
        validate_file_manifest_binding(&manifest, &program)?;
        let canonical_game_root = read_guards::bind_canonical_game_root(
            preparation.canonical_game_root,
            program.roots(),
        )?;
        let renodx_reshade_ini = renodx_reshade_ini::bind_preparation(
            preparation.feature,
            &canonical_game_root,
            &program,
            preparation.renodx_reshade_ini,
        )?;
        read_guards::validate_dlss_projection_manifest_binding(
            program.roots(),
            program.intents(),
            program.before(),
            &projection,
        )?;

        self.storage.with_transaction(|transaction| {
            validate_preparing_file_row(
                transaction,
                preparation.mutation_id,
                preparation.game_id,
                preparation.feature,
                preparation.subject_id,
            )?;
            let read_guards = read_guards::bind_initial(read_guards::InitialReadGuardInput {
                canonical_game_root: preparation.canonical_game_root,
                transition: read_guards::ReadGuardTransition {
                    sealed_roots: program.roots(),
                    before_peer: preparation.before_peer,
                    after_peer: preparation.after_peer,
                    before_topology: preparation.before_topology,
                    planned_after_topology: preparation.planned_after_topology,
                    route: preparation.route,
                    intents: program.intents(),
                    program_before: program.before(),
                },
                initial_evidence: preparation.initial_read_guards,
                companions: read_guards::ReadGuardCompanions {
                    catalog: None,
                    catalog_claim: None,
                    renodx_reshade_ini: renodx_reshade_ini.as_ref(),
                    dlss: Some(&projection),
                    optiscaler_config: None,
                },
            })?;
            let contract = derive_contract_with_renodx_dlss(
                PeerContractInput {
                    before_peer: preparation.before_peer,
                    after_peer: preparation.after_peer,
                    before_topology: preparation.before_topology,
                    planned_after_topology: preparation.planned_after_topology,
                    route: preparation.route,
                    program: &program,
                    renodx_reshade_ini,
                },
                projection,
            )?;
            let values = PermitValues::from_file(
                preparation,
                program,
                contract,
                read_guards,
                OwnedCatalogProjection::default(),
                None,
            );
            let previous = read_file_fingerprint(transaction, &values.mutation_id)?
                .ok_or_else(|| AppError::storage_failed("peer preparation row disappeared"))?;
            let now_ms = sqlite_clock::now_ms(transaction)?;
            let changed = transaction
                .execute(
                    "UPDATE pending_file_mutations
                     SET state = 'prepared', manifest_json = :manifest_json,
                         updated_at = :now_ms
                     WHERE id = :id AND state = 'preparing'",
                    named_params! {
                        ":id": values.mutation_id.as_str(),
                        ":manifest_json": values.manifest_json.as_str(),
                        ":now_ms": now_ms,
                    },
                )
                .map_err(storage_error)?;
            if changed != 1 {
                return Err(AppError::storage_failed(
                    "peer preparation changed before Prepared transition",
                ));
            }
            invalidate_catalog_for_prepared(transaction, &values.game_id, &values.mutation_id)?;
            let fingerprint = read_file_fingerprint(transaction, &values.mutation_id)?
                .ok_or_else(|| AppError::storage_failed("peer Prepared row disappeared"))?;
            ensure_identity_stable(previous, fingerprint)?;
            Ok(values.into_permit(
                Arc::clone(&self.instance),
                PreparedRowBinding::File,
                fingerprint,
            ))
        })
    }

    /// Completes a game-scoped shared row and mints a permit from that row.
    pub fn finish_shared_peer_preparation(
        &self,
        preparation: SharedPeerCommitPreparation<'_>,
    ) -> AppResult<PreparedPeerCommitPermit> {
        validate_common_preparation(
            preparation.mutation_id,
            preparation.feature,
            preparation.manifest_json,
        )?;
        let manifest = parse_manifest(preparation.manifest_json, "shared peer manifest")?;
        let program = parse_peer_program(&manifest, "shared peer manifest")?;
        validate_program_binding(&program, preparation.mutation_id, true)?;

        self.storage.with_transaction(|transaction| {
            validate_preparing_shared_row(
                transaction,
                preparation.mutation_id,
                preparation.feature,
                preparation.game_id,
            )?;
            let roots = shared_roots::bind_preparing_row(
                transaction,
                preparation.mutation_id,
                program.roots(),
            )?;
            let authority = if let Some(game_root) = roots.canonical_game_root() {
                renodx_reshade_ini::bind_preparation(
                    preparation.feature,
                    game_root,
                    &program,
                    preparation.renodx_reshade_ini,
                )?
            } else if preparation.renodx_reshade_ini.is_some()
                || program.renodx_reshade_ini_intent().is_some()
            {
                return Err(AppError::invalid_input(
                    "RenoDX ReShade.ini authority requires a game-0 shared root",
                ));
            } else {
                None
            };
            let contract = derive_contract(PeerContractInput {
                before_peer: preparation.before_peer,
                after_peer: preparation.after_peer,
                before_topology: preparation.before_topology,
                planned_after_topology: preparation.planned_after_topology,
                route: preparation.route,
                program: &program,
                renodx_reshade_ini: authority,
            })?;
            let values = PermitValues::from_shared(preparation, program, contract, roots);
            let previous = read_shared_fingerprint(transaction, &values.mutation_id)?
                .ok_or_else(|| AppError::storage_failed("shared preparation row disappeared"))?;
            let now_ms = sqlite_clock::now_ms(transaction)?;
            let changed = transaction
                .execute(
                    "UPDATE pending_shared_vulkan_mutations
                     SET state = 'prepared', manifest_json = :manifest_json,
                         updated_at = :now_ms
                     WHERE resource_key = :resource_key AND id = :id
                       AND scope = 'game_shared' AND game_id = :game_id
                       AND feature = :feature AND state = 'preparing'",
                    named_params! {
                        ":resource_key": crate::repositories::pending_shared_vulkan_mutations::RESOURCE_KEY,
                        ":id": values.mutation_id.as_str(),
                        ":game_id": values.game_id.as_str(),
                        ":feature": values.feature.as_str(),
                        ":manifest_json": values.manifest_json.as_str(),
                        ":now_ms": now_ms,
                    },
                )
                .map_err(storage_error)?;
            if changed != 1 {
                return Err(AppError::storage_failed(
                    "shared preparation changed before Prepared transition",
                ));
            }
            invalidate_catalog_for_prepared(transaction, &values.game_id, &values.mutation_id)?;
            let fingerprint = read_shared_fingerprint(transaction, &values.mutation_id)?
                .ok_or_else(|| AppError::storage_failed("shared Prepared row disappeared"))?;
            ensure_identity_stable(previous, fingerprint)?;
            Ok(values.into_permit(
                Arc::clone(&self.instance),
                PreparedRowBinding::Shared,
                fingerprint,
            ))
        })
    }
}
