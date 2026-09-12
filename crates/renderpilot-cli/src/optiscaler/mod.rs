use renderpilot_orchestration::addons::optiscaler;
use renderpilot_orchestration::domain::GameId;
use renderpilot_orchestration::{Context, ServiceError};

use crate::commands::{CliOutput, block_on, render_json};

pub(crate) fn render_status(context: &Context, game_id: &GameId) -> CliOutput {
    render_json(&optiscaler::status(context, game_id)?)
}

pub(crate) fn render_uninstall(context: &Context, game_id: &GameId) -> CliOutput {
    let result = block_on(optiscaler::uninstall(context, game_id))?;
    render_json(&result)
}

pub(crate) fn render_check_update(context: &Context, game_id: &GameId) -> CliOutput {
    let report = block_on(optiscaler::check_update(context, game_id))?;
    render_json(&report)
}

pub(crate) fn render_check_updates(context: &Context) -> CliOutput {
    let reports = block_on(async { reports_to_map(optiscaler::check_updates(context).await?) })?;
    render_json(&reports)
}

fn reports_to_map<T: serde::Serialize>(
    reports: Vec<(GameId, T)>,
) -> Result<serde_json::Map<String, serde_json::Value>, ServiceError> {
    reports
        .into_iter()
        .map(|(game_id, report)| {
            serde_json::to_value(report)
                .map(|value| (game_id.as_str().to_owned(), value))
                .map_err(|error| {
                    ServiceError::command_failed(format!(
                        "failed to serialize OptiScaler update: {error}"
                    ))
                })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_reports_are_keyed_by_game_id() {
        let game_id = GameId::new("game-1").expect("valid game id");
        let reports = reports_to_map(vec![(game_id, serde_json::json!({ "status": "current" }))])
            .expect("serializable report");

        assert_eq!(
            reports.get("game-1"),
            Some(&serde_json::json!({ "status": "current" }))
        );
    }
}
