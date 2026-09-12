use super::*;

pub(super) fn load_config_inputs(
    release: &OptiScalerRelease,
    archive: &PreparedArchive,
    old_state: Option<&OptiScalerInstallState>,
    target_dir: &Path,
) -> Result<ConfigInputs, ServiceError> {
    let member = release
        .members
        .iter()
        .find(|member| member.target == "OptiScaler.ini")
        .ok_or_else(|| failed("release has no OptiScaler.ini"))?;
    let new_config = archive.bytes(&member.archive_path)?.to_vec();
    let source_path = old_state
        .map(|state| Path::new(state.target_dir.as_str()).join("OptiScaler.ini"))
        .unwrap_or_else(|| target_dir.join("OptiScaler.ini"));
    let destination_path = target_dir.join("OptiScaler.ini");
    let target_mode = if crate::paths::same_path(&source_path, &destination_path) {
        ConfigTargetMode::InPlace
    } else {
        ConfigTargetMode::RetargetToAbsent
    };
    if target_mode == ConfigTargetMode::RetargetToAbsent
        && super::super::super::maybe_exact_receipt_from_live(
            &destination_path,
            FileOwnership::Reused,
        )?
        .is_some()
    {
        return Err(failed(format!(
            "cannot retarget OptiScaler configuration to occupied path {}",
            destination_path.display()
        )));
    }
    let (current_config, current_receipt) = match super::super::super::read_exact_file_from_live(
        &source_path,
        FileOwnership::Reused,
    )? {
        Some((bytes, receipt)) => (bytes, Some(receipt)),
        None => (
            old_state
                .and_then(|state| state.configuration_baseline().bytes())
                .map_or_else(|| new_config.clone(), |bytes| bytes.to_vec()),
            None,
        ),
    };
    let current_sha256 = current_receipt
        .as_ref()
        .map(|receipt| receipt.digest().clone());
    Ok(ConfigInputs {
        config_member_archive_path: member.archive_path.clone(),
        new_config,
        current_config,
        target_mode,
        source_path,
        destination_path,
        current_sha256,
        current_receipt,
    })
}
