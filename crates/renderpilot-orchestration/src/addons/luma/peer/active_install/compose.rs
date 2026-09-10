use std::path::Path;

use renderpilot_domain::{Sha256Hash, normalized_path_key};

use crate::{
    ServiceError, addons::luma::install::PreparedInstall, peer_mutation_executor::ExactEndpoint,
};

use super::{
    super::{
        active_dgvoodoo::{
            ActiveDgVoodooRecordProjection, ActiveDgVoodooSnapshot, lower_active_dgvoodoo,
            plan_active_dgvoodoo,
        },
        active_dlss::{classify_active_dlss, lower_active_dlss_owned},
        active_host::{classify_active_host, lower_active_host_owned},
        active_payload::lower_active_payload,
        effects::{LumaPeerEffectAccumulator, LumaPeerOperationOrder},
    },
    error::LumaActiveInstallCompositionError,
    model::{LumaActiveInstallComposition, LumaActiveInstallInput},
    observation::{dlss_path, effective_addon_root, observe},
    record::{RecordInput, build_record},
};

/// Composes one active-topology Luma install without storage, writes, or
/// application authority. All returned bytes are owned by the peer program.
pub(crate) fn compose_active_install(
    input: LumaActiveInstallInput<'_>,
) -> Result<LumaActiveInstallComposition, ServiceError> {
    compose_active_install_inner(input).map_err(|error| {
        ServiceError::invalid_input(format!("active Luma install rejected: {error}"))
    })
}

fn compose_active_install_inner(
    input: LumaActiveInstallInput<'_>,
) -> Result<LumaActiveInstallComposition, LumaActiveInstallCompositionError> {
    let LumaActiveInstallInput {
        prepared,
        topology,
        authority,
        assessment,
        host_path,
        host_snapshot,
        catalog_claim,
        minimum_host_version,
    } = input;
    if prepared.game_id != topology.game_id {
        return Err(LumaActiveInstallCompositionError::InvalidInput(
            "prepared install and proxy topology belong to different games",
        ));
    }
    topology.validate().map_err(|_| {
        LumaActiveInstallCompositionError::InvalidInput("proxy topology is invalid")
    })?;
    if topology.outer.implementation != renderpilot_domain::ProxyImplementation::OptiScaler {
        return Err(LumaActiveInstallCompositionError::InvalidInput(
            "proxy topology outer implementation is not OptiScaler",
        ));
    }
    for participant in topology.participant_paths() {
        authority
            .authorized_root(participant)
            .map_err(LumaActiveInstallCompositionError::Authority)?;
    }
    let topology_root_parent = Path::new(topology.root_slot.as_str())
        .parent()
        .map(|parent| normalized_path_key(&parent.to_string_lossy()));
    let authority_root = normalized_path_key(authority.canonical_game_root_ref().as_str());
    if topology_root_parent.as_deref() != Some(authority_root.as_str()) {
        return Err(LumaActiveInstallCompositionError::InvalidInput(
            "proxy topology root slot is not directly under the sealed game root",
        ));
    }
    if prepared.asset_source_url.trim().is_empty() {
        return Err(LumaActiveInstallCompositionError::InvalidInput(
            "add-on source URL is empty",
        ));
    }
    Sha256Hash::new(prepared.zip_digest.clone()).map_err(|_| {
        LumaActiveInstallCompositionError::InvalidInput("add-on ZIP digest is not valid SHA-256")
    })?;

    let PreparedInstall {
        game_id,
        proxy_dll_name: _,
        payload,
        main_addon_rel,
        asset_source_url,
        zip_digest,
        source_etag,
        source_last_modified,
        build_label,
        reshade_dll_bytes,
        reshade_source_url,
        reshade_source_etag,
        reshade_last_modified,
        reshade_digest,
        dgvoodoo,
    } = prepared;

    // Classification only consumes supplied topology evidence and prepared
    // host bytes; the exact host is not read by this composer.
    let host_classification = classify_active_host(
        authority,
        topology,
        assessment,
        host_path,
        host_snapshot,
        (!reshade_dll_bytes.is_empty()).then_some(reshade_dll_bytes),
        minimum_host_version,
    )?;
    let host_binding = host_classification.binding().clone();
    let host_owned = host_classification.owned().is_some();
    if host_owned {
        let supplied = Sha256Hash::new(reshade_digest.clone()).map_err(|_| {
            LumaActiveInstallCompositionError::InvalidInput(
                "owned ReShade host provenance digest is empty or invalid",
            )
        })?;
        if &supplied != host_binding.installed_sha256() {
            return Err(LumaActiveInstallCompositionError::InvalidInput(
                "prepared ReShade host digest does not match its installed binding",
            ));
        }
        if reshade_source_url.trim().is_empty() {
            return Err(LumaActiveInstallCompositionError::InvalidInput(
                "owned ReShade host source URL is empty",
            ));
        }
    }
    let planned_topology = host_classification.planned_topology().clone();
    let owned_host_plan = host_classification.into_owned();

    let dgvoodoo_plan = plan_active_dgvoodoo(authority, dgvoodoo)?;
    let mut accumulator = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);
    let mut payload_projection =
        lower_active_payload(authority, payload, &main_addon_rel, &mut accumulator)?;

    let dlss_target = dlss_path(authority)?;
    let dlss_live = observe(&dlss_target, effective_addon_root(authority))?;
    let dlss_bytes = payload_projection.take_dlss_bytes();
    let dlss_classification =
        classify_active_dlss(&dlss_target, dlss_bytes, catalog_claim, &dlss_live)?;
    let dlss_binding = dlss_classification.binding().cloned();
    let owned_dlss_plan = dlss_classification.into_owned();
    if let Some(plan) = owned_dlss_plan {
        let sidecar_path = plan
            .sidecar_path()
            .map_err(LumaActiveInstallCompositionError::DlssLowering)?;
        let sidecar_snapshot = observe(&sidecar_path, effective_addon_root(authority))?;
        lower_active_dlss_owned(plan, &dlss_live, &sidecar_snapshot, &mut accumulator)?;
    }

    let dgvoodoo_projection = if let Some(plan) = dgvoodoo_plan {
        let root = authority.canonical_game_root_ref();
        let images = plan
            .observation_paths()
            .iter()
            .map(|path| observe(path, root))
            .collect::<Result<Vec<_>, _>>()?;
        let snapshots = plan
            .observation_paths()
            .iter()
            .zip(&images)
            .map(|(path, image)| ActiveDgVoodooSnapshot::new(path, image))
            .collect::<Vec<_>>();
        lower_active_dgvoodoo(plan, &snapshots, &mut accumulator)?
    } else {
        ActiveDgVoodooRecordProjection::default()
    };

    if let Some(plan) = owned_host_plan {
        let sidecar_snapshot = if plan.is_create() {
            None
        } else {
            let sidecar_path = plan
                .sidecar_path()
                .map_err(LumaActiveInstallCompositionError::HostLowering)?;
            Some(observe(&sidecar_path, authority.canonical_game_root_ref())?)
        };
        lower_active_host_owned(
            plan,
            host_snapshot,
            sidecar_snapshot.as_ref(),
            &mut accumulator,
        )?;
    }

    let record = build_record(RecordInput {
        game_id,
        addon_source_url: asset_source_url,
        addon_source_etag: source_etag,
        addon_source_last_modified: source_last_modified,
        addon_zip_digest: zip_digest,
        addon_version: build_label,
        main_addon: payload_projection.main_addon().clone(),
        reshade_source_url,
        reshade_source_etag,
        reshade_source_last_modified: reshade_last_modified,
        reshade_digest,
        payload: payload_projection,
        dgvoodoo: dgvoodoo_projection,
        host: host_binding,
        dlss: dlss_binding,
        host_owned,
    })?;

    let effects = accumulator
        .finalize()
        .map_err(LumaActiveInstallCompositionError::Effects)?
        .ok_or(LumaActiveInstallCompositionError::EmptyProgram)?;
    let (program, payloads) = effects.into_parts();
    authority
        .validate(
            None,
            Some(&record),
            None,
            program.endpoints().iter().map(ExactEndpoint::path),
        )
        .map_err(LumaActiveInstallCompositionError::Authority)?;

    Ok(LumaActiveInstallComposition::new(
        record,
        program,
        payloads,
        planned_topology,
    ))
}
