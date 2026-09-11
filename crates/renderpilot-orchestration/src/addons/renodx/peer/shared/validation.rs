use std::path::{Path, PathBuf};

use renderpilot_domain::{
    AddonKind, InstalledAddon, InstalledAddonHostKind, Sha256Hash, SharedArtifactKind,
};
use renderpilot_platform_windows::vulkan_layer::{
    FileObservation, LAYER_DLL_NAME, LayerPlanOperation,
};

use super::{ActiveSharedMutationError, ActiveSharedMutationValidation};
use crate::addons::renodx::peer::SharedLayerSource;
use crate::addons::reshade::fetch::sha256_hex;
use crate::addons::shared_vulkan_mutation::FileIntent;

const MANIFEST_FILE_NAME: &str = "ReShade64.json";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Operation {
    Install,
    Update,
    ChannelSwitch,
}

pub(super) fn validate_inputs(
    input: &ActiveSharedMutationValidation<'_>,
) -> Result<(), ActiveSharedMutationError> {
    let ActiveSharedMutationValidation {
        feature,
        game_id,
        game_root,
        topology,
        before_record,
        after_record,
        game_intents,
        shared_plan,
        reshade_ini_authority,
        layer_dir,
        source,
        shared_record,
    } = input;

    if feature.trim().is_empty() {
        return Err(ActiveSharedMutationError::InvalidInput(
            "mutation feature is empty",
        ));
    }
    let operation = match *feature {
        renderpilot_domain::mutation_features::RENODX_INSTALL
        | renderpilot_domain::mutation_features::RENODX_INSTALL_FROM_FILE => Operation::Install,
        renderpilot_domain::mutation_features::RENODX_UPDATE => Operation::Update,
        renderpilot_domain::mutation_features::RENODX_SWITCH_RESHADE_CHANNEL => {
            Operation::ChannelSwitch
        }
        _ => {
            return Err(ActiveSharedMutationError::InvalidInput(
                "unsupported RenoDX shared mutation feature",
            ));
        }
    };

    match operation {
        Operation::Install => {
            if before_record.is_some() {
                return Err(ActiveSharedMutationError::InvalidInput(
                    "shared install cannot carry a before record",
                ));
            }
            if shared_record.is_some() {
                return Err(ActiveSharedMutationError::InvalidInput(
                    "shared install cannot carry a prepared update record",
                ));
            }
            if shared_plan.is_noop() {
                return Err(ActiveSharedMutationError::InvalidInput(
                    "shared-layer plan is a no-op; use the ordinary game route",
                ));
            }
            if !matches!(
                shared_plan.operation,
                LayerPlanOperation::InstallAndRegister | LayerPlanOperation::RegisterApp
            ) {
                return Err(ActiveSharedMutationError::InvalidInput(
                    "shared install plan is not an active install/register plan",
                ));
            }
            validate_install_source(shared_plan.operation, *source, shared_plan, layer_dir)?;
            if let Some(authority) = reshade_ini_authority {
                if !crate::paths::same_path(
                    Path::new(authority.canonical_game_root().as_str()),
                    game_root,
                ) {
                    return Err(ActiveSharedMutationError::InvalidInput(
                        "ReShade.ini authority belongs to another game root",
                    ));
                }
                if authority.feature().as_feature() != *feature {
                    return Err(ActiveSharedMutationError::InvalidInput(
                        "ReShade.ini authority feature differs from the transaction feature",
                    ));
                }
            }
        }
        Operation::Update => {
            if before_record.is_none() {
                return Err(ActiveSharedMutationError::InvalidInput(
                    "shared update is missing its exact before record",
                ));
            }
            if source.is_some() {
                return Err(ActiveSharedMutationError::InvalidInput(
                    "shared update cannot carry layer download metadata",
                ));
            }
            if reshade_ini_authority.is_some() {
                return Err(ActiveSharedMutationError::InvalidInput(
                    "shared update cannot carry ReShade.ini authority",
                ));
            }
            let shared_record = shared_record.ok_or(ActiveSharedMutationError::InvalidInput(
                "shared update is missing its prepared shared-artifact record",
            ))?;
            if shared_plan.is_noop() {
                return Err(ActiveSharedMutationError::InvalidInput(
                    "shared-layer plan is a no-op; use the ordinary game route",
                ));
            }
            if shared_plan.operation != LayerPlanOperation::Refresh {
                return Err(ActiveSharedMutationError::InvalidInput(
                    "shared update plan is not a refresh plan",
                ));
            }
            validate_shared_update_record(shared_record, shared_plan, layer_dir)?;
        }
        Operation::ChannelSwitch => {
            if before_record.is_none() {
                return Err(ActiveSharedMutationError::InvalidInput(
                    "shared channel switch is missing its exact before record",
                ));
            }
            if source.is_some() {
                return Err(ActiveSharedMutationError::InvalidInput(
                    "shared channel switch cannot carry layer download metadata",
                ));
            }
            if reshade_ini_authority.is_some() {
                return Err(ActiveSharedMutationError::InvalidInput(
                    "shared channel switch cannot carry ReShade.ini authority",
                ));
            }
            let shared_record = shared_record.ok_or(ActiveSharedMutationError::InvalidInput(
                "shared channel switch is missing its prepared shared-artifact record",
            ))?;
            if shared_plan.is_noop() {
                return Err(ActiveSharedMutationError::InvalidInput(
                    "shared-layer plan is a no-op; use the ordinary game route",
                ));
            }
            if shared_plan.operation != LayerPlanOperation::Refresh {
                return Err(ActiveSharedMutationError::InvalidInput(
                    "shared channel switch plan is not a refresh plan",
                ));
            }
            validate_shared_update_record(shared_record, shared_plan, layer_dir)?;
        }
    }

    validate_record(after_record, game_id, game_root)?;
    if let Some(before) = before_record {
        validate_record(before, game_id, game_root)?;
        if before.host_kind() != after_record.host_kind() {
            return Err(ActiveSharedMutationError::InvalidInput(
                "shared update changes the registered host kind",
            ));
        }
        if before.registered_exe_path() != after_record.registered_exe_path() {
            return Err(ActiveSharedMutationError::InvalidInput(
                "shared update changes the registered executable",
            ));
        }
        if !crate::paths::same_path(
            Path::new(before.addon_file().as_str()),
            Path::new(after_record.addon_file().as_str()),
        ) {
            return Err(ActiveSharedMutationError::InvalidInput(
                "shared update relocates the addon payload",
            ));
        }
    }

    topology.validate().map_err(|_| {
        ActiveSharedMutationError::InvalidInput("shared mutation topology is invalid")
    })?;
    if topology.game_id != **game_id {
        return Err(ActiveSharedMutationError::InvalidInput(
            "shared mutation topology belongs to another game",
        ));
    }
    if topology
        .participant_paths()
        .any(|path| !crate::paths::is_within(Path::new(path.as_str()), game_root))
    {
        return Err(ActiveSharedMutationError::InvalidInput(
            "shared mutation topology escapes the game root",
        ));
    }

    validate_game_intents(game_intents, game_root, operation)?;
    if !crate::paths::same_path(&shared_plan.directory.path, layer_dir) {
        return Err(ActiveSharedMutationError::InvalidPath(
            shared_plan.directory.path.clone(),
        ));
    }
    Ok(())
}

fn validate_record(
    record: &InstalledAddon,
    game_id: &renderpilot_domain::GameId,
    game_root: &Path,
) -> Result<(), ActiveSharedMutationError> {
    if record.game_id() != game_id {
        return Err(ActiveSharedMutationError::InvalidInput(
            "shared mutation record belongs to another game",
        ));
    }
    if record.kind() != AddonKind::RenoDx {
        return Err(ActiveSharedMutationError::InvalidInput(
            "shared mutation record is not a RenoDX record",
        ));
    }
    if record.host_kind() != Some(InstalledAddonHostKind::SharedVulkanLayer) {
        return Err(ActiveSharedMutationError::InvalidInput(
            "shared mutation record is not marked as a Vulkan install",
        ));
    }
    if !record.created_files().iter().any(|path| {
        crate::paths::same_path(
            Path::new(path.as_str()),
            Path::new(record.addon_file().as_str()),
        )
    }) {
        return Err(ActiveSharedMutationError::InvalidInput(
            "shared mutation record is missing its addon payload claim",
        ));
    }
    if !crate::paths::is_within(Path::new(record.addon_file().as_str()), game_root) {
        return Err(ActiveSharedMutationError::InvalidPath(PathBuf::from(
            record.addon_file().as_str(),
        )));
    }
    if let Some(path) = record
        .created_files()
        .iter()
        .find(|path| !crate::paths::is_within(Path::new(path.as_str()), game_root))
    {
        return Err(ActiveSharedMutationError::InvalidPath(PathBuf::from(
            path.as_str(),
        )));
    }
    if !record.backed_up_files().is_empty() {
        return Err(ActiveSharedMutationError::InvalidInput(
            "shared mutation record carries unexpected generic backup ownership",
        ));
    }
    if !record.managed_files().is_empty() {
        return Err(ActiveSharedMutationError::InvalidInput(
            "shared mutation record claims a proxy managed host",
        ));
    }
    let registered_executable =
        record
            .registered_exe_path()
            .ok_or(ActiveSharedMutationError::InvalidInput(
                "shared mutation record is missing its registered executable",
            ))?;
    if !crate::paths::is_within(Path::new(registered_executable.as_str()), game_root) {
        return Err(ActiveSharedMutationError::InvalidPath(PathBuf::from(
            registered_executable.as_str(),
        )));
    }
    Ok(())
}

fn validate_install_source(
    operation: LayerPlanOperation,
    source: Option<SharedLayerSource<'_>>,
    plan: &renderpilot_platform_windows::vulkan_layer::SharedVulkanLayerPlan,
    layer_dir: &Path,
) -> Result<(), ActiveSharedMutationError> {
    match (operation, source) {
        (LayerPlanOperation::InstallAndRegister, None) => {
            return Err(ActiveSharedMutationError::InvalidInput(
                "layer installation is missing its prepared source metadata",
            ));
        }
        (LayerPlanOperation::RegisterApp, Some(_)) => {
            return Err(ActiveSharedMutationError::InvalidInput(
                "app registration cannot carry layer download metadata",
            ));
        }
        _ => {}
    }
    if let (LayerPlanOperation::InstallAndRegister, Some((_, download))) = (operation, source) {
        let expected = Sha256Hash::new(download.digest.clone()).map_err(|_| {
            ActiveSharedMutationError::InvalidInput("shared download digest is not SHA-256")
        })?;
        if expected.as_str() != sha256_hex(&download.bytes) {
            return Err(ActiveSharedMutationError::InvalidInput(
                "shared download digest does not match its bytes",
            ));
        }
        let dll_path = layer_dir.join(LAYER_DLL_NAME);
        let has_dll = plan.files.iter().any(|file| {
            crate::paths::same_path(&file.path, &dll_path)
                && matches!(
                    &file.after,
                    FileObservation::Present(bytes) if bytes == &download.bytes
                )
        });
        if !has_dll {
            return Err(ActiveSharedMutationError::InvalidInput(
                "shared install plan does not publish the prepared ReShade DLL",
            ));
        }
    }
    Ok(())
}

fn validate_shared_update_record(
    record: &renderpilot_domain::SharedArtifactRecord,
    plan: &renderpilot_platform_windows::vulkan_layer::SharedVulkanLayerPlan,
    layer_dir: &Path,
) -> Result<(), ActiveSharedMutationError> {
    if record.kind() != SharedArtifactKind::RenoDxVulkanLayer {
        return Err(ActiveSharedMutationError::InvalidInput(
            "shared update record is not a RenoDX Vulkan artifact",
        ));
    }
    let expected_dir = layer_dir.to_path_buf();
    let expected_manifest = layer_dir.join(MANIFEST_FILE_NAME);
    let expected_dll = layer_dir.join(LAYER_DLL_NAME);
    if !crate::paths::same_path(Path::new(record.install_dir().as_str()), &expected_dir)
        || !crate::paths::same_path(
            Path::new(record.manifest_path().as_str()),
            &expected_manifest,
        )
        || !crate::paths::same_path(Path::new(record.dll_path().as_str()), &expected_dll)
    {
        return Err(ActiveSharedMutationError::InvalidPath(expected_dir));
    }
    if !record
        .created_files()
        .iter()
        .any(|path| crate::paths::same_path(Path::new(path.as_str()), &expected_dll))
    {
        return Err(ActiveSharedMutationError::InvalidInput(
            "shared update record is missing its DLL provenance claim",
        ));
    }
    if !record
        .created_files()
        .iter()
        .any(|path| crate::paths::same_path(Path::new(path.as_str()), &expected_manifest))
    {
        return Err(ActiveSharedMutationError::InvalidInput(
            "shared update record is missing its manifest provenance claim",
        ));
    }
    if let Some(path) = record
        .created_files()
        .iter()
        .find(|path| !crate::paths::is_within(Path::new(path.as_str()), layer_dir))
    {
        return Err(ActiveSharedMutationError::InvalidPath(PathBuf::from(
            path.as_str(),
        )));
    }
    let digest = record
        .source_digest()
        .ok_or(ActiveSharedMutationError::InvalidInput(
            "shared update record is missing its source digest",
        ))?;
    let expected_digest = Sha256Hash::new(digest).map_err(|_| {
        ActiveSharedMutationError::InvalidInput("shared update record digest is not SHA-256")
    })?;
    let after = plan
        .files
        .iter()
        .find(|file| crate::paths::same_path(&file.path, &expected_dll))
        .and_then(|file| match &file.after {
            FileObservation::Present(bytes) => Some(bytes),
            FileObservation::Absent => None,
        })
        .ok_or(ActiveSharedMutationError::InvalidInput(
            "shared update plan does not publish the prepared ReShade DLL",
        ))?;
    if expected_digest.as_str() != sha256_hex(after) {
        return Err(ActiveSharedMutationError::InvalidInput(
            "shared update record digest does not match its DLL postimage",
        ));
    }
    Ok(())
}

fn validate_game_intents(
    intents: &[FileIntent],
    game_root: &Path,
    operation: Operation,
) -> Result<(), ActiveSharedMutationError> {
    if operation == Operation::Install && intents.is_empty() {
        return Err(ActiveSharedMutationError::InvalidInput(
            "shared install has no game file intents",
        ));
    }
    for (index, intent) in intents.iter().enumerate() {
        if intent.before == intent.after {
            return Err(ActiveSharedMutationError::InvalidInput(
                "game file intent is a no-op",
            ));
        }
        if !crate::paths::is_within(&intent.live_path, game_root) {
            return Err(ActiveSharedMutationError::InvalidPath(
                intent.live_path.clone(),
            ));
        }
        if intents[..index].iter().any(|other| {
            crate::paths::same_path(&other.live_path, &intent.live_path)
                || crate::paths::is_within(&other.live_path, &intent.live_path)
                || crate::paths::is_within(&intent.live_path, &other.live_path)
        }) {
            return Err(ActiveSharedMutationError::InvalidPath(
                intent.live_path.clone(),
            ));
        }
    }
    Ok(())
}
