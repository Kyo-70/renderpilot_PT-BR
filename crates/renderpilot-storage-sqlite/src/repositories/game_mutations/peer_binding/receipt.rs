use super::super::peer_conversion::{
    ensure_exact_created_host_to_managed_downstream, ensure_exact_luma_out_managed_to_native,
    exact_created_path_map,
};
use super::super::*;

pub(in crate::repositories::game_mutations) fn validate_peer_receipt_transition(
    expected: Option<&ExpectedPeerRelocation>,
    peer: OptiScalerPeerMutation<'_>,
    peer_claims_source: bool,
) -> AppResult<Option<ExpectedPeerRelocation>> {
    match (expected, peer) {
        (None, OptiScalerPeerMutation::Keep) => Ok(None),
        (Some(_), OptiScalerPeerMutation::Keep) if !peer_claims_source => Ok(None),
        (Some(_), OptiScalerPeerMutation::Keep) => {
            Err(renderpilot_application::AppError::invalid_input(
                "OptiScaler topology requires a peer relocation receipt",
            ))
        }
        (None, OptiScalerPeerMutation::Replace { .. }) => {
            Err(renderpilot_application::AppError::invalid_input(
                "OptiScaler peer mutation is not backed by a topology relocation",
            ))
        }
        (Some(expected), OptiScalerPeerMutation::Replace { before, after }) => {
            if !matches!(before.kind(), AddonKind::RenoDx | AddonKind::Luma)
                || before.kind() != after.kind()
            {
                return Err(renderpilot_application::AppError::invalid_input(
                    "OptiScaler peer replacement must preserve one RenoDX or Luma identity",
                ));
            }
            ensure_exact_peer_relocation(before, after, expected)
        }
    }
}

pub(in crate::repositories::game_mutations) fn peer_receipt_claims_source(
    peer: &InstalledAddon,
    expected: &ExpectedPeerRelocation,
) -> bool {
    if !matches!(peer.kind(), AddonKind::RenoDx | AddonKind::Luma) {
        return false;
    }
    let source_key = normalized_path_key(expected.source.as_str());
    peer.created_files()
        .iter()
        .any(|path| normalized_path_key(path.as_str()) == source_key)
        || peer
            .backed_up_files()
            .iter()
            .any(|path| normalized_path_key(path.as_str()) == source_key)
        || peer
            .managed_files()
            .iter()
            .any(|file| normalized_path_key(file.path().as_str()) == source_key)
        || normalized_path_key(peer.addon_file().as_str()) == source_key
        || peer
            .registered_exe_path()
            .is_some_and(|path| normalized_path_key(path.as_str()) == source_key)
}

pub(in crate::repositories::game_mutations) fn ensure_exact_peer_relocation(
    before: &InstalledAddon,
    after: &InstalledAddon,
    expected: &ExpectedPeerRelocation,
) -> AppResult<Option<ExpectedPeerRelocation>> {
    if before.kind() == AddonKind::Luma
        && after.kind() == AddonKind::Luma
        && expected.direction == super::PeerTopologyDirection::OutOfOptiTopology
    {
        return ensure_exact_luma_out_managed_to_native(before, after, expected).map(Some);
    }
    let luma_native_projection = before.kind() == AddonKind::Luma
        && after.kind() == AddonKind::Luma
        && before.managed_files().iter().any(|file| {
            normalized_path_key(file.path().as_str())
                == normalized_path_key(expected.source.as_str())
        })
        && !after.managed_files().iter().any(|file| {
            let key = normalized_path_key(file.path().as_str());
            key == normalized_path_key(expected.source.as_str())
                || key == normalized_path_key(expected.destination.as_str())
        });
    if luma_native_projection {
        return Err(renderpilot_application::AppError::invalid_input(
            "Luma managed-to-native peer conversion requires an OutOfOptiTopology direction",
        ));
    }

    let scalar_identity_unchanged = before.addon_file() == after.addon_file()
        && before.addon_version() == after.addon_version()
        && before.backed_up_files() == after.backed_up_files()
        && before.tracked_sources() == after.tracked_sources()
        && before.installed_at() == after.installed_at()
        && before.updated_at() == after.updated_at()
        && before.host_kind() == after.host_kind()
        && before.reshade_channel() == after.reshade_channel()
        && before.registered_exe_path() == after.registered_exe_path();
    if !scalar_identity_unchanged
        || before.created_files().len() < after.created_files().len()
        || after.managed_files().len() < before.managed_files().len()
    {
        return Err(renderpilot_application::AppError::invalid_input(
            "OptiScaler peer replacement may change only one coordinated host path",
        ));
    }

    if before.created_files().len() == after.created_files().len() + 1
        && after.managed_files().len() == before.managed_files().len() + 1
    {
        return ensure_exact_created_host_to_managed_downstream(before, after, expected).map(Some);
    }
    if before.created_files().len() != after.created_files().len()
        || before.managed_files().len() != after.managed_files().len()
    {
        return Err(renderpilot_application::AppError::invalid_input(
            "OptiScaler peer replacement may change only one coordinated host path",
        ));
    }

    match (
        before.created_files() == after.created_files(),
        before.managed_files() == after.managed_files(),
    ) {
        (true, false) => ensure_exact_managed_peer_relocation(before, after, expected).map(Some),
        (false, true) => ensure_exact_created_peer_relocation(before, after, expected).map(Some),
        _ => Err(renderpilot_application::AppError::invalid_input(
            "OptiScaler peer replacement requires exactly one created-file or managed-file host relocation",
        )),
    }
}

pub(in crate::repositories::game_mutations) fn ensure_exact_managed_peer_relocation(
    before: &InstalledAddon,
    after: &InstalledAddon,
    expected: &ExpectedPeerRelocation,
) -> AppResult<ExpectedPeerRelocation> {
    let before_files = before
        .managed_files()
        .iter()
        .map(|file| (normalized_path_key(file.path().as_str()), file))
        .collect::<BTreeMap<_, _>>();
    let after_files = after
        .managed_files()
        .iter()
        .map(|file| (normalized_path_key(file.path().as_str()), file))
        .collect::<BTreeMap<_, _>>();
    for (key, before_file) in &before_files {
        if let Some(after_file) = after_files.get(key)
            && *after_file != *before_file
        {
            return Err(renderpilot_application::AppError::invalid_input(
                "OptiScaler peer replacement changed an unrelated managed-file receipt",
            ));
        }
    }
    let removed = before_files
        .iter()
        .filter(|(key, _)| !after_files.contains_key(*key))
        .map(|(_, file)| *file)
        .collect::<Vec<_>>();
    let added = after_files
        .iter()
        .filter(|(key, _)| !before_files.contains_key(*key))
        .map(|(_, file)| *file)
        .collect::<Vec<_>>();
    let [removed] = removed.as_slice() else {
        return Err(renderpilot_application::AppError::invalid_input(
            "OptiScaler peer replacement requires exactly one source host",
        ));
    };
    let [added] = added.as_slice() else {
        return Err(renderpilot_application::AppError::invalid_input(
            "OptiScaler peer replacement requires exactly one destination host",
        ));
    };
    if normalized_path_key(removed.path().as_str()) != normalized_path_key(expected.source.as_str())
        || normalized_path_key(added.path().as_str())
            != normalized_path_key(expected.destination.as_str())
    {
        return Err(renderpilot_application::AppError::invalid_input(
            "OptiScaler managed peer relocation does not match topology source and destination",
        ));
    }
    let expected_mode = match expected.ownership {
        FileOwnership::Owned => ManagedFileMode::Owned,
        FileOwnership::Reused => ManagedFileMode::Reused,
    };
    if removed.mode() != added.mode()
        || removed.mode() != expected_mode
        || removed.baseline() != added.baseline()
        || removed.installed_sha256() != added.installed_sha256()
        || removed.installed_sha256() != &expected.sha256
    {
        return Err(renderpilot_application::AppError::invalid_input(
            "OptiScaler peer replacement changed host custody or identity",
        ));
    }
    let before_sidecar = peer_owned_sidecar_path_map(Some(before));
    let after_sidecar = peer_owned_sidecar_path_map(Some(after));
    let source_sidecar = format!("{}.bak", expected.source.as_str());
    let destination_sidecar = format!("{}.bak", expected.destination.as_str());
    match (
        before_sidecar.get(&normalized_path_key(&source_sidecar)),
        after_sidecar.get(&normalized_path_key(&destination_sidecar)),
    ) {
        (Some((_, before_hash)), Some((_, after_hash))) if before_hash == after_hash => {}
        (None, None) => {}
        _ => {
            return Err(renderpilot_application::AppError::invalid_input(
                "OptiScaler managed peer relocation changed sidecar custody",
            ));
        }
    }
    Ok(expected.clone())
}

pub(in crate::repositories::game_mutations) fn ensure_exact_created_peer_relocation(
    before: &InstalledAddon,
    after: &InstalledAddon,
    expected: &ExpectedPeerRelocation,
) -> AppResult<ExpectedPeerRelocation> {
    let before_paths = exact_created_path_map(before.created_files())?;
    let after_paths = exact_created_path_map(after.created_files())?;
    for (key, before_path) in &before_paths {
        if let Some(after_path) = after_paths.get(key)
            && *after_path != *before_path
        {
            return Err(renderpilot_application::AppError::invalid_input(
                "OptiScaler peer replacement changed an unrelated created-file receipt",
            ));
        }
    }
    let removed = before_paths
        .iter()
        .filter(|(key, _)| !after_paths.contains_key(*key))
        .map(|(_, path)| *path)
        .collect::<Vec<_>>();
    let added = after_paths
        .iter()
        .filter(|(key, _)| !before_paths.contains_key(*key))
        .map(|(_, path)| *path)
        .collect::<Vec<_>>();
    let [source] = removed.as_slice() else {
        return Err(renderpilot_application::AppError::invalid_input(
            "OptiScaler created-file peer replacement requires exactly one source host",
        ));
    };
    let [destination] = added.as_slice() else {
        return Err(renderpilot_application::AppError::invalid_input(
            "OptiScaler created-file peer replacement requires exactly one destination host",
        ));
    };
    if normalized_path_key(source.as_str()) != normalized_path_key(expected.source.as_str())
        || normalized_path_key(destination.as_str())
            != normalized_path_key(expected.destination.as_str())
    {
        return Err(renderpilot_application::AppError::invalid_input(
            "OptiScaler created-file peer relocation does not match topology source and destination",
        ));
    }
    Ok(expected.clone())
}
