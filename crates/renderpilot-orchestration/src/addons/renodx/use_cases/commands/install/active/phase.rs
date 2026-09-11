use std::path::PathBuf;

use renderpilot_application::ProxyTopologyRepository;
use renderpilot_domain::{AddonKind, GameProxyTopology, PathRef, managed_sidecar_path};

use crate::ServiceError;
use crate::addons::exclusivity;
use crate::addons::records;
use crate::addons::renodx::errors;
use crate::addons::renodx::matcher::ResolvedInstall;
use crate::addons::renodx::peer::{InstallActiveSnapshot, RenoDxRootAuthority};
use crate::addons::renodx::source;
use crate::addons::renodx::types::renodx_ini_defaults;
use crate::addons::reshade::host_policy::{
    HostLifecycle, assess_topology_downstream_from_snapshot,
};
use crate::addons::reshade::proxy::HostKind;
use crate::peer_mutation_executor::observe_peer_path_snapshot;

use super::fingerprint::{plan_fingerprint, request_fingerprint};
use super::model::{ActiveInstallResolution, ResolveActiveInstallRequest};
use super::plan;
use super::validation;

struct ProxyHostSnapshot {
    host_path: Option<PathRef>,
    host_preimage: Option<crate::peer_mutation_executor::PeerPathSnapshot>,
    host_assessment: Option<crate::addons::reshade::host_policy::TopologyHostAssessment>,
    acquisition_sidecar_preimage: Option<crate::peer_mutation_executor::PeerPathSnapshot>,
    writes_host: bool,
}

/// Resolves and seals phase-one active-install evidence.
pub(crate) fn resolve_phase1(
    request: &ResolveActiveInstallRequest<'_>,
) -> Result<ActiveInstallResolution, ServiceError> {
    let topology = read_selected_topology(request)?;
    resolve_snapshot(request, topology)
}

/// Re-resolves phase-three evidence and proves every fact is unchanged.
pub(crate) fn resolve_phase3(
    request: &ResolveActiveInstallRequest<'_>,
    phase1: &ActiveInstallResolution,
) -> Result<ActiveInstallResolution, ServiceError> {
    let topology = read_selected_topology(request)?;
    let current = resolve_snapshot(request, topology)?;
    phase1
        .snapshot()
        .ensure_phase3_matches(current.snapshot())?;
    Ok(current)
}

fn read_selected_topology(
    request: &ResolveActiveInstallRequest<'_>,
) -> Result<GameProxyTopology, ServiceError> {
    let current = request
        .context
        .storage()
        .get_proxy_topology(request.game_id)?
        .ok_or_else(errors::state_changed_retry_install)?;
    if current != request.selected_topology {
        return Err(errors::state_changed_retry_install());
    }
    Ok(current)
}

fn resolve_snapshot(
    request: &ResolveActiveInstallRequest<'_>,
    topology: GameProxyTopology,
) -> Result<ActiveInstallResolution, ServiceError> {
    records::ensure_no_record(
        request.context,
        request.game_id,
        AddonKind::RenoDx,
        "RenoDX is already installed for this game; uninstall before reinstalling",
    )?;

    let resolved = plan::resolve(request)?;
    let target_dir = plan::target_dir(&resolved.analysis)?;
    let registered_exe_path = resolved
        .analysis
        .primary_executable
        .as_ref()
        .map(|executable| PathBuf::from(executable.as_str()));
    if matches!(resolved.plan.host_kind, HostKind::Vulkan) && registered_exe_path.is_none() {
        return Err(errors::invalid(
            "active Vulkan RenoDX installs require a registered executable".to_owned(),
        ));
    }

    let ini_defaults = renodx_ini_defaults();
    let authority = RenoDxRootAuthority::resolve(
        &target_dir,
        resolved.plan.host_kind,
        ini_defaults.addon_path.as_deref(),
        registered_exe_path.as_deref(),
    )?;
    let canonical_target_dir = authority.canonical_game_root().to_path_buf();
    validation::validate_topology(request.game_id, &canonical_target_dir, &topology)?;

    let sealed_roots = authority
        .roots()
        .roots()
        .iter()
        .map(PathBuf::as_path)
        .collect::<Vec<_>>();
    exclusivity::ensure_not_blocked(
        request.context,
        request.game_id,
        AddonKind::RenoDx,
        Some(&sealed_roots),
    )?;
    if crate::addons::engine::is_install_torn(&canonical_target_dir, AddonKind::RenoDx) {
        return Err(errors::invalid(
            "an incomplete earlier RenoDX install is present; recover it before retrying"
                .to_owned(),
        ));
    }

    let payload_path = canonical_payload_path(&authority, &resolved.plan)?;
    let payload_root = authority.authorized_root(&payload_path)?.clone();
    let payload_preimage = observe_peer_path_snapshot(&payload_path, &payload_root)?;

    let ProxyHostSnapshot {
        host_path,
        host_preimage,
        host_assessment,
        acquisition_sidecar_preimage,
        writes_host,
    } = match resolved.plan.host_kind {
        HostKind::Proxy => resolve_proxy_host(&authority)?,
        HostKind::Vulkan => ProxyHostSnapshot {
            host_path: None,
            host_preimage: None,
            host_assessment: None,
            acquisition_sidecar_preimage: None,
            writes_host: false,
        },
    };

    let canonical_target_dir_ref = authority.canonical_game_root_ref().clone();
    let registered_executable = authority
        .seal()
        .canonical_registered_exe()
        .map(|path| authority.path_ref(path, "registered executable"))
        .transpose()?;
    let request_fingerprint =
        request_fingerprint(request.game_id, request.source, request.requested_channel)?;
    let plan_fingerprint = plan_fingerprint(&resolved.plan)?;
    let snapshot = InstallActiveSnapshot {
        variant: resolved.variant,
        feature: resolved.feature,
        request_fingerprint,
        plan_fingerprint,
        requested_channel: request.requested_channel,
        canonical_target_dir,
        canonical_target_dir_ref,
        topology: Some(topology),
        root_seal: authority.seal().clone(),
        payload_path,
        payload_preimage,
        host_path,
        host_preimage,
        host_assessment,
        acquisition_sidecar_preimage,
        registered_executable,
        writes_host,
        content: authority.content(),
    };
    Ok(ActiveInstallResolution::new(resolved.plan, snapshot))
}

fn canonical_payload_path(
    authority: &RenoDxRootAuthority,
    plan: &ResolvedInstall,
) -> Result<PathRef, ServiceError> {
    let file_name = source::addon_file_name(&plan.slug, plan.arch);
    let path = authority.effective_addon_root().join(file_name);
    let path = authority.path_ref(&path, "RenoDX add-on path")?;
    authority.authorized_root(&path)?;
    Ok(path)
}

fn resolve_proxy_host(authority: &RenoDxRootAuthority) -> Result<ProxyHostSnapshot, ServiceError> {
    let host_path = authority
        .exact_proxy_host()
        .ok_or_else(|| errors::failed("RenoDX proxy authority has no exact ReShade64.dll path"))?;
    let host_path = authority.path_ref(host_path, "exact ReShade64.dll host path")?;
    let host_root = authority.authorized_root(&host_path)?.clone();
    let host_preimage = observe_peer_path_snapshot(&host_path, &host_root)?;
    let assessment = assess_topology_downstream_from_snapshot(
        authority.canonical_game_root(),
        &host_path,
        &host_preimage,
        authority.content(),
        "RenoDX",
        None,
    )?;
    assessment
        .assessment()
        .ensure_initial_installable("ReShade64.dll")?;
    let writes_host = assessment.requires_host_download();
    let sidecar = if assessment.snapshot().lifecycle == HostLifecycle::RepairEmpty {
        let sidecar = managed_sidecar_path(&host_path).map_err(|error| {
            errors::failed(format!("invalid RenoDX host sidecar path: {error}"))
        })?;
        let sidecar_root = authority.authorized_root(&sidecar)?.clone();
        Some(observe_peer_path_snapshot(&sidecar, &sidecar_root)?)
    } else {
        None
    };
    Ok(ProxyHostSnapshot {
        host_path: Some(host_path),
        host_preimage: Some(host_preimage),
        host_assessment: Some(assessment),
        acquisition_sidecar_preimage: sidecar,
        writes_host,
    })
}
