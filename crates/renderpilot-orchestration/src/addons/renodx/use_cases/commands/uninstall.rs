//! Uninstalls RenoDX from a game and cleans shared Vulkan app registration.

mod active;
mod inactive;
mod route;
mod shared;

use renderpilot_domain::GameId;

use crate::{Context, ServiceError};

/// Uninstalls RenoDX through the active topology route or the preserved
/// inactive/orphan route selected by the persisted state.
pub fn uninstall(context: &Context, game_id: &GameId) -> Result<(), ServiceError> {
    route::uninstall(context, game_id)
}

pub(crate) fn uninstall_shared_locked(
    context: &Context,
    guards: &crate::mutation_boundary::GameSharedMutationGuards,
    game_id: &GameId,
    record: &renderpilot_domain::InstalledAddon,
) -> Result<(), ServiceError> {
    route::uninstall_shared_locked(context, guards, game_id, record)
}

pub(crate) fn uninstall_locked(
    context: &Context,
    guard: &crate::game_mutation_lock::GameMutationGuard,
    game_id: &GameId,
) -> Result<(), ServiceError> {
    route::uninstall_locked(context, guard, game_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::addons::records;
    use renderpilot_application::InstalledAddonRepository;
    use renderpilot_domain::{AddonKind, InstalledAddon, InstalledAddonHostKind, PathRef};
    use tempfile::tempdir;

    #[test]
    fn uninstall_reports_not_installed_for_a_luma_record_and_leaves_it_untouched() {
        let db_dir = tempdir().expect("db dir");
        let context = Context::open_at(db_dir.path().join("catalog.sqlite")).expect("context");
        let game_id = GameId::new("steam:1091500").expect("game id");
        let luma_record = InstalledAddon::new(
            game_id.clone(),
            AddonKind::Luma,
            PathRef::new(r"C:\Games\Test\Luma-Test.addon").expect("path"),
        );
        context
            .storage()
            .upsert_installed_addon(&luma_record)
            .expect("seed luma record");

        let error = uninstall(&context, &game_id).expect_err("renodx uninstall must be refused");
        assert!(matches!(error, ServiceError::InvalidInput(_)));

        let still_present = records::foreign_record(&context, &game_id, AddonKind::RenoDx)
            .expect("get")
            .expect("the luma record must survive untouched");
        assert_eq!(still_present.kind(), AddonKind::Luma);
    }

    #[test]
    fn uninstall_clears_metadata_when_recorded_paths_are_unreachable() {
        let db_dir = tempdir().expect("db dir");
        let context = Context::open_at(db_dir.path().join("catalog.sqlite")).expect("context");
        let game_id = GameId::new("manual:Z:/renderpilot-missing/RenoGame").expect("game id");
        let record = InstalledAddon::new(
            game_id.clone(),
            AddonKind::RenoDx,
            PathRef::new("Z:/renderpilot-missing/RenoGame/renodx-renogame.addon64").expect("path"),
        )
        .with_addon_version("snapshot-2026.06");
        context
            .storage()
            .upsert_installed_addon(&record)
            .expect("seed record");

        uninstall(&context, &game_id).expect("orphan uninstall must clear metadata");
        assert!(
            context
                .storage()
                .get_installed_addon(&game_id)
                .expect("query")
                .is_none(),
            "unreachable install path must still clear the install record"
        );
    }

    #[test]
    fn uninstall_clears_record_for_reachable_temp_install() {
        let db_dir = tempdir().expect("db dir");
        let game_dir = tempdir().expect("game dir");
        let context = Context::open_at(db_dir.path().join("catalog.sqlite")).expect("context");
        let addon_path = game_dir.path().join("renodx-temp.addon64");
        std::fs::write(&addon_path, b"addon").expect("write addon");
        let game_id = GameId::new(format!(
            "manual:{}",
            game_dir.path().to_string_lossy().replace('\\', "/")
        ))
        .expect("game id");
        let record = InstalledAddon::new(
            game_id.clone(),
            AddonKind::RenoDx,
            PathRef::new(addon_path.to_string_lossy().as_ref()).expect("path"),
        );
        context
            .storage()
            .upsert_installed_addon(&record)
            .expect("seed record");

        uninstall(&context, &game_id).expect("reachable uninstall");
        assert!(!addon_path.exists(), "addon file should be removed");
        assert!(
            context
                .storage()
                .get_installed_addon(&game_id)
                .expect("query")
                .is_none()
        );
    }

    #[test]
    fn game_only_entry_rejects_shared_vulkan_before_mutating_files_or_catalog() {
        let db_dir = tempdir().expect("db dir");
        let game_dir = tempdir().expect("game dir");
        let context = Context::open_at(db_dir.path().join("catalog.sqlite")).expect("context");
        let addon_path = game_dir.path().join("renodx-vulkan.addon64");
        std::fs::write(&addon_path, b"addon").expect("write addon");
        let game_id = GameId::new(format!(
            "manual:{}",
            game_dir.path().to_string_lossy().replace('\\', "/")
        ))
        .expect("game id");
        let record = InstalledAddon::new(
            game_id.clone(),
            AddonKind::RenoDx,
            PathRef::new(addon_path.to_string_lossy().as_ref()).expect("addon path"),
        )
        .with_host_kind(InstalledAddonHostKind::SharedVulkanLayer)
        .with_registered_exe_path(
            PathRef::new(game_dir.path().join("game.exe").to_string_lossy().as_ref())
                .expect("executable path"),
        );
        context
            .storage()
            .upsert_installed_addon(&record)
            .expect("seed record");
        let guard = crate::game_mutation_lock::blocking_lock(&game_id);

        let error = uninstall_locked(&context, &guard, &game_id)
            .expect_err("game-only entry must reject a shared mutation");

        assert!(matches!(error, ServiceError::InvalidInput(_)));
        assert_eq!(std::fs::read(&addon_path).expect("addon remains"), b"addon");
        assert!(
            context
                .storage()
                .get_installed_addon(&game_id)
                .expect("query")
                .is_some()
        );
    }
}
