//! Explicit managed-install relocation command.

use super::*;

/// Moves a managed installation only to the explicitly selected executable.
pub async fn relocate(
    request: RelocateOptiScalerRequest<'_>,
) -> Result<OptiScalerOperationResult, ServiceError> {
    let game_id = request.safety.game_id().clone();
    let initial_snapshot = {
        let _guard =
            crate::mutation_boundary::enter_game_mutation_boundary_async(request.context, &game_id)
                .await?;
        let snapshot = capture_lifecycle_snapshot(request.context, &game_id)?;
        ensure_managed_install_snapshot(&snapshot)?;
        snapshot
    };
    let state = initial_snapshot
        .state
        .clone()
        .ok_or_else(|| failed("OptiScaler is not installed for this game"))?;
    let old_managed_files = managed_bindings_from_state(Some(&state));
    if !request.target_exe.is_file() {
        return Err(failed("the relocation executable does not exist"));
    }
    let game = crate::addons::game_context::require_game(request.context, &game_id)?;
    let install_root = crate::paths::canonicalize_existing(Path::new(game.install_path().as_str()))
        .map_err(|error| failed(format!("failed to resolve game install root: {error}")))?;
    let canonical_target = crate::paths::canonicalize_existing(request.target_exe)
        .map_err(|error| failed(format!("failed to resolve relocation executable: {error}")))?;
    if !crate::paths::is_within(&canonical_target, &install_root) {
        return Err(failed(
            "the relocation executable is outside the selected game installation",
        ));
    }
    let intent = ApplyIntent::Relocate {
        target_exe: canonical_target,
    };
    let relocation_target = intent
        .relocation_target()
        .ok_or_else(|| failed("missing relocation target"))?;
    let release = request
        .manifest
        .releases
        .iter()
        .find(|release| release.id == state.release_id)
        .ok_or_else(|| failed("installed OptiScaler release is absent from the manifest"))?;
    let modules = validate_module_selection(request.manifest, &release.id, &state.modules)?;
    let availability = matcher::relocation_evaluation_off_runtime(
        request.context,
        request.manifest,
        &game_id,
        relocation_target,
    )
    .await?;
    ensure_apply_allowed(&availability)?;
    // Same-directory changes may coordinate the exact topology-owned host.
    // Cross-directory moves with any active downstream are rejected by the
    // availability evaluation above and revalidated again under the game lock;
    // generic peer receipts do not contain enough per-file evidence to move the
    // rest of a RenoDX/Luma installation safely.
    ensure_selected_modules_available(&availability, &modules)?;
    let target = target_from_availability(&availability)?;
    // Validate peer receipt ownership and sidecar provenance before network
    // acquisition. The guarded apply path repeats this under the live lock.
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
    let revalidated = matcher::relocation_evaluation_off_runtime(
        request.context,
        request.manifest,
        &game_id,
        relocation_target,
    )
    .await?;
    ensure_apply_allowed(&revalidated)?;
    ensure_target_unchanged(&target, &revalidated)?;
    ensure_selected_modules_available(&revalidated, &modules)?;
    apply_release_off_runtime(&ApplyPlan {
        intent,
        context: request.context,
        guard,
        manifest: request.manifest.clone(),
        game_id,
        old_state: Some(state),
        old_managed_files,
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
