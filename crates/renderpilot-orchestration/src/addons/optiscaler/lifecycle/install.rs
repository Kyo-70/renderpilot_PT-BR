//! Fresh install and exact-adoption command orchestration.

use super::*;

/// Installs one immutable OptiScaler release through a durable file transaction.
pub async fn install(
    request: InstallOptiScalerRequest<'_>,
) -> Result<OptiScalerOperationResult, ServiceError> {
    let game_id = request.safety.game_id().clone();
    let intent = ApplyIntent::Install {
        modules: request.modules.map(<[String]>::to_vec),
    };
    let (availability, initial_snapshot) = {
        let _guard =
            crate::mutation_boundary::enter_game_mutation_boundary_async(request.context, &game_id)
                .await?;
        let snapshot = capture_lifecycle_snapshot(request.context, &game_id)?;
        let availability =
            matcher::evaluate(request.context, request.manifest, request.catalog, &game_id)?;
        (availability, snapshot)
    };
    ensure_fresh_install_snapshot(&initial_snapshot)?;
    if availability.unmanaged {
        ensure_adoption_allowed(&availability)?;
        if let Some(result) = adopt_exact(
            request.context,
            request.manifest,
            request.catalog,
            &game_id,
            AdoptionPolicy::UserRequested,
            &availability,
        )
        .await?
        {
            return Ok(result);
        }
        return Err(failed(
            "the unmanaged OptiScaler files do not match one known release; preserve them and use manual cleanup",
        ));
    }
    ensure_apply_allowed(&availability)?;
    let release = current_release(request.manifest)?;
    let modules = intent.requested_modules().map_or_else(
        || {
            let requested = availability
                .modules
                .iter()
                .filter(|module| module.selected)
                .map(|module| module.id.clone())
                .collect::<Vec<_>>();
            validate_module_selection(request.manifest, &release.id, &requested)
        },
        |modules| validate_module_selection(request.manifest, &release.id, modules),
    )?;
    ensure_selected_modules_available(&availability, &modules)?;
    let target = target_from_availability(&availability)?;
    // Fail closed on ambiguous peer receipts before any release or
    // private-module download begins. The under-lock apply path repeats this
    // exact preflight immediately before custody preparation.
    apply::plan_apply_peer_transition(
        request.context,
        &game_id,
        initial_snapshot.state.as_ref(),
        initial_snapshot.topology.as_ref(),
        &target.proxy,
    )?;
    let archive = download_and_stage(release, &modules, request.progress).await?;
    let module_artifacts =
        prepare_selected(request.manifest, release, &modules, request.progress).await?;
    let native_artifacts =
        prepare_native_modules(request.context, &game_id, &modules, request.progress).await?;
    crate::addons::progress::emit_tool_finalizing(request.progress, AddonKind::OptiScaler);

    let guard =
        crate::mutation_boundary::enter_game_mutation_boundary_async(request.context, &game_id)
            .await?;
    ensure_lifecycle_snapshot_unchanged(request.context, &game_id, &initial_snapshot)?;
    let revalidated =
        matcher::evaluate(request.context, request.manifest, request.catalog, &game_id)?;
    ensure_apply_allowed(&revalidated)?;
    ensure_target_unchanged(&target, &revalidated)?;
    ensure_selected_modules_available(&revalidated, &modules)?;
    apply_release_off_runtime(&ApplyPlan {
        intent,
        context: request.context,
        guard,
        manifest: request.manifest.clone(),
        game_id,
        old_state: None,
        old_managed_files: Vec::new(),
        old_config_base: None,
        release: release.clone(),
        modules,
        artifacts: ArtifactSet {
            archive,
            module_artifacts,
            native_artifacts,
        },
        target,
        safety: request.safety,
        compatibility_invariants: revalidated.managed_ini_overrides,
        accepted_prerequisite_binding: revalidated.accepted_prerequisite_binding,
    })
}
