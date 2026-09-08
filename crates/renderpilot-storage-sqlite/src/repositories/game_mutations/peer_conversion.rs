use super::optiscaler_validation::ExpectedPeerRelocation;
use super::peer_binding::PeerTopologyDirection;
use super::*;

fn unique_path_keys(
    paths: &[PathRef],
    label: &str,
) -> AppResult<std::collections::BTreeSet<String>> {
    let mut keys = std::collections::BTreeSet::new();
    for path in paths {
        if !keys.insert(normalized_path_key(path.as_str())) {
            return Err(renderpilot_application::AppError::invalid_input(format!(
                "Luma reverse peer receipt contains a duplicate {label} path",
            )));
        }
    }
    Ok(keys)
}

fn exact_append(
    before: &[PathRef],
    after: &[PathRef],
    destination: &PathRef,
    append: bool,
    label: &str,
) -> AppResult<()> {
    if append {
        if after.len() != before.len() + 1
            || after[..before.len()] != *before
            || normalized_path_key(after.last().expect("appended path").as_str())
                != normalized_path_key(destination.as_str())
        {
            return Err(renderpilot_application::AppError::invalid_input(format!(
                "Luma reverse peer receipt must append the destination to {label} in order",
            )));
        }
    } else if after != before {
        return Err(renderpilot_application::AppError::invalid_input(format!(
            "Luma reverse peer receipt changed unrelated {label} claims",
        )));
    }
    unique_path_keys(after, label)?;
    Ok(())
}

fn ensure_no_unexpected_overlap(
    created: &std::collections::BTreeSet<String>,
    backed: &std::collections::BTreeSet<String>,
    managed: &std::collections::BTreeSet<String>,
    destination_key: &str,
    allow_destination_engine_overlap: bool,
) -> AppResult<()> {
    for (left, right, label) in [
        (created, backed, "created/backed-up"),
        (created, managed, "created/managed"),
        (backed, managed, "backed-up/managed"),
    ] {
        if left.iter().any(|key| {
            right.contains(key) && !(allow_destination_engine_overlap && key == destination_key)
        }) {
            return Err(renderpilot_application::AppError::invalid_input(format!(
                "Luma reverse peer receipt contains an unexpected {label} overlap",
            )));
        }
    }
    Ok(())
}

/// Validates the only native projection admitted when OptiScaler is removed
/// from a Luma chain. The source is one managed claim; the native destination
/// is either appended to the generic engine lists (Owned custody) or is left
/// unclaimed (Reused custody). Every unrelated claim retains its exact value
/// and order.
pub(super) fn ensure_exact_luma_out_managed_to_native(
    before: &InstalledAddon,
    after: &InstalledAddon,
    expected: &ExpectedPeerRelocation,
) -> AppResult<ExpectedPeerRelocation> {
    if expected.direction != PeerTopologyDirection::OutOfOptiTopology {
        return Err(renderpilot_application::AppError::invalid_input(
            "Luma managed-to-native peer conversion requires an OutOfOptiTopology direction",
        ));
    }
    if before.kind() != AddonKind::Luma
        || after.kind() != AddonKind::Luma
        || before.game_id() != after.game_id()
        || before.addon_file() != after.addon_file()
    {
        return Err(renderpilot_application::AppError::invalid_input(
            "Luma reverse peer conversion requires one unchanged Luma identity",
        ));
    }
    if normalized_path_key(expected.source.as_str()).is_empty()
        || normalized_path_key(expected.destination.as_str()).is_empty()
        || normalized_path_key(expected.source.as_str())
            == normalized_path_key(expected.destination.as_str())
    {
        return Err(renderpilot_application::AppError::invalid_input(
            "Luma reverse peer conversion requires distinct source and destination paths",
        ));
    }
    let source_key = normalized_path_key(expected.source.as_str());
    let destination_key = normalized_path_key(expected.destination.as_str());
    let identity_claim_keys = before
        .registered_exe_path()
        .into_iter()
        .chain(after.registered_exe_path())
        .map(|path| normalized_path_key(path.as_str()))
        .chain([
            normalized_path_key(before.addon_file().as_str()),
            normalized_path_key(after.addon_file().as_str()),
        ])
        .collect::<std::collections::BTreeSet<_>>();
    if identity_claim_keys.contains(&source_key) || identity_claim_keys.contains(&destination_key) {
        return Err(renderpilot_application::AppError::invalid_input(
            "Luma reverse peer conversion cannot overlap the add-on or executable claim",
        ));
    }
    if before.addon_version() != after.addon_version()
        || before.tracked_sources() != after.tracked_sources()
        || before.installed_at() != after.installed_at()
        || before.updated_at() != after.updated_at()
        || before.host_kind() != after.host_kind()
        || before.reshade_channel() != after.reshade_channel()
        || before.registered_exe_path() != after.registered_exe_path()
    {
        return Err(renderpilot_application::AppError::invalid_input(
            "Luma reverse peer conversion changed unrelated metadata",
        ));
    }

    let before_created = unique_path_keys(before.created_files(), "created")?;
    let before_backed = unique_path_keys(before.backed_up_files(), "backed-up")?;
    let after_created = unique_path_keys(after.created_files(), "created")?;
    let after_backed = unique_path_keys(after.backed_up_files(), "backed-up")?;
    if before_created.contains(&source_key)
        || before_backed.contains(&source_key)
        || before_created.contains(&destination_key)
        || before_backed.contains(&destination_key)
        || before
            .managed_files()
            .iter()
            .any(|file| normalized_path_key(file.path().as_str()) == destination_key)
        || after_created.contains(&source_key)
        || after_backed.contains(&source_key)
    {
        return Err(renderpilot_application::AppError::invalid_input(
            "Luma reverse peer conversion overlaps the managed source or native destination",
        ));
    }

    let before_managed = before
        .managed_files()
        .iter()
        .filter(|file| normalized_path_key(file.path().as_str()) == source_key)
        .collect::<Vec<_>>();
    let [source] = before_managed.as_slice() else {
        return Err(renderpilot_application::AppError::invalid_input(
            "Luma reverse peer conversion requires exactly one managed source claim",
        ));
    };
    let after_source_or_destination = after.managed_files().iter().any(|file| {
        let key = normalized_path_key(file.path().as_str());
        key == source_key || key == destination_key
    });
    if after_source_or_destination {
        return Err(renderpilot_application::AppError::invalid_input(
            "Luma reverse peer conversion must remove the managed source and destination claims",
        ));
    }
    let before_unrelated = before
        .managed_files()
        .iter()
        .filter(|file| normalized_path_key(file.path().as_str()) != source_key)
        .collect::<Vec<_>>();
    if !after
        .managed_files()
        .iter()
        .eq(before_unrelated.iter().copied())
    {
        return Err(renderpilot_application::AppError::invalid_input(
            "Luma reverse peer conversion changed unrelated managed claims",
        ));
    }
    let after_managed_keys = unique_path_keys(
        &after
            .managed_files()
            .iter()
            .map(|file| file.path().clone())
            .collect::<Vec<_>>(),
        "managed",
    )?;
    let managed_mode_key_set = unique_path_keys(
        &before
            .managed_files()
            .iter()
            .map(|file| file.path().clone())
            .collect::<Vec<_>>(),
        "managed",
    )?;
    ensure_no_unexpected_overlap(
        &before_created,
        &before_backed,
        &managed_mode_key_set,
        &destination_key,
        false,
    )?;
    ensure_no_unexpected_overlap(
        &after_created,
        &after_backed,
        &after_managed_keys,
        &destination_key,
        true,
    )?;

    if source.installed_sha256() != &expected.sha256 {
        return Err(renderpilot_application::AppError::invalid_input(
            "Luma reverse peer conversion changed the host digest",
        ));
    }
    let expected_mode = match expected.ownership {
        FileOwnership::Owned => ManagedFileMode::Owned,
        FileOwnership::Reused => ManagedFileMode::Reused,
    };
    if source.mode() != expected_mode {
        return Err(renderpilot_application::AppError::invalid_input(
            "Luma reverse peer conversion changed host custody",
        ));
    }
    let append_backed = match (source.mode(), source.baseline()) {
        (ManagedFileMode::Owned, ManagedFileBaseline::Absent) => false,
        (ManagedFileMode::Owned, ManagedFileBaseline::Present { .. }) => true,
        (ManagedFileMode::Reused, ManagedFileBaseline::Present { .. }) => false,
        (ManagedFileMode::Reused, ManagedFileBaseline::Absent) => {
            return Err(renderpilot_application::AppError::invalid_input(
                "Luma reverse peer conversion rejects a Reused/Absent source",
            ));
        }
    };
    let append_created = source.mode() == ManagedFileMode::Owned;
    exact_append(
        before.created_files(),
        after.created_files(),
        &expected.destination,
        append_created,
        "created",
    )?;
    exact_append(
        before.backed_up_files(),
        after.backed_up_files(),
        &expected.destination,
        append_backed,
        "backed-up",
    )?;
    if append_created && !after_created.contains(&destination_key) {
        return Err(renderpilot_application::AppError::invalid_input(
            "Luma reverse peer conversion must append the native destination claim",
        ));
    }
    if append_backed && !after_backed.contains(&destination_key) {
        return Err(renderpilot_application::AppError::invalid_input(
            "Luma reverse peer conversion must append the native baseline claim",
        ));
    }
    if !append_backed && after_backed.contains(&destination_key) {
        return Err(renderpilot_application::AppError::invalid_input(
            "Luma reverse peer conversion added an unexpected native baseline claim",
        ));
    }
    Ok(expected.clone())
}

pub(super) fn exact_created_path_map(
    paths: &[PathRef],
) -> AppResult<std::collections::BTreeMap<String, &PathRef>> {
    let mut mapped = std::collections::BTreeMap::new();
    for path in paths {
        if mapped
            .insert(normalized_path_key(path.as_str()), path)
            .is_some()
        {
            return Err(renderpilot_application::AppError::invalid_input(
                "OptiScaler peer receipt contains duplicate normalized created-file paths",
            ));
        }
    }
    Ok(mapped)
}

/// Validates the one legacy-to-canonical handoff permitted at the active
/// proxy aggregate boundary. The generic source is removed from the peer
/// engine and the exact live bytes are adopted as one Owned/Absent managed
/// claim at the topology destination.
pub(super) fn ensure_exact_created_host_to_managed_downstream(
    before: &InstalledAddon,
    after: &InstalledAddon,
    expected: &ExpectedPeerRelocation,
) -> AppResult<ExpectedPeerRelocation> {
    if !matches!(before.kind(), AddonKind::RenoDx | AddonKind::Luma)
        || before.kind() != after.kind()
    {
        return Err(renderpilot_application::AppError::invalid_input(
            "OptiScaler peer host conversion requires one unchanged RenoDX or Luma identity",
        ));
    }
    let source_key = normalized_path_key(expected.source.as_str());
    let destination_key = normalized_path_key(expected.destination.as_str());
    if source_key.is_empty() || destination_key.is_empty() || source_key == destination_key {
        return Err(renderpilot_application::AppError::invalid_input(
            "OptiScaler peer host conversion requires distinct source and destination paths",
        ));
    }
    if expected.ownership != FileOwnership::Owned {
        return Err(renderpilot_application::AppError::invalid_input(
            "OptiScaler peer host conversion requires Owned topology custody",
        ));
    }
    let before_addon_key = normalized_path_key(before.addon_file().as_str());
    let after_addon_key = normalized_path_key(after.addon_file().as_str());
    if before_addon_key == source_key
        || after_addon_key == source_key
        || before_addon_key == destination_key
        || after_addon_key == destination_key
        || before.registered_exe_path().is_some_and(|path| {
            let key = normalized_path_key(path.as_str());
            key == source_key || key == destination_key
        })
        || after.registered_exe_path().is_some_and(|path| {
            let key = normalized_path_key(path.as_str());
            key == source_key || key == destination_key
        })
    {
        return Err(renderpilot_application::AppError::invalid_input(
            "OptiScaler peer host conversion cannot overlap the add-on or executable claim",
        ));
    }

    let before_created = exact_created_path_map(before.created_files())?;
    let after_created = exact_created_path_map(after.created_files())?;
    let mut removed_created = before_created
        .iter()
        .filter(|(key, _)| !after_created.contains_key(*key))
        .map(|(_, path)| *path);
    let (Some(removed), None) = (removed_created.next(), removed_created.next()) else {
        return Err(renderpilot_application::AppError::invalid_input(
            "OptiScaler peer host conversion requires exactly one removed generic source",
        ));
    };
    if normalized_path_key(removed.as_str()) != source_key
        || before_created.contains_key(&destination_key)
        || after_created.contains_key(&source_key)
        || after_created.contains_key(&destination_key)
    {
        return Err(renderpilot_application::AppError::invalid_input(
            "OptiScaler peer host conversion changed an unrelated generic claim",
        ));
    }
    for (key, before_path) in &before_created {
        if let Some(after_path) = after_created.get(key)
            && *after_path != *before_path
        {
            return Err(renderpilot_application::AppError::invalid_input(
                "OptiScaler peer host conversion changed an unrelated created-file receipt",
            ));
        }
    }

    let before_managed = before
        .managed_files()
        .iter()
        .map(|file| (normalized_path_key(file.path().as_str()), file))
        .collect::<std::collections::BTreeMap<_, _>>();
    let after_managed = after
        .managed_files()
        .iter()
        .map(|file| (normalized_path_key(file.path().as_str()), file))
        .collect::<std::collections::BTreeMap<_, _>>();
    if before_managed.len() != before.managed_files().len()
        || after_managed.len() != after.managed_files().len()
        || before_managed.contains_key(&source_key)
        || before_managed.contains_key(&destination_key)
        || after_managed.contains_key(&source_key)
    {
        return Err(renderpilot_application::AppError::invalid_input(
            "OptiScaler peer host conversion overlaps an existing managed claim",
        ));
    }
    let mut added_managed = after_managed
        .iter()
        .filter(|(key, _)| !before_managed.contains_key(*key))
        .map(|(_, file)| *file);
    let (Some(added), None) = (added_managed.next(), added_managed.next()) else {
        return Err(renderpilot_application::AppError::invalid_input(
            "OptiScaler peer host conversion requires exactly one added managed destination",
        ));
    };
    if normalized_path_key(added.path().as_str()) != destination_key
        || added.mode() != ManagedFileMode::Owned
        || added.baseline() != &ManagedFileBaseline::Absent
        || added.installed_sha256() != &expected.sha256
    {
        return Err(renderpilot_application::AppError::invalid_input(
            "OptiScaler peer host conversion requires an exact Owned/Absent destination claim",
        ));
    }
    for (key, before_file) in &before_managed {
        if let Some(after_file) = after_managed.get(key)
            && *after_file != *before_file
        {
            return Err(renderpilot_application::AppError::invalid_input(
                "OptiScaler peer host conversion changed an unrelated managed-file receipt",
            ));
        }
    }

    let before_backups = before
        .backed_up_files()
        .iter()
        .map(|path| normalized_path_key(path.as_str()))
        .collect::<std::collections::BTreeSet<_>>();
    let after_backups = after
        .backed_up_files()
        .iter()
        .map(|path| normalized_path_key(path.as_str()))
        .collect::<std::collections::BTreeSet<_>>();
    if before_backups.len() != before.backed_up_files().len()
        || after_backups.len() != after.backed_up_files().len()
        || before_backups.contains(&source_key)
        || before_backups.contains(&destination_key)
        || after_backups.contains(&source_key)
        || after_backups.contains(&destination_key)
        || before_backups != after_backups
    {
        return Err(renderpilot_application::AppError::invalid_input(
            "OptiScaler peer host conversion changed or overlapped a backup claim",
        ));
    }
    Ok(expected.clone())
}
