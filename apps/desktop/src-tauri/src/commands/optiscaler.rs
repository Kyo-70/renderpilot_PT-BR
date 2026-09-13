//! OptiScaler desktop IPC commands.

use std::sync::Arc;

use crate::diagnostic_event::CommandOperation;
use renderpilot_api as desktop;
use renderpilot_orchestration::Context;

use super::{CommandBoundary, JsonCommandResult, download_progress_emitter, require_game_context};

#[tauri::command]
pub async fn get_optiscaler_availability(
    game_id: String,
    context: tauri::State<'_, Arc<Context>>,
) -> JsonCommandResult {
    let boundary = CommandBoundary::new(CommandOperation::OptiScalerAvailability);
    let (game_id, context) = require_game_context(&boundary, game_id, &context)?;
    boundary
        .run_async(
            move || async move { desktop::get_optiscaler_availability(&context, game_id).await },
        )
        .await
}

#[tauri::command]
pub async fn install_optiscaler(
    app: tauri::AppHandle,
    game_id: String,
    modules: Option<Vec<String>>,
    game_context_token: Option<String>,
    context: tauri::State<'_, Arc<Context>>,
) -> JsonCommandResult {
    let boundary = CommandBoundary::new(CommandOperation::OptiScalerInstall);
    let (game_id, context) = require_game_context(&boundary, game_id, &context)?;
    boundary
        .run_async(move || async move {
            let emit = download_progress_emitter(app, game_id.clone());
            desktop::install_optiscaler(
                &context,
                game_id,
                modules.as_deref(),
                game_context_token,
                Some(&emit as &desktop::ProgressObserver<'_>),
            )
            .await
        })
        .await
}

#[tauri::command]
pub async fn check_optiscaler_update(
    game_id: String,
    context: tauri::State<'_, Arc<Context>>,
) -> JsonCommandResult {
    let boundary = CommandBoundary::new(CommandOperation::OptiScalerCheckUpdate);
    let (game_id, context) = require_game_context(&boundary, game_id, &context)?;
    boundary
        .run_async(move || async move { desktop::check_optiscaler_update(&context, game_id).await })
        .await
}

#[tauri::command]
pub async fn update_optiscaler(
    app: tauri::AppHandle,
    game_id: String,
    game_context_token: Option<String>,
    context: tauri::State<'_, Arc<Context>>,
) -> JsonCommandResult {
    let boundary = CommandBoundary::new(CommandOperation::OptiScalerUpdate);
    let (game_id, context) = require_game_context(&boundary, game_id, &context)?;
    boundary
        .run_async(move || async move {
            let emit = download_progress_emitter(app, game_id.clone());
            desktop::update_optiscaler(
                &context,
                game_id,
                game_context_token,
                Some(&emit as &desktop::ProgressObserver<'_>),
            )
            .await
        })
        .await
}

#[tauri::command]
pub async fn repair_optiscaler(
    app: tauri::AppHandle,
    game_id: String,
    game_context_token: Option<String>,
    context: tauri::State<'_, Arc<Context>>,
) -> JsonCommandResult {
    let boundary = CommandBoundary::new(CommandOperation::OptiScalerRepair);
    let (game_id, context) = require_game_context(&boundary, game_id, &context)?;
    boundary
        .run_async(move || async move {
            let emit = download_progress_emitter(app, game_id.clone());
            desktop::repair_optiscaler(
                &context,
                game_id,
                game_context_token,
                Some(&emit as &desktop::ProgressObserver<'_>),
            )
            .await
        })
        .await
}

#[tauri::command]
pub async fn set_optiscaler_modules(
    app: tauri::AppHandle,
    game_id: String,
    modules: Vec<String>,
    game_context_token: Option<String>,
    context: tauri::State<'_, Arc<Context>>,
) -> JsonCommandResult {
    let boundary = CommandBoundary::new(CommandOperation::OptiScalerSetModules);
    let (game_id, context) = require_game_context(&boundary, game_id, &context)?;
    boundary
        .run_async(move || async move {
            let emit = download_progress_emitter(app, game_id.clone());
            desktop::set_optiscaler_modules(
                &context,
                game_id,
                &modules,
                game_context_token,
                Some(&emit as &desktop::ProgressObserver<'_>),
            )
            .await
        })
        .await
}

#[tauri::command]
pub async fn relocate_optiscaler(
    app: tauri::AppHandle,
    game_id: String,
    target_exe: String,
    game_context_token: Option<String>,
    context: tauri::State<'_, Arc<Context>>,
) -> JsonCommandResult {
    let boundary = CommandBoundary::new(CommandOperation::OptiScalerRelocate);
    let (game_id, context) = require_game_context(&boundary, game_id, &context)?;
    boundary
        .run_async(move || async move {
            let emit = download_progress_emitter(app, game_id.clone());
            desktop::relocate_optiscaler(
                &context,
                game_id,
                target_exe,
                game_context_token,
                Some(&emit as &desktop::ProgressObserver<'_>),
            )
            .await
        })
        .await
}

#[tauri::command]
pub async fn uninstall_optiscaler(
    game_id: String,
    context: tauri::State<'_, Arc<Context>>,
) -> JsonCommandResult {
    let boundary = CommandBoundary::new(CommandOperation::OptiScalerUninstall);
    let (game_id, context) = require_game_context(&boundary, game_id, &context)?;
    boundary
        .run_async(move || async move { desktop::uninstall_optiscaler(&context, game_id).await })
        .await
}
