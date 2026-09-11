//! Narrow RenoDX-owned bridge for OptiScaler's typed ReShade-chain switch.
//!
//! This is intentionally not a second configuration lifecycle. It observes
//! the one persisted OptiScaler configuration receipt, changes only
//! `Plugins.LoadReshade`, and hands the exact endpoint plus successor proof to
//! the existing ordinary peer permit.

use std::path::Path;

use renderpilot_application::OptiScalerStateRepository;
use renderpilot_domain::{
    ExactOptiConfigProjection, FileOwnership, FileReceipt, OptiConfigOperation,
    OptiScalerConfigAuthority, PathRef, PeerEndpointRole, normalized_path_key,
};

use crate::addons::optiscaler::config::set_reshade_chain_enabled;
use crate::addons::peer_lifecycle::package::RenoDxOptiScalerConfigCompanion;
use crate::peer_mutation_executor::{
    EndpointExpectation, EndpointPostcondition, ExactEndpoint, PeerPathSnapshot,
    observe_peer_path_snapshot,
};
use crate::{Context, ServiceError};

/// One exact endpoint and durable state companion emitted only when changing
/// the semantic switch is necessary.
pub(super) struct RenoDxOptiScalerConfigPlan {
    companion: RenoDxOptiScalerConfigCompanion,
    endpoint: ExactEndpoint,
    payload: Vec<u8>,
}

impl RenoDxOptiScalerConfigPlan {
    pub(super) fn into_parts(self) -> (RenoDxOptiScalerConfigCompanion, ExactEndpoint, Vec<u8>) {
        (self.companion, self.endpoint, self.payload)
    }
}

/// Captures the configuration only through the sealed game root and returns
/// an exact replacement plan when the requested state differs. A missing or
/// drifted OptiScaler receipt is a hard failure; an already-correct setting is
/// intentionally not manufactured into a no-op state transition.
pub(super) fn plan(
    context: &Context,
    game_id: &renderpilot_domain::GameId,
    canonical_game_root: &Path,
    enabled: bool,
) -> Result<Option<RenoDxOptiScalerConfigPlan>, ServiceError> {
    let state = context
        .storage()
        .get_optiscaler_install_state(game_id)?
        .ok_or_else(|| {
            ServiceError::invalid_input(
                "active RenoDX proxy route requires an exact OptiScaler state",
            )
        })?;
    state
        .validate()
        .map_err(|error| ServiceError::invalid_input(error.to_string()))?;
    let root = PathRef::from_canonical_native_absolute(canonical_game_root)
        .map_err(|error| ServiceError::invalid_input(error.to_string()))?;
    let authority = OptiScalerConfigAuthority::new(root.clone())
        .map_err(|error| ServiceError::invalid_input(error.to_string()))?;
    let before_receipt = state
        .configuration_receipt()
        .map_err(|error| ServiceError::invalid_input(error.to_string()))?;
    if before_receipt.installed.ownership() != FileOwnership::Owned {
        return Err(ServiceError::invalid_input(
            "active RenoDX proxy route requires an owned OptiScaler configuration receipt",
        ));
    }
    if normalized_path_key(before_receipt.path.as_str())
        != normalized_path_key(authority.config_path().as_str())
    {
        return Err(ServiceError::invalid_input(
            "OptiScaler configuration receipt does not match the canonical game root",
        ));
    }
    let snapshot = observe_peer_path_snapshot(authority.config_path(), &root)?;
    let (before, before_bytes) = match &snapshot {
        PeerPathSnapshot::Absent => {
            return Err(ServiceError::invalid_input(
                "OptiScaler configuration receipt is present but its file is absent",
            ));
        }
        PeerPathSnapshot::File(_) => (
            snapshot.file().expect("file variant has metadata"),
            snapshot.bytes().expect("file variant has bytes"),
        ),
    };
    if before.identity() != before_receipt.installed.identity()
        || before.digest() != before_receipt.installed.digest()
    {
        return Err(ServiceError::invalid_input(
            "OptiScaler configuration file differs from its persisted receipt",
        ));
    }
    let after_bytes = set_reshade_chain_enabled(before_bytes, enabled);
    if after_bytes == before_bytes {
        return Ok(None);
    }
    let after_digest = renderpilot_detection::sha256_bytes(&after_bytes)
        .map_err(|error| ServiceError::command_failed(error.to_string()))?;
    let installed = FileReceipt::owned(before.identity(), after_digest.clone())
        .map_err(|error| ServiceError::invalid_input(error.to_string()))?;
    let mut after_receipt = before_receipt.clone();
    after_receipt.installed = installed;
    let projection = ExactOptiConfigProjection::new(
        authority,
        after_receipt,
        if enabled {
            OptiConfigOperation::EnableLoadReshade
        } else {
            OptiConfigOperation::DisableLoadReshade
        },
    )
    .map_err(|error| ServiceError::invalid_input(error.to_string()))?;
    let companion = RenoDxOptiScalerConfigCompanion::new(state, projection.clone())?;
    let endpoint = ExactEndpoint::new(
        projection.authority().config_path().clone(),
        PeerEndpointRole::OptiScalerConfig,
        EndpointExpectation::File(before.clone()),
        EndpointPostcondition::File(after_digest),
    );
    Ok(Some(RenoDxOptiScalerConfigPlan {
        companion,
        endpoint,
        payload: after_bytes,
    }))
}
