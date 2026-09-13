//! Desktop facade for the manifest-driven OptiScaler lifecycle.

use std::path::Path;

use renderpilot_orchestration::Context;
use renderpilot_orchestration::addons::optiscaler::{
    self, InstallOptiScalerRequest, RelocateOptiScalerRequest, SetOptiScalerModulesRequest,
    UpdateOptiScalerRequest,
};
use renderpilot_orchestration::net::ProgressObserver;
use serde::Serialize;

use crate::utils::{JsonResult, parse_game_id, to_json};

#[derive(Clone, Copy)]
enum UpdateMutation {
    Update,
    Repair,
}

#[derive(Serialize)]
struct DesktopOptiScalerInstall {
    installed: bool,
    release: Option<String>,
}

#[derive(Serialize)]
struct DesktopOptiScalerEligibility {
    available: bool,
    block_code: Option<optiscaler::types::OptiScalerCompatibilityBlockCode>,
}

#[derive(Serialize)]
struct DesktopOptiScalerRelocation {
    target_exe: String,
    display_name: String,
    available: bool,
    block_code: Option<optiscaler::types::OptiScalerRelocationBlockCode>,
}

#[derive(Serialize)]
struct DesktopOptiScalerModule {
    id: String,
    selected: bool,
    optional: bool,
    available: bool,
    requires: Vec<String>,
    conflicts: Vec<String>,
    description: String,
}

#[derive(Serialize)]
struct DesktopOptiScalerLifecycle {
    update_available: bool,
    repair_required: bool,
    drifted: bool,
    unmanaged: bool,
    maintenance_available: bool,
    maintenance_block_code: Option<optiscaler::types::OptiScalerCompatibilityBlockCode>,
}

#[derive(Serialize)]
struct DesktopOptiScalerCompatibility {
    status: optiscaler::types::OptiScalerCompatibilityStatus,
    declared_inputs: Vec<optiscaler::types::OptiScalerDeclaredInput>,
    launch: Option<DesktopOptiScalerLaunch>,
    guidance: Vec<optiscaler::types::OptiScalerCompatibilityGuidance>,
}

#[derive(Serialize)]
struct DesktopOptiScalerLaunch {
    arguments: Vec<String>,
    requirement: optiscaler::types::OptiScalerLaunchRequirement,
}

impl From<optiscaler::types::OptiScalerLaunch> for DesktopOptiScalerLaunch {
    fn from(value: optiscaler::types::OptiScalerLaunch) -> Self {
        Self {
            arguments: value.arguments,
            requirement: value.requirement,
        }
    }
}

#[derive(Serialize)]
struct DesktopOptiScalerPrerequisite {
    state: optiscaler::types::OptiScalerPrerequisiteState,
}

#[derive(Serialize)]
struct DesktopOptiScalerAvailability {
    game_id: String,
    launcher: renderpilot_orchestration::domain::Launcher,
    install: DesktopOptiScalerInstall,
    eligibility: DesktopOptiScalerEligibility,
    selected_release: Option<String>,
    relocation: Option<DesktopOptiScalerRelocation>,
    proxy_conflict: Option<String>,
    compatibility: DesktopOptiScalerCompatibility,
    prerequisite: DesktopOptiScalerPrerequisite,
    modules: Vec<DesktopOptiScalerModule>,
    lifecycle: DesktopOptiScalerLifecycle,
}

impl From<optiscaler::types::OptiScalerAvailability> for DesktopOptiScalerAvailability {
    fn from(value: optiscaler::types::OptiScalerAvailability) -> Self {
        let relocation = value.relocation_target_exe.map(|target_exe| {
            let display_name = Path::new(&target_exe)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(&target_exe)
                .to_owned();
            DesktopOptiScalerRelocation {
                target_exe,
                display_name,
                available: value.relocation_blocked_reason.is_none(),
                block_code: value.relocation_block_code,
            }
        });
        let release = value.install_state.map(|state| state.release_id);
        let installed = release.is_some();
        Self {
            game_id: value.game_id,
            launcher: value.launcher,
            install: DesktopOptiScalerInstall { installed, release },
            eligibility: DesktopOptiScalerEligibility {
                available: value.available,
                block_code: value.compatibility_block_code,
            },
            selected_release: value.selected_release,
            relocation,
            proxy_conflict: value.proxy.conflict,
            compatibility: DesktopOptiScalerCompatibility {
                status: value.compatibility.status,
                declared_inputs: value.compatibility.declared_inputs,
                launch: value.compatibility.launch.map(Into::into),
                guidance: value.compatibility.guidance,
            },
            prerequisite: DesktopOptiScalerPrerequisite {
                state: value.prerequisite,
            },
            modules: value
                .modules
                .into_iter()
                .map(|module| DesktopOptiScalerModule {
                    id: module.id,
                    selected: module.selected,
                    optional: module.optional,
                    available: module.available,
                    requires: module.requires,
                    conflicts: module.conflicts,
                    description: module.description,
                })
                .collect(),
            lifecycle: DesktopOptiScalerLifecycle {
                update_available: value.update_available,
                repair_required: value.repair_required,
                drifted: !value.drifted_paths.is_empty(),
                unmanaged: value.unmanaged,
                maintenance_available: value.maintenance_available,
                maintenance_block_code: value.maintenance_block_code,
            },
        }
    }
}

#[derive(Serialize)]
struct DesktopOptiScalerOperation {
    installed: bool,
    release: Option<String>,
    changed_count: usize,
    preserved_count: usize,
    config_conflicts: Vec<String>,
}

impl From<optiscaler::types::OptiScalerOperationResult> for DesktopOptiScalerOperation {
    fn from(value: optiscaler::types::OptiScalerOperationResult) -> Self {
        Self {
            installed: value.state.is_some(),
            release: value.state.map(|state| state.release_id),
            changed_count: value.changed_paths.len(),
            preserved_count: value.preserved_paths.len(),
            config_conflicts: value.config_conflicts,
        }
    }
}

#[derive(Serialize)]
struct DesktopOptiScalerUpdateCheck {
    overall: renderpilot_orchestration::addons::update::UpdateStatus,
    installed_release: Option<String>,
    available_release: Option<String>,
    update_available: bool,
    repair_required: bool,
    drifted: bool,
}

impl From<optiscaler::types::OptiScalerUpdateCheck> for DesktopOptiScalerUpdateCheck {
    fn from(value: optiscaler::types::OptiScalerUpdateCheck) -> Self {
        Self {
            overall: value.overall,
            installed_release: value.installed_release,
            available_release: value.available_release,
            update_available: value.update_available,
            repair_required: value.repair_required,
            drifted: !value.drifted_paths.is_empty(),
        }
    }
}

/// Returns eligibility, reviewed compatibility, release/module choices, topology and warnings.
pub async fn get_optiscaler_availability(
    context: &Context,
    game_id: impl Into<String>,
) -> JsonResult {
    let game_id = parse_game_id(game_id)?;
    to_json(DesktopOptiScalerAvailability::from(
        optiscaler::availability(context, &game_id).await?,
    ))
}

/// Installs one immutable release snapshot and its selected module closure.
pub async fn install_optiscaler(
    context: &Context,
    game_id: impl Into<String>,
    modules: Option<&[String]>,
    game_context_token: Option<String>,
    progress: Option<&ProgressObserver<'_>>,
) -> JsonResult {
    let game_id = parse_game_id(game_id)?;
    let safety = renderpilot_orchestration::FileSafetyAuthority::new()
        .game_permit(game_id, game_context_token.as_deref())?;
    to_json(DesktopOptiScalerOperation::from(
        optiscaler::install(InstallOptiScalerRequest {
            context,
            modules,
            safety,
            progress,
        })
        .await?,
    ))
}

/// Checks immutable release identity and on-disk managed invariants.
pub async fn check_optiscaler_update(context: &Context, game_id: impl Into<String>) -> JsonResult {
    let game_id = parse_game_id(game_id)?;
    to_json(DesktopOptiScalerUpdateCheck::from(
        optiscaler::check_update(context, &game_id).await?,
    ))
}

/// Updates OptiScaler to the current stable release.
pub async fn update_optiscaler(
    context: &Context,
    game_id: impl Into<String>,
    game_context_token: Option<String>,
    progress: Option<&ProgressObserver<'_>>,
) -> JsonResult {
    mutate(
        context,
        game_id,
        game_context_token,
        progress,
        UpdateMutation::Update,
    )
    .await
}

/// Reconverges every selected file and managed configuration invariant.
pub async fn repair_optiscaler(
    context: &Context,
    game_id: impl Into<String>,
    game_context_token: Option<String>,
    progress: Option<&ProgressObserver<'_>>,
) -> JsonResult {
    mutate(
        context,
        game_id,
        game_context_token,
        progress,
        UpdateMutation::Repair,
    )
    .await
}

async fn mutate(
    context: &Context,
    game_id: impl Into<String>,
    game_context_token: Option<String>,
    progress: Option<&ProgressObserver<'_>>,
    mutation: UpdateMutation,
) -> JsonResult {
    let game_id = parse_game_id(game_id)?;
    let safety = renderpilot_orchestration::FileSafetyAuthority::new()
        .game_permit(game_id, game_context_token.as_deref())?;
    let request = UpdateOptiScalerRequest {
        context,
        safety,
        progress,
    };
    let result = match mutation {
        UpdateMutation::Update => optiscaler::update(request).await?,
        UpdateMutation::Repair => optiscaler::repair(request).await?,
    };
    to_json(DesktopOptiScalerOperation::from(result))
}

/// Applies an explicit module selection after dependency closure validation.
pub async fn set_optiscaler_modules(
    context: &Context,
    game_id: impl Into<String>,
    modules: &[String],
    game_context_token: Option<String>,
    progress: Option<&ProgressObserver<'_>>,
) -> JsonResult {
    let game_id = parse_game_id(game_id)?;
    let safety = renderpilot_orchestration::FileSafetyAuthority::new()
        .game_permit(game_id, game_context_token.as_deref())?;
    to_json(DesktopOptiScalerOperation::from(
        optiscaler::set_modules(SetOptiScalerModulesRequest {
            context,
            modules,
            safety,
            progress,
        })
        .await?,
    ))
}

/// Moves the managed install only after an explicit executable selection.
pub async fn relocate_optiscaler(
    context: &Context,
    game_id: impl Into<String>,
    target_exe: impl AsRef<Path>,
    game_context_token: Option<String>,
    progress: Option<&ProgressObserver<'_>>,
) -> JsonResult {
    let game_id = parse_game_id(game_id)?;
    let safety = renderpilot_orchestration::FileSafetyAuthority::new()
        .game_permit(game_id, game_context_token.as_deref())?;
    to_json(DesktopOptiScalerOperation::from(
        optiscaler::relocate(RelocateOptiScalerRequest {
            context,
            target_exe: target_exe.as_ref(),
            safety,
            progress,
        })
        .await?,
    ))
}

/// Safely removes recorded files, restores topology, and reports preserved drift.
pub async fn uninstall_optiscaler(context: &Context, game_id: impl Into<String>) -> JsonResult {
    let game_id = parse_game_id(game_id)?;
    to_json(DesktopOptiScalerOperation::from(
        optiscaler::uninstall(context, &game_id).await?,
    ))
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::task::{Context as TaskContext, Poll, Waker};

    use renderpilot_orchestration::{SafetyScope, ServiceError};

    use super::*;

    fn poll_ready<F: Future>(future: F) -> F::Output {
        let mut task_context = TaskContext::from_waker(Waker::noop());
        let mut future = Box::pin(future);
        match Future::poll(future.as_mut(), &mut task_context) {
            Poll::Ready(output) => output,
            Poll::Pending => panic!("missing safety context reached an async prepare boundary"),
        }
    }

    fn assert_missing_game_context(error: &crate::ApiError) {
        assert!(matches!(
            error,
            crate::ApiError::Service(ServiceError::SafetyContextMissing {
                scope: SafetyScope::Game(_)
            })
        ));
    }

    fn availability_with_launch(
        launcher: renderpilot_orchestration::domain::Launcher,
        launch: Option<optiscaler::types::OptiScalerLaunch>,
    ) -> optiscaler::types::OptiScalerAvailability {
        optiscaler::types::OptiScalerAvailability {
            game_id: "steam:1".to_owned(),
            launcher,
            available: true,
            blocked_reason: Some("internal blocking diagnostic".to_owned()),
            compatibility_block_code: None,
            detected_apis: Vec::new(),
            compatibility: optiscaler::types::OptiScalerCompatibility {
                status: optiscaler::types::OptiScalerCompatibilityStatus::Working,
                declared_inputs: Vec::new(),
                launch,
                guidance: Vec::new(),
            },
            prerequisite: optiscaler::types::OptiScalerPrerequisiteState::None,
            selected_release: Some("v1".to_owned()),
            target_exe: Some("C:/Game/Game.exe".to_owned()),
            relocation_target_exe: None,
            relocation_blocked_reason: None,
            relocation_block_code: None,
            target_dir: Some("C:/Game".to_owned()),
            proxy: optiscaler::types::OptiScalerProxyPlan {
                slot: "dxgi.dll".to_owned(),
                chain_reshade: false,
                downstream_path: None,
                conflict: None,
            },
            modules: Vec::new(),
            install_state: None,
            drifted_paths: vec!["C:/Game/OptiScaler.ini".to_owned()],
            update_available: false,
            repair_required: false,
            unmanaged: false,
            maintenance_available: true,
            maintenance_block_code: None,
        }
    }

    #[test]
    fn optiscaler_file_mutations_require_game_context_before_async_prepare() {
        let dir = tempfile::tempdir().expect("tempdir");
        let context = Context::open_at(dir.path().join("catalog.sqlite")).expect("context");
        let game_id = "manual:missing-optiscaler-safety";

        assert_missing_game_context(
            &poll_ready(install_optiscaler(&context, game_id, None, None, None))
                .expect_err("install must require game context"),
        );
        assert_missing_game_context(
            &poll_ready(update_optiscaler(&context, game_id, None, None))
                .expect_err("update must require game context"),
        );
        assert_missing_game_context(
            &poll_ready(repair_optiscaler(&context, game_id, None, None))
                .expect_err("repair must require game context"),
        );
        assert_missing_game_context(
            &poll_ready(set_optiscaler_modules(
                &context,
                game_id,
                &["core".to_owned()],
                None,
                None,
            ))
            .expect_err("module changes must require game context"),
        );
        assert_missing_game_context(
            &poll_ready(relocate_optiscaler(
                &context, game_id, "Game.exe", None, None,
            ))
            .expect_err("relocation must require game context"),
        );
    }

    #[test]
    fn desktop_availability_contract_is_nested_and_omits_diagnostics() {
        let dto = DesktopOptiScalerAvailability {
            game_id: "steam:1".to_owned(),
            launcher: renderpilot_orchestration::domain::Launcher::Steam,
            install: DesktopOptiScalerInstall {
                installed: true,
                release: Some("v1".to_owned()),
            },
            eligibility: DesktopOptiScalerEligibility {
                available: true,
                block_code: None,
            },
            selected_release: Some("v1".to_owned()),
            relocation: None,
            proxy_conflict: None,
            compatibility: DesktopOptiScalerCompatibility {
                status: optiscaler::types::OptiScalerCompatibilityStatus::Working,
                declared_inputs: vec![optiscaler::types::OptiScalerDeclaredInput::Dlss2Plus],
                launch: None,
                guidance: Vec::new(),
            },
            prerequisite: DesktopOptiScalerPrerequisite {
                state: optiscaler::types::OptiScalerPrerequisiteState::None,
            },
            modules: Vec::new(),
            lifecycle: DesktopOptiScalerLifecycle {
                update_available: false,
                repair_required: false,
                drifted: false,
                unmanaged: false,
                maintenance_available: true,
                maintenance_block_code: None,
            },
        };

        let value = serde_json::to_value(dto).expect("serialize desktop availability");

        assert_eq!(value["install"]["release"], "v1");
        assert_eq!(
            value["compatibility"]["declared_inputs"],
            serde_json::json!(["dlss2_plus"])
        );
        for internal in [
            "target_dir",
            "target_exe",
            "drifted_paths",
            "evidence",
            "detected_technologies",
            "proxy",
        ] {
            assert!(value.get(internal).is_none(), "leaked `{internal}`");
        }
    }

    #[test]
    fn desktop_availability_projects_typed_launch_and_launcher_without_internals() {
        for (requirement, expected_requirement) in [
            (
                optiscaler::types::OptiScalerLaunchRequirement::Required,
                "required",
            ),
            (
                optiscaler::types::OptiScalerLaunchRequirement::Recommended,
                "recommended",
            ),
        ] {
            let dto = DesktopOptiScalerAvailability::from(availability_with_launch(
                renderpilot_orchestration::domain::Launcher::Epic,
                Some(optiscaler::types::OptiScalerLaunch {
                    arguments: vec!["-dx12".to_owned()],
                    requirement,
                }),
            ));
            let value = serde_json::to_value(dto).expect("serialize desktop availability");

            assert_eq!(value["launcher"], "Epic");
            assert_eq!(
                value["compatibility"]["launch"],
                serde_json::json!({
                    "arguments": ["-dx12"],
                    "requirement": expected_requirement,
                })
            );
            for internal in [
                "blocked_reason",
                "target_dir",
                "target_exe",
                "drifted_paths",
                "evidence",
                "proxy",
                "detected_apis",
            ] {
                assert!(value.get(internal).is_none(), "leaked `{internal}`");
            }
        }

        let absent = serde_json::to_value(DesktopOptiScalerAvailability::from(
            availability_with_launch(renderpilot_orchestration::domain::Launcher::Manual, None),
        ))
        .expect("serialize absent launch policy");
        assert!(absent["compatibility"]["launch"].is_null());
    }

    #[test]
    fn desktop_operation_contract_reports_counts_without_paths() {
        let dto = DesktopOptiScalerOperation {
            installed: true,
            release: Some("v1".to_owned()),
            changed_count: 2,
            preserved_count: 1,
            config_conflicts: Vec::new(),
        };

        let value = serde_json::to_value(dto).expect("serialize desktop operation");

        assert_eq!(value["changed_count"], 2);
        assert!(value.get("changed_paths").is_none());
        assert!(value.get("preserved_paths").is_none());
    }
}
