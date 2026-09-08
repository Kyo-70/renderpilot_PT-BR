use renderpilot_application::{AppError, AppResult};
use renderpilot_domain::GameId;
use rusqlite::{OptionalExtension, Transaction};

use crate::error::storage_error;

use super::super::observations;
use super::super::observations::CatalogReadiness;
use super::model::CatalogBinding;

/// Exact catalog authority captured when an OptiScaler journal becomes
/// Prepared. The token is retained alongside the epoch so later aggregate
/// operations cannot accidentally validate a different invalidation event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PreparedOptiScalerCatalogBinding {
    CatalogAbsent,
    CatalogInvalidated {
        authority_epoch: u64,
        mutation_token: String,
    },
}

pub(super) fn classify_catalog_binding_within_transaction(
    transaction: &Transaction<'_>,
    game_id: &GameId,
) -> AppResult<CatalogBinding> {
    let game_exists = transaction
        .query_row(
            "SELECT 1 FROM games WHERE id = ?1 LIMIT 1",
            [game_id.as_str()],
            |_| Ok(()),
        )
        .optional()
        .map_err(storage_error)?
        .is_some();
    let authority_exists = transaction
        .query_row(
            "SELECT 1 FROM catalog_scan_authority WHERE game_id = ?1 LIMIT 1",
            [game_id.as_str()],
            |_| Ok(()),
        )
        .optional()
        .map_err(storage_error)?
        .is_some();
    match (game_exists, authority_exists) {
        (false, false) => Ok(CatalogBinding::CatalogAbsent),
        (true, true) => Ok(CatalogBinding::CatalogPresent(
            observations::readiness_within_transaction(transaction, game_id)?,
        )),
        (true, false) => Err(AppError::storage_failed(format!(
            "catalog game {} is missing scan authority",
            game_id.as_str()
        ))),
        (false, true) => Err(AppError::storage_failed(format!(
            "scan authority exists without catalog game {}",
            game_id.as_str()
        ))),
    }
}

/// Establishes the catalog authority required by a prepared OptiScaler
/// aggregate. A ready or never-completed catalog is invalidated with the
/// exact pending operation token; an already matching invalidation is reused,
/// and an absent catalog remains absent. Classification and invalidation stay
/// in this canonical binding seam so aggregate preparation cannot grow a
/// second catalog state machine.
pub(crate) fn prepare_optiscaler_catalog_binding(
    transaction: &Transaction<'_>,
    game_id: &GameId,
    mutation_id: &str,
) -> AppResult<PreparedOptiScalerCatalogBinding> {
    match classify_catalog_binding_within_transaction(transaction, game_id)? {
        CatalogBinding::CatalogAbsent => Ok(PreparedOptiScalerCatalogBinding::CatalogAbsent),
        CatalogBinding::CatalogPresent(CatalogReadiness::Invalidated {
            authority_epoch,
            mutation_token: Some(token),
            ..
        }) if token == mutation_id => Ok(PreparedOptiScalerCatalogBinding::CatalogInvalidated {
            authority_epoch,
            mutation_token: token,
        }),
        CatalogBinding::CatalogPresent(_) => {
            let repaired = observations::invalidate_game_authority_within_transaction(
                transaction,
                game_id,
                "prepared_optiscaler_mutation",
                Some(mutation_id),
            )?;
            let CatalogReadiness::Invalidated {
                authority_epoch,
                mutation_token: Some(token),
                ..
            } = repaired
            else {
                return Err(AppError::storage_failed(
                    "OptiScaler preparation catalog binding did not become invalidated",
                ));
            };
            if token != mutation_id {
                return Err(AppError::storage_failed(
                    "OptiScaler preparation catalog binding has a mismatched mutation token",
                ));
            }
            Ok(PreparedOptiScalerCatalogBinding::CatalogInvalidated {
                authority_epoch,
                mutation_token: token,
            })
        }
    }
}

/// Revalidates the exact catalog authority captured by a Prepared
/// OptiScaler journal. Classification is performed from the same transaction
/// as the caller's row/reservation operation; no catalog state is inferred.
pub(crate) fn validate_optiscaler_catalog_binding(
    transaction: &Transaction<'_>,
    game_id: &GameId,
    mutation_id: &str,
    expected: &PreparedOptiScalerCatalogBinding,
) -> AppResult<()> {
    let current = classify_catalog_binding_within_transaction(transaction, game_id)?;
    let matches = match (expected, current) {
        (PreparedOptiScalerCatalogBinding::CatalogAbsent, CatalogBinding::CatalogAbsent) => true,
        (
            PreparedOptiScalerCatalogBinding::CatalogInvalidated {
                authority_epoch,
                mutation_token,
            },
            CatalogBinding::CatalogPresent(CatalogReadiness::Invalidated {
                authority_epoch: current_epoch,
                mutation_token: Some(current_token),
                ..
            }),
        ) => {
            *authority_epoch == current_epoch
                && mutation_token == mutation_id
                && current_token == mutation_id
        }
        _ => false,
    };
    if !matches {
        return Err(AppError::storage_failed(
            "OptiScaler Prepared catalog binding changed",
        ));
    }
    Ok(())
}
