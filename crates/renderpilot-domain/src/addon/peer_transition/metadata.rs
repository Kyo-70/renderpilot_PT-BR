//! Metadata-only peer transitions.

use crate::{AddonKind, GameProxyTopology, InstalledAddon, TrackedSource, TrackedSourceRole};

use super::claims::{PeerSnapshot, validate_snapshots};
use super::model::PeerTransitionError;

/// Validates a metadata-only update without allowing any physical claim or
/// topology receipt to change. Upstream provenance fields may refresh, but
/// tracked-source membership, order, and physical-authority digests remain
/// closed.
pub fn validate_peer_metadata_only(
    before_peer: Option<&InstalledAddon>,
    after_peer: Option<&InstalledAddon>,
    before_topology: Option<&GameProxyTopology>,
    after_topology: Option<&GameProxyTopology>,
) -> Result<(), PeerTransitionError> {
    validate_snapshots(before_peer, after_peer, before_topology, after_topology)?;
    PeerSnapshot::from_peer(before_peer)?;
    PeerSnapshot::from_peer(after_peer)?;

    match (before_peer, after_peer) {
        (Some(before), Some(after)) => {
            validate_peer_metadata_fields(before, after, true)?;
        }
        (None, None) => {}
        _ => {
            return Err(PeerTransitionError::InvalidPeerSnapshot(
                "metadata-only transition changed peer presence",
            ));
        }
    }
    if before_topology != after_topology {
        return Err(PeerTransitionError::InvalidTopologySnapshot(
            "metadata-only transition changed topology".to_owned(),
        ));
    }
    Ok(())
}

/// Validates the metadata/physical fields that may remain unchanged while a
/// typed managed-file transition changes only its managed vector. This is the
/// same refresh policy used by [`validate_peer_metadata_only`]; keeping it in
/// one helper prevents a new transition from silently accepting a broader
/// provenance update.
pub(super) fn validate_peer_metadata_refresh(
    before: &InstalledAddon,
    after: &InstalledAddon,
) -> Result<(), PeerTransitionError> {
    validate_peer_metadata_fields(before, after, false)
}

fn validate_peer_metadata_fields(
    before: &InstalledAddon,
    after: &InstalledAddon,
    require_managed_match: bool,
) -> Result<(), PeerTransitionError> {
    if before.game_id() != after.game_id()
        || before.kind() != after.kind()
        || before.addon_file() != after.addon_file()
        || before.created_files() != after.created_files()
        || before.backed_up_files() != after.backed_up_files()
        || (require_managed_match && before.managed_files() != after.managed_files())
        || before.host_kind() != after.host_kind()
        || before.registered_exe_path() != after.registered_exe_path()
    {
        return Err(PeerTransitionError::InvalidPeerSnapshot(
            "metadata-only transition changed physical peer claims",
        ));
    }
    validate_tracked_sources(before, after)
}

fn validate_tracked_sources(
    before: &InstalledAddon,
    after: &InstalledAddon,
) -> Result<(), PeerTransitionError> {
    if before.tracked_sources().len() != after.tracked_sources().len() {
        return Err(PeerTransitionError::InvalidPeerSnapshot(
            "metadata-only transition changed tracked-source membership",
        ));
    }

    for (before_source, after_source) in
        before.tracked_sources().iter().zip(after.tracked_sources())
    {
        if before_source.role() != after_source.role()
            || (!digest_may_refresh(before.kind(), before_source, after_source)
                && before_source.digest() != after_source.digest())
        {
            return Err(PeerTransitionError::InvalidPeerSnapshot(
                "metadata-only transition changed an authoritative tracked-source digest or role",
            ));
        }
    }
    Ok(())
}

fn digest_may_refresh(kind: AddonKind, before: &TrackedSource, after: &TrackedSource) -> bool {
    match before.role() {
        TrackedSourceRole::AddonPayload | TrackedSourceRole::HostBinary => {
            kind == AddonKind::Luma && !before.is_advisory() && !after.is_advisory()
        }
        TrackedSourceRole::DgVoodooWrapper => kind == AddonKind::Luma,
        TrackedSourceRole::DlssFix => false,
    }
}
