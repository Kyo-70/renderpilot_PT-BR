//! Update, repair, and module-selection commands.

use super::*;

/// Updates to the current stable immutable release.
pub async fn update(
    request: UpdateOptiScalerRequest<'_>,
) -> Result<OptiScalerOperationResult, ServiceError> {
    update_with_intent(request, ApplyIntent::Update).await
}

/// Reapplies the installed release while preserving semantic user configuration.
pub async fn repair(
    request: UpdateOptiScalerRequest<'_>,
) -> Result<OptiScalerOperationResult, ServiceError> {
    update_with_intent(request, ApplyIntent::Repair).await
}

/// Reconciles the installed module DAG with an explicit selection.
pub async fn set_modules(
    request: SetOptiScalerModulesRequest<'_>,
) -> Result<OptiScalerOperationResult, ServiceError> {
    update_with_intent(
        UpdateOptiScalerRequest {
            context: request.context,
            manifest: request.manifest,
            safety: request.safety,
            progress: request.progress,
        },
        ApplyIntent::SetModules {
            modules: request.modules.to_vec(),
        },
    )
    .await
}

async fn update_with_intent(
    request: UpdateOptiScalerRequest<'_>,
    intent: ApplyIntent,
) -> Result<OptiScalerOperationResult, ServiceError> {
    let game_id = request.safety.game_id().clone();
    let (initial_snapshot, initial_availability) = {
        let _guard =
            crate::mutation_boundary::enter_game_mutation_boundary_async(request.context, &game_id)
                .await?;
        let snapshot = capture_lifecycle_snapshot(request.context, &game_id)?;
        ensure_managed_install_snapshot(&snapshot)?;
        let availability =
            matcher::evaluation_off_runtime(request.context, request.manifest, &game_id).await?;
        (snapshot, availability)
    };
    let old_state = initial_snapshot
        .state
        .clone()
        .ok_or_else(|| failed("OptiScaler is not installed for this game"))?;
    let old_managed_files = managed_bindings_from_state(Some(&old_state));
    let topology = initial_snapshot.topology.clone();
    ensure_apply_allowed(&initial_availability)?;
    let release = current_release(request.manifest)?;
    let old_release = request
        .manifest
        .releases
        .iter()
        .find(|release| release.id == old_state.release_id)
        .ok_or_else(|| failed("installed OptiScaler release is absent from the manifest"))?;
    let requested_modules = intent.requested_modules();
    let mut modules = requested_modules.map_or_else(
        || reconcile_modules_for_release(request.manifest, &release.id, &old_state.modules),
        |modules| validate_module_selection(request.manifest, &release.id, modules),
    )?;
    if requested_modules.is_none() {
        let components = request
            .context
            .storage()
            .list_components_for_game(&game_id)?;
        if matcher::native_sr_is_redundant(&components, &old_managed_files) {
            modules.remove("nvidia_sr");
        }
    }
    ensure_selected_modules_available(&initial_availability, &modules)?;
    let archive = download_and_stage(release, &modules, request.progress).await?;
    let module_artifacts =
        prepare_selected(request.manifest, release, &modules, request.progress).await?;
    let native_artifacts =
        prepare_native_modules(request.context, &game_id, &modules, request.progress).await?;
    let old_config_base = if old_release.id == release.id {
        None
    } else {
        Some(download_config_base(old_release, request.progress).await?)
    };
    crate::addons::progress::emit_tool_finalizing(request.progress, AddonKind::OptiScaler);
    let proxy_path = topology.as_ref().map_or_else(
        || Path::new(old_state.target_dir.as_str()).join("dxgi.dll"),
        |topology| PathBuf::from(topology.root_slot.as_str()),
    );
    let target = ApplyTarget {
        exe: PathBuf::from(old_state.target_exe_path.as_str()),
        dir: PathBuf::from(old_state.target_dir.as_str()),
        proxy: EvaluatedProxyPlan {
            slot: proxy_path,
            chain_reshade: topology
                .as_ref()
                .is_some_and(|value| value.downstream.is_some()),
            downstream_path: topology
                .as_ref()
                .and_then(|value| value.downstream.as_ref())
                .map(|link| PathBuf::from(link.path.as_str())),
            conflict: None,
            reshade_source_path: None,
            reshade_source_sha256: None,
        },
    };
    let guard =
        crate::mutation_boundary::enter_game_mutation_boundary_async(request.context, &game_id)
            .await?;
    ensure_lifecycle_snapshot_unchanged(request.context, &game_id, &initial_snapshot)?;
    let availability =
        matcher::evaluation_off_runtime(request.context, request.manifest, &game_id).await?;
    ensure_apply_allowed(&availability)?;
    ensure_selected_modules_available(&availability, &modules)?;
    apply_release_off_runtime(&ApplyPlan {
        intent,
        context: request.context,
        guard,
        manifest: request.manifest.clone(),
        game_id,
        old_state: Some(old_state),
        old_managed_files,
        old_config_base,
        release: release.clone(),
        modules,
        artifacts: ArtifactSet {
            archive,
            module_artifacts,
            native_artifacts,
        },
        target,
        safety: request.safety,
        compatibility_invariants: availability.managed_ini_overrides,
        accepted_prerequisite_binding: availability.accepted_prerequisite_binding,
    })
}
