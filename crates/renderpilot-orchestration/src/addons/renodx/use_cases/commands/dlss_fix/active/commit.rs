//! Active-topology RenoDX DLSS-Fix lifecycle.
//!
//! This module is the only command route for a persisted proxy topology. It
//! seals all aggregate, root, and endpoint facts before a download, then uses
//! the typed peer composer and ordinary durable peer transaction for the commit.

use std::path::Path;

use renderpilot_domain::{
    GameId, InstalledAddon, RenoDxInstallState, RenoDxReshadeIniFeature, TrackedSource,
    TrackedSourceRole,
};

use super::snapshot::ActiveSnapshot;
use crate::addons::renodx::dlss_fix::DlssFixRequest;
use crate::addons::renodx::dlss_fix_binding::DlssFixBindingState;
use crate::addons::renodx::peer::{
    ActiveDlssComposition, ActiveDlssEffect, ActiveDlssEndpointInput, ActiveDlssInput,
    compose_active_dlss,
};
use crate::addons::renodx::{errors, source, tracking};
use crate::addons::reshade::ini_schema::ini_merge_strategy;
use crate::addons::reshade::types::{DlssFixIniTweaks, ReshadeIniTweaks};
use crate::{Context, ServiceError};

pub(super) fn reconcile_partial(snapshot: &ActiveSnapshot) -> Result<InstalledAddon, ServiceError> {
    let source = match snapshot.binding.state {
        DlssFixBindingState::SourceOnly => snapshot.binding.source.clone(),
        DlssFixBindingState::OwnedOnly => Some(advisory_source_from_snapshot(snapshot)?),
        _ => {
            return Err(errors::invalid(
                "DLSS-Fix projection is not partial".to_owned(),
            ));
        }
    };
    tracking::rebuild_with_dlss_projection(
        &snapshot.record,
        Some(Path::new(snapshot.companion_path.as_str())),
        source,
        "DLSS-Fix active partial reconciliation",
    )
}

pub(super) fn reconcile_effect(
    snapshot: &ActiveSnapshot,
) -> Result<ActiveDlssEffect, ServiceError> {
    if snapshot.binding.state == DlssFixBindingState::SourceOnly {
        let bytes = snapshot.companion.bytes().ok_or_else(|| {
            errors::invalid("DLSS-Fix partial reconciliation lost its regular companion".to_owned())
        })?;
        Ok(ActiveDlssEffect::Write(bytes.to_vec()))
    } else {
        Ok(ActiveDlssEffect::Unchanged)
    }
}

fn advisory_source_from_snapshot(snapshot: &ActiveSnapshot) -> Result<TrackedSource, ServiceError> {
    let arch = snapshot.binding.arch.ok_or_else(invalid_binding)?;
    let digest = snapshot
        .companion
        .file()
        .ok_or_else(|| {
            errors::invalid("DLSS-Fix companion is not a readable regular file".to_owned())
        })?
        .digest()
        .clone();
    Ok(TrackedSource::new(
        TrackedSourceRole::DlssFix,
        source::dlss_fix_url(arch),
        None,
        digest.as_str().to_owned(),
    )
    .with_advisory())
}

pub(super) fn source_from_download(
    arch: renderpilot_domain::Architecture,
    download: &crate::addons::reshade::fetch::Download,
) -> TrackedSource {
    TrackedSource::new(
        TrackedSourceRole::DlssFix,
        source::dlss_fix_url(arch),
        download.etag.clone(),
        download.digest.clone(),
    )
    .with_last_modified(download.last_modified.clone())
}

pub(super) fn install_ini_effect<'a>(
    snapshot: &'a ActiveSnapshot,
    request: &DlssFixRequest,
) -> Result<Option<ActiveDlssEndpointInput<'a>>, ServiceError> {
    let name = snapshot
        .companion_path
        .file_name()
        .map(str::to_owned)
        .ok_or_else(invalid_binding)?;
    let strategy = ini_merge_strategy(&ReshadeIniTweaks {
        disabled_addons: Vec::new(),
        addon_path: None,
        dlss_fix: Some(DlssFixIniTweaks {
            addon_file_name: name,
            dlss_path: request.dlss_path.clone(),
            streamline_path: request.streamline_path.clone(),
        }),
    });
    transformed_ini(snapshot, &strategy)
}

pub(super) fn remove_ini_effect(
    snapshot: &ActiveSnapshot,
) -> Result<Option<ActiveDlssEndpointInput<'_>>, ServiceError> {
    let strategy = crate::addons::renodx::reshade_ini::ini_remove_dlss_fix_strategy();
    transformed_ini(snapshot, &strategy)
}

fn transformed_ini<'a>(
    snapshot: &'a ActiveSnapshot,
    strategy: &crate::addons::engine::MergeStrategy,
) -> Result<Option<ActiveDlssEndpointInput<'a>>, ServiceError> {
    let base = match snapshot.ini.bytes() {
        Some(bytes) => std::str::from_utf8(bytes).map_err(|_| {
            errors::invalid(format!(
                "active ReShade.ini is not UTF-8: {}",
                snapshot.ini_path.as_str()
            ))
        })?,
        None => "",
    };
    let next = strategy.apply(base);
    if next == base {
        return Ok(None);
    }
    Ok(Some(ActiveDlssEndpointInput::new(
        snapshot.ini_path.clone(),
        &snapshot.ini,
        ActiveDlssEffect::Write(next.into_bytes()),
    )))
}

pub(super) struct ActiveDlssCommit<'a> {
    pub(super) context: &'a Context,
    pub(super) guard: &'a crate::game_mutation_lock::GameMutationGuard,
    pub(super) game_id: &'a GameId,
    pub(super) snapshot: &'a ActiveSnapshot,
    pub(super) after: InstalledAddon,
    pub(super) companion_effect: ActiveDlssEffect,
    pub(super) ini: Option<ActiveDlssEndpointInput<'a>>,
    pub(super) feature: &'static str,
}

pub(super) fn commit_claim_or_physical(
    ActiveDlssCommit {
        context,
        guard,
        game_id,
        snapshot,
        after,
        companion_effect,
        ini,
        feature,
    }: ActiveDlssCommit<'_>,
) -> Result<RenoDxInstallState, ServiceError> {
    let ini_feature = ini.as_ref().map(|_| {
        RenoDxReshadeIniFeature::try_from_feature(feature)
            .expect("DLSS-Fix feature constants are typed")
    });
    let input = ActiveDlssInput {
        before_peer: &snapshot.record,
        after_peer: &after,
        topology: &snapshot.topology,
        root: &snapshot.root,
        companion: ActiveDlssEndpointInput::new(
            snapshot.companion_path.clone(),
            &snapshot.companion,
            companion_effect,
        ),
        ini,
        ini_feature,
    };
    let composition = compose_active_dlss(&input).map_err(map_active_error)?;

    let (after_peer, projection, program, payloads, authority) = match composition {
        ActiveDlssComposition::Noop => return Ok(tracking::install_state_from_record(&after)),
        ActiveDlssComposition::ClaimOnly {
            after_peer,
            projection,
            ..
        } => (after_peer, projection, None, Vec::new(), None),
        ActiveDlssComposition::Physical {
            after_peer,
            projection,
            program,
            payloads,
            reshade_ini_authority,
            ..
        } => (
            after_peer,
            projection,
            Some(program),
            payloads,
            reshade_ini_authority,
        ),
    };
    let planned_topology =
        renderpilot_domain::PlannedGameProxyTopology::Exact(snapshot.topology.clone());
    let package =
        crate::addons::peer_lifecycle::package::PeerMutationPackage::plan_active_with_renodx_dlss(
            crate::addons::peer_lifecycle::package::RenoDxDlssMutationRequest {
                before_peer: &snapshot.record,
                after_peer: &after_peer,
                before_topology: &snapshot.topology,
                planned_after_topology: &planned_topology,
                program,
                payloads,
                game_root: snapshot.root.canonical_game_root().to_path_buf(),
                payload_root: snapshot.root.payload_root().map(Path::to_path_buf),
                renodx_reshade_ini: authority,
                projection,
            },
        )?;
    let prepared = context
        .peer_mutation_executor()
        .prepare_ordinary_file_peer_with_renodx_dlss(
            context,
            guard,
            feature,
            Some(game_id.as_str()),
            package,
        )?;
    let mut changes = crate::addons::engine::InstallChanges::default();
    let applied = prepared.apply(&mut changes)?;
    changes.sync_touched_dirs();
    applied.commit()?;
    Ok(tracking::install_state_from_record(&after_peer))
}

fn map_active_error(error: crate::addons::renodx::peer::ActiveDlssError) -> ServiceError {
    match error {
        crate::addons::renodx::peer::ActiveDlssError::Invalid(reason) => {
            errors::invalid(reason.to_owned())
        }
        crate::addons::renodx::peer::ActiveDlssError::Path(path) => errors::invalid(format!(
            "invalid active RenoDX DLSS-Fix path: {}",
            path.as_str()
        )),
        crate::addons::renodx::peer::ActiveDlssError::Domain(error) => {
            errors::invalid(error.to_string())
        }
        crate::addons::renodx::peer::ActiveDlssError::Program(error) => {
            errors::invalid(error.to_string())
        }
    }
}

pub(super) fn invalid_binding() -> ServiceError {
    errors::invalid(
        "DLSS-Fix record or disk binding requires validation before targeted mutation".to_owned(),
    )
}
