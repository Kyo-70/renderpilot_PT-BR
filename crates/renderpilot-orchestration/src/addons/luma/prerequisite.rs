//! Minimal OptiScaler-facing Luma prerequisite state.
//!
//! This is intentionally not Luma availability: it uses only existing durable
//! records, the install sentinel, payload readability, and the already-derived
//! Luma capability row. It never scans unmanaged files or re-runs a Luma
//! matcher.

use renderpilot_domain::{AddonKind, GameId};

use crate::addons::capabilities::DurableProfileCapabilities;
use crate::addons::game_analysis::{analyze_game, install_target_dir};
use crate::addons::game_context::{executable_override, require_game};
use crate::addons::records;
use crate::{Context, ServiceError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LumaPrerequisite {
    Satisfied,
    RemoveRenoDx,
    LumaTorn,
    LumaBroken,
    InstallLuma,
    LumaUnavailable,
}

pub(crate) fn availability(
    context: &Context,
    game_id: &GameId,
) -> Result<LumaPrerequisite, ServiceError> {
    if records::record_of_kind(context, game_id, AddonKind::RenoDx)?.is_some() {
        return Ok(LumaPrerequisite::RemoveRenoDx);
    }
    let game = require_game(context, game_id)?;
    let override_path = executable_override(context, game_id);
    let analysis = analyze_game(&game, override_path.as_deref());
    if analysis
        .primary_executable
        .as_ref()
        .and_then(|_| install_target_dir(&analysis).ok())
        .is_some_and(|root| crate::addons::engine::is_install_torn(&root, AddonKind::Luma))
    {
        return Ok(LumaPrerequisite::LumaTorn);
    }
    if let Some(record) = records::record_of_kind(context, game_id, AddonKind::Luma)? {
        return Ok(if super::tracking::payload_disk_intact(&record) {
            LumaPrerequisite::Satisfied
        } else {
            LumaPrerequisite::LumaBroken
        });
    }
    if DurableProfileCapabilities::load_for_game(context, game_id)?.contains(&AddonKind::Luma) {
        Ok(LumaPrerequisite::InstallLuma)
    } else {
        Ok(LumaPrerequisite::LumaUnavailable)
    }
}
