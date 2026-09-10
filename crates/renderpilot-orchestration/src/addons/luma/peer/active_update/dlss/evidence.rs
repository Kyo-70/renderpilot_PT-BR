use renderpilot_domain::{ManagedAddonFile, ManagedFileBaseline, PathRef};

use crate::addons::luma::peer::active_update::{
    error::LumaActiveUpdateError, model::LumaActiveUpdateDlssInput,
};
use crate::addons::luma::peer::root_authority::LumaPeerRootAuthority;
use crate::coordinated_files::CatalogPathClaim;
use crate::peer_mutation_executor::{PeerPathSnapshot, observe_peer_path_snapshot};

use super::model::{BundledDlss, InputKind, LiveDlss, LiveImage, LiveOwnedImage};

pub(super) fn prepare_bundled(
    input: LumaActiveUpdateDlssInput,
) -> Result<(InputKind, Option<BundledDlss>), LumaActiveUpdateError> {
    match input {
        LumaActiveUpdateDlssInput::Preserve => Ok((InputKind::Preserve, None)),
        LumaActiveUpdateDlssInput::Full { bundled_bytes } => Ok((
            InputKind::Full,
            bundled_bytes
                .map(|bytes| parse_bundled(&bytes))
                .transpose()?,
        )),
    }
}

pub(super) fn parse_bundled(bytes: &[u8]) -> Result<BundledDlss, LumaActiveUpdateError> {
    let info = renderpilot_detection::DlssBinaryInfo::from_bytes(bytes)
        .map_err(|error| detail(format!("bundled nvngx_dlss.dll is invalid: {error}")))?;
    let digest = renderpilot_detection::sha256_bytes(bytes)
        .map_err(|error| detail(format!("bundled nvngx_dlss.dll cannot be hashed: {error}")))?;
    Ok(BundledDlss {
        bytes: bytes.to_vec(),
        digest,
        version: info.version().clone(),
    })
}

pub(super) fn inspect_live<'a>(
    target: &PathRef,
    snapshot: &'a PeerPathSnapshot,
) -> Result<Option<LiveDlss<'a>>, LumaActiveUpdateError> {
    let Some(image) = inspect_live_image(target, snapshot)? else {
        return Ok(None);
    };
    let info = renderpilot_detection::DlssBinaryInfo::from_bytes(image.bytes)
        .map_err(|error| detail(format!("live nvngx_dlss.dll is invalid: {error}")))?;
    Ok(Some(LiveDlss {
        image,
        version: info.version().clone(),
    }))
}

pub(super) fn inspect_live_image<'a>(
    target: &PathRef,
    snapshot: &'a PeerPathSnapshot,
) -> Result<Option<LiveImage<'a>>, LumaActiveUpdateError> {
    let Some(file) = snapshot.file() else {
        return Ok(None);
    };
    let bytes = snapshot
        .bytes()
        .ok_or_else(|| invalid("retained DLSS snapshot has no bytes"))?;
    crate::addons::luma::peer::effects::ensure_bytes_match_image(target, bytes, file, false)
        .map_err(|error| detail(format!("retained DLSS snapshot is invalid: {error}")))?;
    Ok(Some(LiveImage { file, bytes }))
}

pub(super) fn ensure_compatible(
    target: &PathRef,
    live: &LiveDlss<'_>,
    bundled: &BundledDlss,
) -> Result<(), LumaActiveUpdateError> {
    if renderpilot_domain::dlss::versions_are_compatible(&live.version, &bundled.version) {
        Ok(())
    } else {
        Err(detail(format!(
            "live and bundled DLSS generations are incompatible at {target}"
        )))
    }
}

pub(super) fn require_persisted_live<'a>(
    target: &PathRef,
    live: Option<&'a LiveImage<'a>>,
    expected: &renderpilot_domain::Sha256Hash,
) -> Result<&'a LiveImage<'a>, LumaActiveUpdateError> {
    let live =
        live.ok_or_else(|| invalid("persisted DLSS binding is missing from the game tree"))?;
    if live.file.digest() != expected {
        return Err(detail(format!(
            "persisted DLSS binding drifted at {target}; catalog cannot heal managed drift"
        )));
    }
    Ok(live)
}

pub(super) fn validate_catalog_live(
    claim: &CatalogPathClaim,
    digest: &renderpilot_domain::Sha256Hash,
) -> Result<(), LumaActiveUpdateError> {
    if claim.active_hashes().iter().any(|hash| hash != digest) {
        return Err(invalid(
            "catalog active DLSS claim drifted from the observed live digest",
        ));
    }
    Ok(())
}

pub(super) fn validate_catalog_absent(
    claim: &CatalogPathClaim,
) -> Result<(), LumaActiveUpdateError> {
    if !claim.active_hashes().is_empty() {
        return Err(invalid(
            "catalog owns the DLSS endpoint and cannot be acquired by Luma",
        ));
    }
    Ok(())
}

pub(super) fn validate_owned_identity(
    binding: &ManagedAddonFile,
    claim: &CatalogPathClaim,
) -> Result<(), LumaActiveUpdateError> {
    if let Some(catalog_baseline) = claim.baseline()
        && catalog_baseline != binding.baseline()
    {
        return Err(invalid(
            "catalog and persisted DLSS binding disagree about the baseline",
        ));
    }
    Ok(())
}

pub(super) fn validate_release_baseline(
    binding: &ManagedAddonFile,
    target: &PathRef,
) -> Result<(), LumaActiveUpdateError> {
    if let ManagedFileBaseline::Present { sha256 } = binding.baseline()
        && sha256 == binding.installed_sha256()
    {
        return Err(detail(format!(
            "owned DLSS release has byte-identical installed and baseline images at {target}"
        )));
    }
    Ok(())
}

pub(super) fn validate_owned_baseline(
    authority: &LumaPeerRootAuthority,
    binding: &ManagedAddonFile,
    claim: &CatalogPathClaim,
    target: &PathRef,
    require_sidecar: bool,
) -> Result<Option<LiveOwnedImage>, LumaActiveUpdateError> {
    validate_owned_identity(binding, claim)?;
    let sidecar_path =
        renderpilot_domain::managed_sidecar_path(target).map_err(LumaActiveUpdateError::domain)?;
    let sidecar = observe_path(authority, &sidecar_path)?;
    match binding.baseline() {
        ManagedFileBaseline::Absent => {
            require_absent_sidecar(&sidecar_path, &sidecar)?;
            Ok(None)
        }
        ManagedFileBaseline::Present { sha256 } => {
            let image = sidecar
                .file()
                .ok_or_else(|| invalid("persisted DLSS baseline requires a present sidecar"))?;
            let bytes = sidecar
                .bytes()
                .ok_or_else(|| invalid("DLSS sidecar has no bytes"))?;
            crate::addons::luma::peer::effects::ensure_bytes_match_image(
                &sidecar_path,
                bytes,
                image,
                true,
            )
            .map_err(|error| detail(format!("DLSS baseline sidecar is invalid: {error}")))?;
            if image.digest() != sha256 {
                return Err(invalid(
                    "persisted DLSS baseline sidecar digest does not match",
                ));
            }
            if require_sidecar {
                Ok(Some(LiveOwnedImage {
                    file: image.clone(),
                    bytes: bytes.to_vec(),
                }))
            } else {
                Ok(None)
            }
        }
    }
}

pub(super) fn observe_sidecar(
    authority: &LumaPeerRootAuthority,
    target: &PathRef,
) -> Result<(PathRef, PeerPathSnapshot), LumaActiveUpdateError> {
    let sidecar =
        renderpilot_domain::managed_sidecar_path(target).map_err(LumaActiveUpdateError::domain)?;
    let snapshot = observe_path(authority, &sidecar)?;
    Ok((sidecar, snapshot))
}

pub(super) fn observe_path(
    authority: &LumaPeerRootAuthority,
    path: &PathRef,
) -> Result<PeerPathSnapshot, LumaActiveUpdateError> {
    let root = authority
        .authorized_root(path)
        .map_err(LumaActiveUpdateError::authority)?;
    observe_peer_path_snapshot(path, root)
        .map_err(|error| LumaActiveUpdateError::observation(path.clone(), error))
}

pub(super) fn require_absent_sidecar(
    path: &PathRef,
    snapshot: &PeerPathSnapshot,
) -> Result<(), LumaActiveUpdateError> {
    if matches!(snapshot, PeerPathSnapshot::Absent) {
        Ok(())
    } else {
        Err(detail(format!("DLSS sidecar must be absent at {path}")))
    }
}

pub(super) fn into_owned(live: &LiveImage<'_>) -> LiveOwnedImage {
    LiveOwnedImage {
        file: live.file.clone(),
        bytes: live.bytes.to_vec(),
    }
}

pub(super) fn invalid(reason: &'static str) -> LumaActiveUpdateError {
    LumaActiveUpdateError::invalid_input(reason)
}

fn detail(reason: String) -> LumaActiveUpdateError {
    LumaActiveUpdateError::invalid_input_detail(reason)
}
