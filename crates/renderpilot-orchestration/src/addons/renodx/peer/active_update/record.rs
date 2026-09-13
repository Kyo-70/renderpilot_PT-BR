use renderpilot_domain::{
    AddonKind, GameProxyTopology, InstalledAddon, ManagedAddonFile, ManagedFileMode,
    NormalizedPathRelation, PathRef, normalized_path_relation,
};

use super::error::RenoDxActiveUpdateError;
use super::paths::require_under_roots;

pub(super) fn validate_records(
    before: &InstalledAddon,
    after: &InstalledAddon,
    topology: &GameProxyTopology,
) -> Result<(), RenoDxActiveUpdateError> {
    for record in [before, after] {
        if record.kind() != AddonKind::RenoDx {
            return Err(RenoDxActiveUpdateError::InvalidRecord(
                "active update requires RenoDX records",
            ));
        }
        if record.game_id() != &topology.game_id {
            return Err(RenoDxActiveUpdateError::InvalidRecord(
                "peer record game differs from active topology",
            ));
        }
        if !record.created_files().iter().any(|path| {
            matches!(
                normalized_path_relation(path.as_str(), record.addon_file().as_str()),
                NormalizedPathRelation::Equal
            )
        }) {
            return Err(RenoDxActiveUpdateError::InvalidRecord(
                "peer record does not claim its add-on payload",
            ));
        }
    }
    Ok(())
}

pub(super) fn validate_record_paths(
    record: &InstalledAddon,
    canonical_game_root: &std::path::Path,
    payload_root: Option<&std::path::Path>,
) -> Result<(), RenoDxActiveUpdateError> {
    for path in record
        .created_files()
        .iter()
        .chain(record.backed_up_files())
        .chain(record.managed_files().iter().map(ManagedAddonFile::path))
    {
        require_under_roots(path, canonical_game_root, payload_root)?;
    }
    Ok(())
}

pub(super) fn validate_addon_claims(
    before: &InstalledAddon,
    after: &InstalledAddon,
    addon_path: &renderpilot_domain::PathRef,
) -> Result<(), RenoDxActiveUpdateError> {
    if !matches!(
        normalized_path_relation(before.addon_file().as_str(), addon_path.as_str()),
        NormalizedPathRelation::Equal
    ) || !matches!(
        normalized_path_relation(after.addon_file().as_str(), addon_path.as_str()),
        NormalizedPathRelation::Equal
    ) {
        return Err(RenoDxActiveUpdateError::InvalidPath(addon_path.clone()));
    }
    if before.managed_files().iter().any(|file| {
        matches!(
            normalized_path_relation(file.path().as_str(), addon_path.as_str()),
            NormalizedPathRelation::Equal
        )
    }) || after.managed_files().iter().any(|file| {
        matches!(
            normalized_path_relation(file.path().as_str(), addon_path.as_str()),
            NormalizedPathRelation::Equal
        )
    }) {
        return Err(RenoDxActiveUpdateError::InvalidRecord(
            "add-on payload cannot be a managed coordinated file",
        ));
    }
    Ok(())
}

pub(super) fn owned_host_claim<'a>(
    record: &'a InstalledAddon,
    path: &renderpilot_domain::PathRef,
) -> Result<&'a renderpilot_domain::ManagedAddonFile, RenoDxActiveUpdateError> {
    let claims = record
        .managed_files()
        .iter()
        .filter(|file| {
            matches!(
                normalized_path_relation(file.path().as_str(), path.as_str()),
                NormalizedPathRelation::Equal
            )
        })
        .collect::<Vec<_>>();
    let [claim] = claims.as_slice() else {
        return Err(RenoDxActiveUpdateError::InvalidRecord(
            "active host update requires one exact managed host claim",
        ));
    };
    if claim.mode() != ManagedFileMode::Owned {
        return Err(RenoDxActiveUpdateError::InvalidRecord(
            "active host update requires an owned host claim",
        ));
    }
    Ok(claim)
}

pub(super) fn ensure_no_physical_claim_changes(
    before: &InstalledAddon,
    after: &InstalledAddon,
) -> Result<(), RenoDxActiveUpdateError> {
    if before.created_files() != after.created_files()
        || before.backed_up_files() != after.backed_up_files()
        || before.managed_files() != after.managed_files()
        || before.addon_file() != after.addon_file()
    {
        return Err(RenoDxActiveUpdateError::InvalidRecord(
            "record physical claims changed without an endpoint input",
        ));
    }
    Ok(())
}

pub(super) fn ensure_non_host_claims_unchanged(
    before: &InstalledAddon,
    after: &InstalledAddon,
    host_path: &PathRef,
) -> Result<(), RenoDxActiveUpdateError> {
    if before.created_files() != after.created_files()
        || before.backed_up_files() != after.backed_up_files()
        || before.addon_file() != after.addon_file()
    {
        return Err(RenoDxActiveUpdateError::InvalidRecord(
            "record physical claims changed outside the supplied endpoints",
        ));
    }
    let before_other = before
        .managed_files()
        .iter()
        .filter(|file| {
            !matches!(
                normalized_path_relation(file.path().as_str(), host_path.as_str()),
                NormalizedPathRelation::Equal
            )
        })
        .collect::<Vec<_>>();
    let after_other = after
        .managed_files()
        .iter()
        .filter(|file| {
            !matches!(
                normalized_path_relation(file.path().as_str(), host_path.as_str()),
                NormalizedPathRelation::Equal
            )
        })
        .collect::<Vec<_>>();
    if before_other != after_other {
        return Err(RenoDxActiveUpdateError::InvalidRecord(
            "record managed claims changed outside the supplied host endpoint",
        ));
    }
    Ok(())
}
