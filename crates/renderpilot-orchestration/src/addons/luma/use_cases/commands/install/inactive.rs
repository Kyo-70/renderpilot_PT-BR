//! Established Luma install lifecycle for games without a proxy topology.
//!
//! This route intentionally retains the ordinary scan, torn recovery, and
//! engine transaction semantics. It is not used when an active OptiScaler
//! topology exists.

mod plan;

use std::path::Path;

use renderpilot_domain::{AddonKind, InstalledAddon};

use crate::ServiceError;
use crate::addons::luma::fetch::prepare::prepare_install;
use crate::addons::luma::install::install as install_files;
use crate::addons::progress::emit_tool_finalizing;

pub(super) async fn install(
    request: super::InstallRequest<'_>,
) -> Result<InstalledAddon, ServiceError> {
    let super::InstallRequest {
        context,
        manifest,
        reshade_sources,
        game_id,
        safety,
        progress,
    } = request;

    let (snapshot, plan, dgvoodoo_kind) = {
        let _guard =
            crate::mutation_boundary::enter_game_mutation_boundary_async(context, game_id).await?;
        let resolved = plan::resolve(context, manifest, game_id)?;
        (resolved.snapshot, resolved.plan, resolved.dgvoodoo_kind)
    };
    let dgvoodoo = plan::preparation_for_plan(&plan, &snapshot.target_dir, dgvoodoo_kind)?;
    let mut prepared = prepare_install(
        &plan,
        reshade_sources,
        game_id.clone(),
        snapshot.writes_host,
        dgvoodoo,
        progress,
    )
    .await?;

    let guard =
        crate::mutation_boundary::enter_game_mutation_boundary_async(context, game_id).await?;
    let revalidated = plan::resolve(context, manifest, game_id)?;
    plan::ensure_matches(&snapshot, &revalidated.snapshot)?;
    plan::refresh_adopted(
        &mut prepared,
        &revalidated.snapshot.target_dir,
        &revalidated.plan,
    )?;

    emit_tool_finalizing(progress, AddonKind::Luma);
    let min_version = manifest.min_reshade_version_parsed()?;
    let targets = crate::addons::luma::mutation_targets::install_targets(
        &revalidated.snapshot.target_dir,
        &prepared,
        &min_version,
    )?;
    crate::FileSafetyAuthority::new().authorize_game_commit(
        context,
        crate::addons::mutation_features::LUMA_INSTALL,
        &guard,
        &safety,
        || {
            crate::addons::durable::run_install_mutation(
                context,
                &guard,
                targets,
                crate::addons::mutation_features::LUMA_INSTALL,
                game_id,
                || {
                    let source_last_modified = prepared.source_last_modified.clone();
                    let (record, commit) = install_files(
                        context,
                        &revalidated.snapshot.target_dir,
                        prepared,
                        &min_version,
                    )?;
                    crate::fs::stamp_mtime_best_effort(
                        Path::new(record.addon_file().as_str()),
                        source_last_modified.as_deref(),
                        None,
                    );
                    Ok((record, commit))
                },
            )
        },
    )
}
