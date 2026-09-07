//! Storage binding for the sole RenoDX peer route that coordinates
//! OptiScaler's `Plugins.LoadReshade` receipt.

use renderpilot_application::{AppError, AppResult};
use renderpilot_domain::{
    ExactOptiConfigProjection, FileOwnership, FileReceipt, GameProxyTopology, OptiConfigOperation,
    OptiScalerConfigAuthority, OptiScalerInstallState, PeerEndpointEvidence, PeerEndpointOperation,
    PeerEndpointRole, ProxyImplementation, normalized_path_key,
};

use super::manifest::ParsedPeerProgram;
use super::permit::RenoDxOptiScalerConfigPeerCommit;

pub(super) fn bind_preparation(
    feature: &str,
    canonical_game_root: &renderpilot_domain::PathRef,
    program: &ParsedPeerProgram,
    before_topology: &GameProxyTopology,
    before_state: &OptiScalerInstallState,
    projection: &ExactOptiConfigProjection,
) -> AppResult<RenoDxOptiScalerConfigPeerCommit> {
    bind(
        feature,
        canonical_game_root,
        program,
        Some(before_topology),
        before_state,
        projection,
    )
}

/// Validates the manifest-only portion of the specialized route before a
/// durable preparation has sealed its topology. The permit path must call
/// [`bind_preparation`] and therefore cannot omit that binding.
pub(super) fn validate_preflight(
    feature: &str,
    canonical_game_root: &renderpilot_domain::PathRef,
    program: &ParsedPeerProgram,
    before_state: &OptiScalerInstallState,
    projection: &ExactOptiConfigProjection,
) -> AppResult<()> {
    bind(
        feature,
        canonical_game_root,
        program,
        None,
        before_state,
        projection,
    )
    .map(|_| ())
}

fn bind(
    feature: &str,
    canonical_game_root: &renderpilot_domain::PathRef,
    program: &ParsedPeerProgram,
    before_topology: Option<&GameProxyTopology>,
    before_state: &OptiScalerInstallState,
    projection: &ExactOptiConfigProjection,
) -> AppResult<RenoDxOptiScalerConfigPeerCommit> {
    if !matches!(
        feature,
        renderpilot_domain::mutation_features::RENODX_INSTALL
            | renderpilot_domain::mutation_features::RENODX_INSTALL_FROM_FILE
            | renderpilot_domain::mutation_features::RENODX_UNINSTALL
    ) {
        return Err(AppError::invalid_input(
            "OptiScaler configuration companion requires a RenoDX main install or uninstall feature",
        ));
    }
    before_state
        .validate()
        .map_err(|error| AppError::invalid_input(error.to_string()))?;
    if let Some(before_topology) = before_topology {
        if before_state.proxy_topology_id.as_deref() != Some(before_topology.id.as_str()) {
            return Err(AppError::invalid_input(
                "OptiScaler configuration companion state is not bound to the sealed before topology",
            ));
        }
        if before_topology.outer.implementation != ProxyImplementation::OptiScaler {
            return Err(AppError::invalid_input(
                "OptiScaler configuration companion requires an OptiScaler outer proxy topology",
            ));
        }
    }
    projection.validate().map_err(super::domain_error)?;
    if normalized_path_key(before_state.target_dir.as_str())
        != normalized_path_key(canonical_game_root.as_str())
        || normalized_path_key(projection.authority().canonical_game_root().as_str())
            != normalized_path_key(canonical_game_root.as_str())
    {
        return Err(AppError::invalid_input(
            "OptiScaler configuration companion root differs from the sealed game root",
        ));
    }
    let before_receipt = before_state
        .configuration_receipt()
        .map_err(|error| AppError::invalid_input(error.to_string()))?;
    if before_receipt.installed.ownership() != FileOwnership::Owned
        || projection.receipt().installed.ownership() != FileOwnership::Owned
    {
        return Err(AppError::invalid_input(
            "OptiScaler configuration companion requires an owned configuration receipt",
        ));
    }
    if projection.receipt().installed.identity() != before_receipt.installed.identity() {
        return Err(AppError::invalid_input(
            "OptiScaler configuration successor must preserve the owned file identity",
        ));
    }
    before_state
        .with_configuration_receipt(projection.receipt())
        .map_err(|error| AppError::invalid_input(error.to_string()))?;

    let mut matching = program
        .intents()
        .iter()
        .enumerate()
        .filter(|(_, intent)| intent.role() == PeerEndpointRole::OptiScalerConfig);
    let (Some((ordinal, intent)), None) = (matching.next(), matching.next()) else {
        return Err(AppError::invalid_input(
            "OptiScaler configuration companion requires exactly one typed endpoint",
        ));
    };
    if intent.operation() != PeerEndpointOperation::Replace
        || !projection.authority().matches_config_path(intent.path())
        || intent.planned_sha256() != Some(projection.receipt().installed.digest())
        || intent.planned_length().is_none()
    {
        return Err(AppError::invalid_input(
            "OptiScaler configuration endpoint differs from its exact projection",
        ));
    }
    let Some(before) = program.before().get(ordinal).and_then(Option::as_ref) else {
        return Err(AppError::invalid_input(
            "OptiScaler configuration endpoint is missing its exact preimage",
        ));
    };
    if before.identity() != before_receipt.installed.identity()
        || before.sha256() != before_receipt.installed.digest()
    {
        return Err(AppError::invalid_input(
            "OptiScaler configuration endpoint preimage differs from persisted state",
        ));
    }
    let expected_operation = match feature {
        renderpilot_domain::mutation_features::RENODX_INSTALL
        | renderpilot_domain::mutation_features::RENODX_INSTALL_FROM_FILE => {
            OptiConfigOperation::EnableLoadReshade
        }
        renderpilot_domain::mutation_features::RENODX_UNINSTALL => {
            OptiConfigOperation::DisableLoadReshade
        }
        _ => unreachable!("feature was checked above"),
    };
    if projection.operation() != expected_operation {
        return Err(AppError::invalid_input(
            "OptiScaler configuration operation does not match the RenoDX feature",
        ));
    }
    Ok(RenoDxOptiScalerConfigPeerCommit::new(
        before_state.clone(),
        projection.clone(),
    ))
}

/// Reconstructs only the filesystem authority needed to replay an ordinary
/// peer recovery program. Durable recovery never advances OptiScaler state,
/// but it must not execute an unbound typed endpoint.
pub(super) fn bind_recovery(
    feature: &str,
    canonical_game_root: &renderpilot_domain::PathRef,
    program: &ParsedPeerProgram,
) -> AppResult<()> {
    let authority =
        OptiScalerConfigAuthority::new(canonical_game_root.clone()).map_err(super::domain_error)?;
    let config_path_matches = program
        .intents()
        .iter()
        .filter(|intent| authority.matches_config_path(intent.path()))
        .collect::<Vec<_>>();
    let mut typed = program
        .intents()
        .iter()
        .enumerate()
        .filter(|(_, intent)| intent.role() == PeerEndpointRole::OptiScalerConfig);

    let (config_ordinal, config) = match (typed.next(), typed.next()) {
        (None, _) => {
            if let Some(intent) = config_path_matches.first() {
                return Err(invalid(format!(
                    "OptiScaler configuration path has an untyped role: {}",
                    intent.role().as_str()
                )));
            }
            return Ok(());
        }
        (Some((ordinal, config)), None) => (ordinal, config),
        (Some(_), Some(_)) => {
            return Err(invalid(
                "typed OptiScaler configuration recovery requires exactly one endpoint",
            ));
        }
    };
    if !matches!(
        feature,
        renderpilot_domain::mutation_features::RENODX_INSTALL
            | renderpilot_domain::mutation_features::RENODX_INSTALL_FROM_FILE
            | renderpilot_domain::mutation_features::RENODX_UNINSTALL
    ) {
        return Err(invalid(
            "typed OptiScaler configuration recovery requires a RenoDX main install or uninstall feature",
        ));
    }
    if config_path_matches.len() != 1
        || config_path_matches[0].role() != PeerEndpointRole::OptiScalerConfig
        || !authority.matches_config_path(config.path())
        || config.operation() != PeerEndpointOperation::Replace
        || config.planned_sha256().is_none()
        || config.planned_length().is_none()
        || program
            .before()
            .get(config_ordinal)
            .and_then(Option::as_ref)
            .is_none()
    {
        return Err(invalid(
            "typed OptiScaler configuration recovery endpoint differs from its exact replacement authority",
        ));
    }
    let mut downstream_matches = program
        .intents()
        .iter()
        .enumerate()
        .filter(|(_, intent)| intent.role() == PeerEndpointRole::TopologyDownstream);
    let (downstream_ordinal, downstream) = match (
        downstream_matches.next(),
        downstream_matches.next(),
    ) {
        (Some((ordinal, downstream)), None) => (ordinal, downstream),
        _ => {
            return Err(invalid(
                "typed OptiScaler configuration recovery requires exactly one topology downstream endpoint",
            ));
        }
    };
    let ordered = match feature {
        renderpilot_domain::mutation_features::RENODX_INSTALL
        | renderpilot_domain::mutation_features::RENODX_INSTALL_FROM_FILE => {
            matches!(
                downstream.operation(),
                PeerEndpointOperation::Create | PeerEndpointOperation::Replace
            ) && config_ordinal > downstream_ordinal
        }
        renderpilot_domain::mutation_features::RENODX_UNINSTALL => {
            matches!(
                downstream.operation(),
                PeerEndpointOperation::Remove | PeerEndpointOperation::Replace
            ) && config_ordinal < downstream_ordinal
        }
        _ => unreachable!("feature was checked above"),
    };
    if !ordered {
        return Err(invalid(
            "typed OptiScaler configuration recovery endpoint order does not match the RenoDX feature",
        ));
    }
    Ok(())
}

/// Materializes the only permitted durable state successor from sealed O2
/// evidence. The projection is not trusted as a write instruction: the
/// evidence must reproduce its identity, digest, and already-validated byte
/// length before state can advance.
pub(super) fn successor_from_evidence(
    companion: &RenoDxOptiScalerConfigPeerCommit,
    program: &ParsedPeerProgram,
    evidence: &[PeerEndpointEvidence],
) -> AppResult<OptiScalerInstallState> {
    let mut matching = program
        .intents()
        .iter()
        .enumerate()
        .filter(|(_, intent)| intent.role() == PeerEndpointRole::OptiScalerConfig);
    let (ordinal, intent) = match (matching.next(), matching.next()) {
        (Some((ordinal, intent)), None) => (ordinal, intent),
        _ => {
            return Err(AppError::storage_failed(
                "sealed OptiScaler configuration companion has no exact endpoint",
            ));
        }
    };
    let Some(observed) = evidence.get(ordinal) else {
        return Err(AppError::storage_failed(
            "sealed OptiScaler configuration companion evidence is incomplete",
        ));
    };
    if observed.intent() != intent {
        return Err(AppError::storage_failed(
            "OptiScaler configuration companion evidence intent changed",
        ));
    }
    let before_receipt = companion
        .before_state()
        .configuration_receipt()
        .map_err(|error| AppError::storage_failed(error.to_string()))?;
    let Some(before) = observed.before() else {
        return Err(AppError::storage_failed(
            "OptiScaler configuration companion lacks an O1 image",
        ));
    };
    if before.identity() != before_receipt.installed.identity()
        || before.sha256() != before_receipt.installed.digest()
    {
        return Err(AppError::storage_failed(
            "OptiScaler configuration O1 image differs from persisted receipt",
        ));
    }
    let Some(after) = observed.after() else {
        return Err(AppError::storage_failed(
            "OptiScaler configuration companion lacks an O2 image",
        ));
    };
    let projection = companion.projection();
    if after.identity() != projection.receipt().installed.identity()
        || after.sha256() != projection.receipt().installed.digest()
        || intent.planned_length() != Some(after.length())
    {
        return Err(AppError::storage_failed(
            "OptiScaler configuration O2 image differs from its sealed successor",
        ));
    }
    let mut receipt = projection.receipt().clone();
    receipt.installed = FileReceipt::owned(after.identity(), after.sha256().clone())
        .map_err(|error| AppError::storage_failed(error.to_string()))?;
    let successor = companion
        .before_state()
        .with_configuration_receipt(&receipt)
        .map_err(|error| AppError::storage_failed(error.to_string()))?;
    let expected = companion
        .before_state()
        .with_configuration_receipt(projection.receipt())
        .map_err(|error| AppError::storage_failed(error.to_string()))?;
    if successor != expected {
        return Err(AppError::storage_failed(
            "OptiScaler configuration successor changed fields outside its exact receipt",
        ));
    }
    Ok(successor)
}

fn invalid(message: impl Into<String>) -> AppError {
    AppError::invalid_input(format!(
        "OptiScaler configuration authority: {}",
        message.into()
    ))
}

#[cfg(test)]
mod tests {
    use renderpilot_domain::{
        GameId, OptiScalerAdoptionState, OptiScalerConfigurationBaseline, OptiScalerFileCleanup,
        OptiScalerFileReceipt, OptiScalerFileRole, OptiScalerInstallStateParts, PathRef, ProxyLink,
        ProxyRootPrestate, Sha256Hash,
    };
    use serde_json::json;

    use super::*;
    use crate::peer_runtime::manifest::parse_peer_program;

    const ROOT: &str = "C:/game";

    fn hash(value: char) -> Sha256Hash {
        Sha256Hash::new(value.to_string().repeat(Sha256Hash::HEX_LENGTH)).expect("hash")
    }

    fn path(value: &str) -> PathRef {
        PathRef::new(value).expect("path")
    }

    fn topology(game_id: &GameId) -> GameProxyTopology {
        let root_slot = path("C:/game/dxgi.dll");
        GameProxyTopology {
            id: "topology:opti-config".to_owned(),
            game_id: game_id.clone(),
            root_slot: root_slot.clone(),
            outer: ProxyLink {
                implementation: ProxyImplementation::OptiScaler,
                path: root_slot,
                receipt: FileReceipt::owned("outer", hash('e')).expect("receipt"),
            },
            downstream: None,
            downstream_origin: None,
            root_prestate: ProxyRootPrestate::Absent,
        }
    }

    fn state_and_projection(
        game_id: &GameId,
        topology: &GameProxyTopology,
    ) -> (OptiScalerInstallState, ExactOptiConfigProjection) {
        let config = path("C:/game/OptiScaler.ini");
        let state = renderpilot_domain::from_persisted(
            OptiScalerInstallStateParts {
                game_id: game_id.clone(),
                release_id: "release".to_owned(),
                manifest_revision: "revision".to_owned(),
                archive_sha256: None,
                source: None,
                target_exe_path: path("C:/game/Game.exe"),
                target_dir: path(ROOT),
                modules: vec!["core".to_owned()],
                release_files: vec![OptiScalerFileReceipt {
                    path: config,
                    installed: FileReceipt::owned("config", hash('a')).expect("receipt"),
                    role: OptiScalerFileRole::Configuration,
                    cleanup: OptiScalerFileCleanup::RemoveIfUnchanged,
                    baseline: renderpilot_domain::OptiScalerReleaseFileBaseline::Absent,
                }],
                runtime_bindings: Vec::new(),
                directory_receipts: Vec::new(),
                proxy_topology_id: Some(topology.id.clone()),
                config_schema: 1,
                config_base_release: "release".to_owned(),
                adoption_state: OptiScalerAdoptionState::Managed,
                prerequisite_binding: renderpilot_domain::OptiScalerPrerequisiteBinding::None,
                created_at: None,
                updated_at: None,
            },
            OptiScalerConfigurationBaseline::absent(),
        )
        .expect("state");
        let mut receipt = state.configuration_receipt().expect("receipt").clone();
        receipt.installed = FileReceipt::owned("config", hash('b')).expect("post receipt");
        let projection = ExactOptiConfigProjection::new(
            OptiScalerConfigAuthority::new(path(ROOT)).expect("authority"),
            receipt,
            OptiConfigOperation::EnableLoadReshade,
        )
        .expect("projection");
        (state, projection)
    }

    fn program() -> ParsedPeerProgram {
        let value = json!({
            "peer_program": {
                "format": 1,
                "transaction_owner": "opti-config-test",
                "execution_class": "ordinary",
                "roots": [ROOT],
                "stage": [],
                "custody": ["C:/game:optiscaler.ini"],
                "created_ancestors": [],
                "endpoints": [
                    {
                        "ordinal": 0,
                        "path": "C:/game/ReShade64.dll",
                        "role": "topology_downstream",
                        "operation": "create",
                        "planned_sha256": hash('d').as_str(),
                        "planned_length": 4,
                        "before": null,
                        "read_guards": ["C:/game:reshade64.dll"],
                        "subtree_publishes": []
                    },
                    {
                        "ordinal": 1,
                        "path": "C:/game/OptiScaler.ini",
                        "role": "optiscaler_config",
                        "operation": "replace",
                        "planned_sha256": hash('b').as_str(),
                        "planned_length": 4,
                        "before": {"identity": "config", "sha256": hash('a').as_str(), "length": 4},
                        "read_guards": ["C:/game:optiscaler.ini"],
                        "subtree_publishes": []
                    }
                ]
            }
        });
        parse_peer_program(&value, "OptiScaler config test").expect("program")
    }

    #[test]
    fn preparation_binds_the_state_to_the_sealed_optiscaler_topology() {
        let game_id = GameId::new("game:opti-config-topology").expect("game id");
        let topology = topology(&game_id);
        let (state, projection) = state_and_projection(&game_id, &topology);
        bind_preparation(
            renderpilot_domain::mutation_features::RENODX_INSTALL,
            &path(ROOT),
            &program(),
            &topology,
            &state,
            &projection,
        )
        .expect("exact topology binding");

        let mut stale = topology.clone();
        stale.id = "topology:other".to_owned();
        assert!(
            bind_preparation(
                renderpilot_domain::mutation_features::RENODX_INSTALL,
                &path(ROOT),
                &program(),
                &stale,
                &state,
                &projection,
            )
            .is_err()
        );

        let mut wrong_outer = topology;
        wrong_outer.outer.implementation = ProxyImplementation::ReShade;
        assert!(
            bind_preparation(
                renderpilot_domain::mutation_features::RENODX_INSTALL,
                &path(ROOT),
                &program(),
                &wrong_outer,
                &state,
                &projection,
            )
            .is_err()
        );
    }
}
