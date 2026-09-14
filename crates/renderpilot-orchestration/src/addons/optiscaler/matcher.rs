//! Eligibility, evidence confidence, compatibility rules, and proxy planning.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use renderpilot_application::{
    ArtifactRepository, ComponentRepository, OptiScalerStateRepository, ProxyTopologyRepository,
};
use renderpilot_domain::{
    AddonKind, Architecture, ComponentFile, GameId, LibraryArtifact,
    LibraryComponent as GraphicsComponent, LibraryTechnology as GraphicsTechnology,
    ManagedAddonFile, ManagedFileMode, PathRef, Sha256Hash,
};

use crate::addons::game_analysis::{analyze_game, install_target_dir};
use crate::addons::game_context::{executable_override, require_game};
use crate::{Context, ServiceError, failed};

use super::evaluation::{
    EvaluatedAvailability, EvaluatedModule, EvaluatedProxyPlan, EvaluatedRelocation,
};
mod proxy;
use proxy::{PlanningMode as ProxyPlanningMode, plan as proxy_plan};
mod capability;
mod compatibility;
mod drift;
use compatibility::{
    EvaluationMode as CompatibilityMode, evaluate as evaluate_compatibility, proxy_conflict_message,
};
mod modules;
use super::types::{
    OptiScalerCompatibilityBlockCode, OptiScalerManifest, OptiScalerModuleProvisioning,
    OptiScalerPrerequisiteState, OptiScalerRelease, OptiScalerRelocationBlockCode,
};
pub(crate) use capability::capability_available;
use drift::detect as drifted_paths;
#[cfg(test)]
use modules::{NativeModuleSpec, module_applies_to_game};
use modules::{ProjectionRequest, project as project_modules};
pub(crate) use modules::{
    best_native_module_artifact, module_artifact_for_release, module_is_installable,
    native_component_available, native_module_component, native_module_matches_path,
    native_module_spec, native_sr_is_redundant, reconcile_modules_for_release,
    validate_module_selection,
};

pub(crate) fn evaluate(
    context: &Context,
    manifest: &OptiScalerManifest,
    catalog: &super::compatibility_catalog::OptiScalerCompatibilityCatalog,
    game_id: &GameId,
) -> Result<EvaluatedAvailability, ServiceError> {
    let mut evaluation = availability_with_target(EvaluationRequest {
        context,
        manifest,
        catalog,
        game_id,
        target: EvaluationTarget::ConfiguredOrInstalled,
    })?;
    if let Some(target) = evaluation
        .relocation
        .as_ref()
        .map(|relocation| relocation.target_exe.clone())
    {
        let relocation = availability_with_target(EvaluationRequest {
            context,
            manifest,
            catalog,
            game_id,
            target: EvaluationTarget::Relocation(&target),
        })?;
        evaluation.relocation = Some(EvaluatedRelocation {
            target_exe: target,
            blocked_reason: relocation.blocked_reason.or(relocation.proxy.conflict),
            block_code: relocation.relocation_policy_block_code,
        });
    }
    Ok(evaluation)
}

/// Availability analysis boundary for OptiScaler lifecycle orchestrators.
///
/// Desktop commands run on background async tasks via the boundary runner,
/// while borrowed evaluation remains clean, synchronous, and memory-safe
/// without static allocation overhead.
pub(crate) async fn evaluation_off_runtime(
    context: &Context,
    manifest: &OptiScalerManifest,
    game_id: &GameId,
) -> Result<EvaluatedAvailability, ServiceError> {
    let catalog = super::compatibility_catalog::get_or_fetch_catalog().await?;
    evaluate(context, manifest, &catalog, game_id)
}

/// Relocation availability analysis boundary for OptiScaler lifecycle orchestrators.
pub(crate) async fn relocation_evaluation_off_runtime(
    context: &Context,
    manifest: &OptiScalerManifest,
    game_id: &GameId,
    target_exe: &Path,
) -> Result<EvaluatedAvailability, ServiceError> {
    let catalog = super::compatibility_catalog::get_or_fetch_catalog().await?;
    relocation_evaluation(context, manifest, &catalog, game_id, target_exe)
}

fn relocation_evaluation(
    context: &Context,
    manifest: &OptiScalerManifest,
    catalog: &super::compatibility_catalog::OptiScalerCompatibilityCatalog,
    game_id: &GameId,
    target_exe: &Path,
) -> Result<EvaluatedAvailability, ServiceError> {
    availability_with_target(EvaluationRequest {
        context,
        manifest,
        catalog,
        game_id,
        target: EvaluationTarget::Relocation(target_exe),
    })
}

#[derive(Clone, Copy)]
enum EvaluationTarget<'a> {
    ConfiguredOrInstalled,
    Relocation(&'a Path),
}

#[derive(Clone, Copy)]
struct EvaluationRequest<'a> {
    context: &'a Context,
    manifest: &'a OptiScalerManifest,
    catalog: &'a super::compatibility_catalog::OptiScalerCompatibilityCatalog,
    game_id: &'a GameId,
    target: EvaluationTarget<'a>,
}

fn availability_with_target(
    request: EvaluationRequest<'_>,
) -> Result<EvaluatedAvailability, ServiceError> {
    let EvaluationRequest {
        context,
        manifest,
        catalog,
        game_id,
        target,
    } = request;
    let game = require_game(context, game_id)?;
    let installed_state = context.storage().get_optiscaler_install_state(game_id)?;
    let configured_override = executable_override(context, game_id);
    let configured_analysis = analyze_game(&game, configured_override.as_deref());
    let (analysis, relocation_target_exe, proxy_mode) = match target {
        EvaluationTarget::ConfiguredOrInstalled => {
            let relocation_target_exe = installed_state.as_ref().and_then(|state| {
                configured_analysis
                    .primary_executable
                    .as_ref()
                    .and_then(|target| {
                        (!crate::paths::same_path(
                            Path::new(target.as_str()),
                            Path::new(state.target_exe_path.as_str()),
                        ))
                        .then(|| PathBuf::from(target.as_str()))
                    })
            });
            match installed_state.as_ref() {
                Some(state) => (
                    analyze_game(&game, Some(Path::new(state.target_exe_path.as_str()))),
                    relocation_target_exe,
                    ProxyPlanningMode::ExistingInstall,
                ),
                None => (
                    configured_analysis,
                    relocation_target_exe,
                    ProxyPlanningMode::Candidate,
                ),
            }
        }
        EvaluationTarget::Relocation(path) => (
            analyze_game(&game, Some(path)),
            None,
            ProxyPlanningMode::Candidate,
        ),
    };
    let target_dir = if analysis.primary_executable.is_some() {
        Some(install_target_dir(&analysis)?)
    } else {
        None
    };
    let components = context.storage().list_components_for_game(game_id)?;
    let managed_bindings = super::lifecycle::managed_bindings_from_state(installed_state.as_ref());
    let resolved = super::compatibility_catalog::resolve(catalog, &analysis.facts);
    let release = manifest.current_release();
    let compatibility = evaluate_compatibility(
        &components,
        resolved,
        release.is_some(),
        if proxy_mode == ProxyPlanningMode::ExistingInstall {
            CompatibilityMode::ManagedTarget
        } else {
            CompatibilityMode::Candidate
        },
    );
    let mut blocked_reason = compatibility.blocked_reason;
    let mut compatibility_block_code = compatibility.block_code;
    let evidence = compatibility.evidence;
    let accepted_prerequisite_binding = compatibility.accepted_prerequisite_binding;
    let maintenance_available = installed_state.is_some() && blocked_reason.is_none();
    let prerequisite = if compatibility.policy.is_some_and(|policy| {
        matches!(
            policy.prerequisite,
            super::compatibility_catalog::CompatibilityPrerequisite::Luma
        )
    }) {
        match super::super::luma::prerequisite::availability(context, game_id)? {
            super::super::luma::prerequisite::LumaPrerequisite::Satisfied => {
                OptiScalerPrerequisiteState::Satisfied
            }
            super::super::luma::prerequisite::LumaPrerequisite::RemoveRenoDx => {
                OptiScalerPrerequisiteState::RemoveRenoDx
            }
            super::super::luma::prerequisite::LumaPrerequisite::LumaTorn => {
                OptiScalerPrerequisiteState::LumaTorn
            }
            super::super::luma::prerequisite::LumaPrerequisite::LumaBroken => {
                OptiScalerPrerequisiteState::LumaBroken
            }
            super::super::luma::prerequisite::LumaPrerequisite::InstallLuma => {
                OptiScalerPrerequisiteState::InstallLuma
            }
            super::super::luma::prerequisite::LumaPrerequisite::LumaUnavailable => {
                OptiScalerPrerequisiteState::LumaUnavailable
            }
        }
    } else {
        OptiScalerPrerequisiteState::None
    };
    if proxy_mode == ProxyPlanningMode::Candidate
        && !matches!(
            prerequisite,
            OptiScalerPrerequisiteState::None | OptiScalerPrerequisiteState::Satisfied
        )
        && blocked_reason.is_none()
    {
        compatibility_block_code = Some(OptiScalerCompatibilityBlockCode::LumaPrerequisite);
        blocked_reason = Some(
            "the selected OptiScaler compatibility entry requires a healthy Luma installation"
                .to_owned(),
        );
    }

    let catalog_policy = if proxy_mode == ProxyPlanningMode::Candidate
        || maintenance_policy_applies(
            context,
            game_id,
            compatibility.status,
            compatibility.policy,
            prerequisite,
        )? {
        compatibility.policy
    } else {
        None
    };
    let proxy = proxy_plan(
        context,
        manifest,
        game_id,
        target_dir.as_deref(),
        catalog_policy,
        proxy_mode,
    )?;
    if proxy.conflict.is_some() && blocked_reason.is_none() {
        blocked_reason.clone_from(&proxy.conflict);
    }
    let relocation_policy_block_code = if matches!(target, EvaluationTarget::Relocation(_))
        && cross_target_relocation_has_active_downstream(
            installed_state
                .as_ref()
                .map(|state| Path::new(state.target_dir.as_str())),
            target_dir.as_deref(),
            context
                .storage()
                .get_proxy_topology(game_id)?
                .is_some_and(|topology| topology.downstream.is_some()),
        ) {
        if blocked_reason.is_none() {
            blocked_reason = Some(
                "moving OptiScaler to another runtime directory requires uninstalling the active RenoDX or Luma peer first"
                    .to_owned(),
            );
        }
        Some(OptiScalerRelocationBlockCode::ActivePeerCrossTargetUnsupported)
    } else {
        None
    };
    let modules = project_modules(&ProjectionRequest {
        context,
        manifest,
        release,
        components: &components,
        installed_state: installed_state.as_ref(),
        managed_bindings: &managed_bindings,
        rule: catalog_policy,
        evidence: &evidence,
    })?
    .modules;
    let selected_modules_unavailable = modules
        .iter()
        .any(|module| module.selected && !module.available);
    if selected_modules_unavailable && blocked_reason.is_none() {
        compatibility_block_code =
            Some(OptiScalerCompatibilityBlockCode::SelectedModulesUnavailable);
        blocked_reason = Some(
            "one or more selected OptiScaler modules are unavailable for this release or game"
                .to_owned(),
        );
    }
    let drift = installed_state
        .as_ref()
        .map(|state| {
            drifted_paths(
                context,
                manifest,
                state,
                &proxy.slot,
                catalog_policy.map_or(&[], |policy| policy.ini_overrides.as_slice()),
            )
        })
        .transpose()?
        .unwrap_or_default();
    let update_available = installed_state
        .as_ref()
        .zip(release)
        .is_some_and(|(state, release)| state.release_id != release.id);
    let module_reconciliation_required = match (installed_state.as_ref(), release) {
        (Some(state), Some(release)) => {
            let mut reconciled =
                reconcile_modules_for_release(manifest, &release.id, &state.modules)?;
            if native_sr_is_redundant(&components, &managed_bindings) {
                reconciled.remove("nvidia_sr");
            }
            reconciled.len() != state.modules.len()
                || state
                    .modules
                    .iter()
                    .any(|module| !reconciled.contains(module))
        }
        _ => false,
    };
    let repair_required = module_reconciliation_required || drift.repair_required();
    let unmanaged = installed_state.is_none()
        && target_dir
            .as_deref()
            .is_some_and(super::tool::unmanaged_install_present);
    Ok(EvaluatedAvailability {
        game_id: game_id.clone(),
        launcher: analysis.facts.launcher,
        blocked_reason,
        compatibility_block_code,
        detected_apis: analysis.facts.graphics.apis().to_vec(),
        accepted_prerequisite_binding,
        compatibility: super::types::OptiScalerCompatibility {
            status: compatibility.status,
            declared_inputs: compatibility.declared_inputs,
            launch: compatibility.launch,
            guidance: compatibility.guidance,
        },
        prerequisite,
        selected_release: release.map(|release| release.id.clone()),
        target_exe: analysis
            .primary_executable
            .map(|path| PathBuf::from(path.as_str())),
        relocation: relocation_target_exe.map(|target_exe| EvaluatedRelocation {
            target_exe,
            blocked_reason: None,
            block_code: None,
        }),
        relocation_policy_block_code,
        target_dir,
        proxy,
        modules,
        install_state: installed_state,
        drifted_paths: drift.paths,
        update_available,
        repair_required,
        unmanaged,
        managed_ini_overrides: catalog_policy
            .map_or_else(Vec::new, |policy| policy.ini_overrides.clone()),
        maintenance_available,
        maintenance_block_code: compatibility_block_code,
    })
}

fn cross_target_relocation_has_active_downstream(
    installed_target_dir: Option<&Path>,
    candidate_target_dir: Option<&Path>,
    has_active_downstream: bool,
) -> bool {
    has_active_downstream
        && installed_target_dir
            .zip(candidate_target_dir)
            .is_some_and(|(installed, candidate)| !crate::paths::same_path(installed, candidate))
}

fn maintenance_policy_applies(
    context: &Context,
    game_id: &GameId,
    status: super::types::OptiScalerCompatibilityStatus,
    policy: Option<&super::compatibility_catalog::ResolvedVariant>,
    prerequisite: OptiScalerPrerequisiteState,
) -> Result<bool, ServiceError> {
    let Some(policy) = policy else {
        return Ok(false);
    };
    if !matches!(
        status,
        super::types::OptiScalerCompatibilityStatus::Working
            | super::types::OptiScalerCompatibilityStatus::Conditional
    ) || (matches!(
        policy.prerequisite,
        super::compatibility_catalog::CompatibilityPrerequisite::Luma
    ) && prerequisite != OptiScalerPrerequisiteState::Satisfied)
    {
        return Ok(false);
    }
    match &policy.proxy {
        super::compatibility_catalog::ProxyPolicy::Automatic => Ok(true),
        super::compatibility_catalog::ProxyPolicy::Exact { slot } => Ok(context
            .storage()
            .get_proxy_topology(game_id)?
            .is_some_and(|topology| {
                Path::new(topology.root_slot.as_str())
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|current| current.eq_ignore_ascii_case(slot))
            })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use renderpilot_application::GameRepository;
    use renderpilot_domain::{
        ComponentId, ComponentKind, GameId, GameIdentity, GameInstallation, GameRuntime, Launcher,
        ManagedAddonFile, ManagedFileBaseline, Platform, Sha256Hash, Swappability,
    };
    use sha2::Digest;
    use tempfile::tempdir;

    #[test]
    fn active_downstream_blocks_only_cross_directory_relocation() {
        let installed = Path::new("C:/Games/Test/Binaries/Win64");
        let same_directory = Path::new("c:\\games\\test\\binaries\\win64");
        let other_directory = Path::new("C:/Games/Test/Bin");

        assert!(!cross_target_relocation_has_active_downstream(
            Some(installed),
            Some(same_directory),
            true,
        ));
        assert!(cross_target_relocation_has_active_downstream(
            Some(installed),
            Some(other_directory),
            true,
        ));
        assert!(!cross_target_relocation_has_active_downstream(
            Some(installed),
            Some(other_directory),
            false,
        ));
    }

    #[test]
    fn optiscaler_chains_a_single_reshade_from_its_actual_existing_slot() {
        let db = tempdir().expect("db");
        let game_dir = tempdir().expect("game dir");
        let context = Context::open_at(db.path().join("catalog.sqlite")).expect("context");
        let game_id = GameId::new("manual:optiscaler-existing-reshade").expect("game id");
        let source = game_dir.path().join("d3d12.dll");
        std::fs::write(
            &source,
            crate::addons::test_support::build_pe_with_exports(
                crate::addons::test_support::MACHINE_AMD64,
                crate::addons::test_support::PE32_PLUS_MAGIC,
                &["ReShadeVersion"],
            ),
        )
        .expect("reshade");

        let manifest = super::super::manifest_store::parse_manifest(include_bytes!(
            "../../../assets/optiscaler-fallback.json"
        ))
        .expect("bundled manifest");
        let plan = proxy_plan(
            &context,
            &manifest,
            &game_id,
            Some(game_dir.path()),
            None,
            ProxyPlanningMode::Candidate,
        )
        .expect("proxy plan");

        assert!(plan.chain_reshade);
        assert!(plan.conflict.is_none());
        assert!(crate::paths::same_path(&plan.slot, &source,));
        assert!(crate::paths::same_path(
            plan.reshade_source_path.as_deref().expect("source"),
            &source
        ));
        assert!(crate::paths::same_path(
            plan.downstream_path.as_deref().expect("downstream"),
            &game_dir.path().join("ReShade64.dll")
        ));
        let expected_hash = renderpilot_detection::sha256_file(&source).expect("hash");
        assert_eq!(plan.reshade_source_sha256.as_ref(), Some(&expected_hash));
    }

    #[test]
    fn known_unmanaged_optiscaler_outer_is_not_a_proxy_conflict_with_reshade() {
        let db = tempdir().expect("db");
        let game_dir = tempdir().expect("game dir");
        let context = Context::open_at(db.path().join("catalog.sqlite")).expect("context");
        let game_id = GameId::new("manual:known-optiscaler-chain").expect("game id");
        let outer = game_dir.path().join("dxgi.dll");
        let downstream = game_dir.path().join("ReShade64.dll");
        std::fs::write(
            &outer,
            crate::addons::test_support::build_pe_with_exports(
                crate::addons::test_support::MACHINE_AMD64,
                crate::addons::test_support::PE32_PLUS_MAGIC,
                &[],
            ),
        )
        .expect("outer loader");
        std::fs::write(
            &downstream,
            crate::addons::test_support::build_pe_with_exports(
                crate::addons::test_support::MACHINE_AMD64,
                crate::addons::test_support::PE32_PLUS_MAGIC,
                &["ReShadeVersion"],
            ),
        )
        .expect("reshade");
        std::fs::write(game_dir.path().join("ReShade.ini"), b"[GENERAL]\n")
            .expect("reshade config");
        let outer_hash = renderpilot_detection::sha256_file(&outer).expect("outer hash");
        let manifest = super::super::manifest_store::parse_manifest(include_bytes!(
            "../../../assets/optiscaler-fallback.json"
        ))
        .expect("bundled manifest");
        let mut wire = manifest.wire_clone();
        let proxy_member = wire
            .releases
            .first_mut()
            .and_then(|release| {
                release
                    .members
                    .iter_mut()
                    .find(|member| member.target == "$proxy")
            })
            .expect("proxy member");
        proxy_member.sha256 = outer_hash.as_str().to_owned();
        let manifest =
            super::super::types::OptiScalerManifest::try_from(wire).expect("valid proxy fixture");

        let plan = proxy_plan(
            &context,
            &manifest,
            &game_id,
            Some(game_dir.path()),
            None,
            ProxyPlanningMode::Candidate,
        )
        .expect("proxy plan");

        assert!(plan.conflict.is_none());
        assert!(plan.chain_reshade);
        assert!(crate::paths::same_path(
            plan.downstream_path.as_deref().expect("downstream"),
            &downstream
        ));
    }

    #[tokio::test]
    async fn database_loss_reconciles_a_known_optiscaler_and_reshade_chain() {
        let db = tempdir().expect("db");
        let game_dir = tempdir().expect("game dir");
        let context = Context::open_at(db.path().join("catalog.sqlite")).expect("context");
        let game_id = GameId::new("manual:known-optiscaler-reconciliation").expect("game id");
        let executable = game_dir.path().join("Game.exe");
        let outer = game_dir.path().join("dxgi.dll");
        let downstream = game_dir.path().join("ReShade64.dll");
        let plain_pe = crate::addons::test_support::build_pe_with_exports(
            crate::addons::test_support::MACHINE_AMD64,
            crate::addons::test_support::PE32_PLUS_MAGIC,
            &[],
        );
        std::fs::write(&executable, &plain_pe).expect("game executable");
        std::fs::write(&outer, &plain_pe).expect("outer loader");
        std::fs::write(
            &downstream,
            crate::addons::test_support::build_pe_with_exports(
                crate::addons::test_support::MACHINE_AMD64,
                crate::addons::test_support::PE32_PLUS_MAGIC,
                &["ReShadeVersion"],
            ),
        )
        .expect("reshade");
        std::fs::write(game_dir.path().join("OptiScaler.ini"), b"[OptiScaler]\n")
            .expect("optiscaler config");
        std::fs::write(game_dir.path().join("ReShade.ini"), b"[GENERAL]\n")
            .expect("reshade config");

        let game = GameInstallation::new(
            GameIdentity::new(game_id.clone(), "Known OptiScaler", Launcher::Manual)
                .expect("game identity"),
            Platform::Windows,
            GameRuntime::NativeWindows,
            PathRef::new(game_dir.path().to_string_lossy().into_owned()).expect("game path"),
        )
        .with_executable_candidate(
            PathRef::new(executable.to_string_lossy().into_owned()).expect("executable path"),
        );
        context.storage().upsert_game(&game).expect("game");

        let manifest = super::super::manifest_store::parse_manifest(include_bytes!(
            "../../../assets/optiscaler-fallback.json"
        ))
        .expect("bundled manifest");
        let mut wire = manifest.wire_clone();
        let proxy_member = wire
            .releases
            .first_mut()
            .and_then(|release| {
                release
                    .members
                    .iter_mut()
                    .find(|member| member.target == "$proxy")
            })
            .expect("proxy member");
        proxy_member.sha256 = renderpilot_detection::sha256_file(&outer)
            .expect("outer hash")
            .as_str()
            .to_owned();
        let manifest =
            super::super::types::OptiScalerManifest::try_from(wire).expect("valid proxy fixture");

        let availability = super::super::availability_with_manifest(&context, &manifest, &game_id)
            .await
            .expect("availability reconciliation");

        assert!(availability.install_state.is_some());
        let topology = context
            .storage()
            .get_proxy_topology(&game_id)
            .expect("topology lookup")
            .expect("reconciled topology");
        assert!(crate::paths::same_path(
            Path::new(topology.root_slot.as_str()),
            &outer
        ));
        assert!(crate::paths::same_path(
            Path::new(
                topology
                    .downstream
                    .as_ref()
                    .expect("downstream link")
                    .path
                    .as_str()
            ),
            &downstream
        ));
    }

    #[test]
    fn unrecognized_reserved_downstream_slot_remains_a_conflict() {
        let db = tempdir().expect("db");
        let game_dir = tempdir().expect("game dir");
        let context = Context::open_at(db.path().join("catalog.sqlite")).expect("context");
        let game_id = GameId::new("manual:unknown-downstream").expect("game id");
        std::fs::write(game_dir.path().join("ReShade64.dll"), b"unknown proxy")
            .expect("unknown downstream");
        let manifest = super::super::manifest_store::parse_manifest(include_bytes!(
            "../../../assets/optiscaler-fallback.json"
        ))
        .expect("bundled manifest");

        let plan = proxy_plan(
            &context,
            &manifest,
            &game_id,
            Some(game_dir.path()),
            None,
            ProxyPlanningMode::Candidate,
        )
        .expect("proxy plan");

        assert!(plan.conflict.is_some());
        assert!(!plan.chain_reshade);
    }

    #[test]
    fn native_modules_bind_to_the_existing_graphics_catalog() {
        assert_eq!(
            native_module_spec("nvidia_sr"),
            Some(NativeModuleSpec {
                technology: GraphicsTechnology::DlssSuperResolution,
            })
        );
        assert_eq!(native_module_spec("streamline"), None);
        assert_eq!(native_module_spec("nvidia_fg"), None);
        assert_eq!(native_module_spec("nvidia_rr"), None);
        assert_eq!(native_module_spec("optipatcher"), None);
    }

    #[test]
    fn native_nvidia_outputs_are_hidden_when_the_game_already_has_dlss() {
        let dlss = GraphicsComponent::new(
            ComponentId::new("component:dlss:input").expect("component id"),
            GameId::new("steam:dlss-input").expect("game id"),
            ComponentKind::NativeLibrary,
            GraphicsTechnology::DlssSuperResolution,
            Swappability::Swappable,
        );
        let components = [dlss];
        assert!(!module_applies_to_game(
            "nvidia_sr",
            &components,
            &HashSet::new(),
            true
        ));
    }

    #[test]
    fn nvidia_sr_requires_an_nvidia_runtime_and_a_non_dlss_input() {
        let fsr = GraphicsComponent::new(
            ComponentId::new("component:fsr:input").expect("component id"),
            GameId::new("steam:fsr-input").expect("game id"),
            ComponentKind::NativeLibrary,
            GraphicsTechnology::AmdFsr,
            Swappability::ReadOnly,
        );
        let components = [fsr];
        assert!(module_applies_to_game(
            "nvidia_sr",
            &components,
            &HashSet::new(),
            true
        ));
        assert!(!module_applies_to_game(
            "nvidia_sr",
            &components,
            &HashSet::new(),
            false
        ));
    }

    #[test]
    fn native_module_availability_uses_detected_component_paths_not_the_exe_root() {
        let root = tempdir().expect("tempdir");
        let nested = root.path().join("bin").join("third_party");
        std::fs::create_dir_all(&nested).expect("nested dir");
        let pe = crate::addons::test_support::build_pe_with_exports(
            crate::addons::test_support::MACHINE_AMD64,
            crate::addons::test_support::PE32_PLUS_MAGIC,
            &[],
        );
        let dlss = nested.join("nvngx_dlss.dll");
        std::fs::write(&dlss, &pe).expect("dlss fixture");
        let component = GraphicsComponent::new(
            ComponentId::new("component:dlss:nested").expect("component id"),
            GameId::new("steam:nested").expect("game id"),
            ComponentKind::NativeLibrary,
            GraphicsTechnology::DlssSuperResolution,
            Swappability::Swappable,
        )
        .with_file(ComponentFile::new(
            PathRef::new(dlss.to_string_lossy().into_owned()).expect("path"),
        ));

        assert!(native_component_available(&[component], "nvidia_sr"));
        assert!(!native_component_available(&[], "nvidia_sr"));
    }

    #[test]
    fn native_sr_is_redundant_only_when_the_game_owns_the_detected_dlss() {
        let root = tempdir().expect("tempdir");
        let dlss = root.path().join("nvngx_dlss.dll");
        let game_id = GameId::new("manual:native-sr-ownership").expect("game id");
        let component = GraphicsComponent::new(
            ComponentId::new("component:dlss:ownership").expect("component id"),
            game_id,
            ComponentKind::NativeLibrary,
            GraphicsTechnology::DlssSuperResolution,
            Swappability::Swappable,
        )
        .with_file(ComponentFile::new(
            PathRef::new(dlss.to_string_lossy().into_owned()).expect("DLSS path"),
        ));
        let hash = Sha256Hash::new(hex::encode(sha2::Sha256::digest(b"dlss"))).expect("hash");
        let owned = ManagedAddonFile::owned(
            PathRef::new(dlss.to_string_lossy().into_owned()).expect("owned path"),
            ManagedFileBaseline::Absent,
            hash.clone(),
        );
        let reused = ManagedAddonFile::reused(
            PathRef::new(dlss.to_string_lossy().into_owned()).expect("reused path"),
            hash,
        );

        assert!(!native_sr_is_redundant(
            std::slice::from_ref(&component),
            std::slice::from_ref(&owned)
        ));
        assert!(native_sr_is_redundant(
            &[component],
            std::slice::from_ref(&reused)
        ));
    }

    #[test]
    fn release_reconciliation_drops_modules_removed_from_the_manifest() {
        let manifest = super::super::manifest_store::parse_manifest(include_bytes!(
            "../../../assets/optiscaler-fallback.json"
        ))
        .expect("bundled manifest");
        let selected = reconcile_modules_for_release(
            &manifest,
            "v0.9.4",
            &["core".to_owned(), "nvidia_rr".to_owned()],
        )
        .expect("reconciled selection");

        assert!(selected.contains("core"));
        assert!(!selected.contains("nvidia_rr"));
    }
}
