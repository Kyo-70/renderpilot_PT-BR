use renderpilot_domain::{ManagedAddonFile, ManagedFileBaseline};

use crate::addons::luma::peer::active_update::{
    dlss::{
        evidence,
        model::{BundledDlss, DlssAction, InputKind, LiveDlss, PersistedDlss},
    },
    error::LumaActiveUpdateError,
};
use crate::addons::luma::peer::root_authority::LumaPeerRootAuthority;
use crate::catalog::cascade::CascadeResult;
use crate::coordinated_files::CatalogPathClaim;

/// Classifies Preserve and Full-without-DLSS routes. A missing persisted
/// binding deliberately carries no live observation: foreign bytes are not
/// Luma authority and must remain untouched.
pub(super) fn classify_without_bundled(
    authority: &LumaPeerRootAuthority,
    target: renderpilot_domain::PathRef,
    persisted: PersistedDlss,
    input_kind: InputKind,
    live: Option<&super::model::LiveImage<'_>>,
    catalog_claim: &CatalogPathClaim,
    cascade: &CascadeResult,
) -> Result<DlssAction, LumaActiveUpdateError> {
    match (persisted, input_kind) {
        (PersistedDlss::None, _) => Ok(DlssAction::Noop { binding: None }),
        (PersistedDlss::Reused(binding), InputKind::Preserve) => {
            evidence::require_persisted_live(&target, live, binding.installed_sha256())?;
            evidence::validate_catalog_live(catalog_claim, binding.installed_sha256())?;
            Ok(DlssAction::Noop {
                binding: Some(binding),
            })
        }
        (PersistedDlss::Owned(binding), InputKind::Preserve) => {
            evidence::require_persisted_live(&target, live, binding.installed_sha256())?;
            evidence::validate_owned_baseline(authority, &binding, catalog_claim, &target, false)?;
            Ok(DlssAction::Noop {
                binding: Some(binding),
            })
        }
        (PersistedDlss::Reused(binding), InputKind::Full) => {
            evidence::require_persisted_live(&target, live, binding.installed_sha256())?;
            evidence::validate_catalog_live(catalog_claim, binding.installed_sha256())?;
            Ok(DlssAction::Noop { binding: None })
        }
        (PersistedDlss::Owned(binding), InputKind::Full) => {
            let live = evidence::require_persisted_live(&target, live, binding.installed_sha256())?;
            evidence::validate_owned_identity(&binding, catalog_claim)?;
            evidence::validate_release_baseline(&binding, &target)?;
            if cascade.catalog_claim().is_some() {
                return Ok(DlssAction::CascadeRelease);
            }
            let sidecar = evidence::validate_owned_baseline(
                authority,
                &binding,
                catalog_claim,
                &target,
                true,
            )?;
            let (sidecar, baseline) = match binding.baseline() {
                ManagedFileBaseline::Absent => (None, None),
                ManagedFileBaseline::Present { .. } => {
                    let sidecar = sidecar.ok_or_else(|| {
                        evidence::invalid("present DLSS baseline requires a retained sidecar")
                    })?;
                    let baseline = sidecar.bytes.clone();
                    (Some(sidecar), Some(baseline))
                }
            };
            Ok(DlssAction::Release {
                target,
                live: evidence::into_owned(live),
                sidecar,
                baseline,
            })
        }
    }
}

/// Classifies a Full update that contains a validated bundled DLSS payload.
/// Only this path parses live DLSS metadata for compatibility/version policy.
pub(super) fn classify_with_bundled(
    authority: &LumaPeerRootAuthority,
    target: renderpilot_domain::PathRef,
    persisted: PersistedDlss,
    live: Option<LiveDlss<'_>>,
    bundled: BundledDlss,
    catalog_claim: &CatalogPathClaim,
) -> Result<DlssAction, LumaActiveUpdateError> {
    match (persisted, live) {
        (PersistedDlss::None, None) => {
            evidence::validate_catalog_absent(catalog_claim)?;
            let (sidecar_path, sidecar) = evidence::observe_sidecar(authority, &target)?;
            evidence::require_absent_sidecar(&sidecar_path, &sidecar)?;
            let binding = ManagedAddonFile::owned(
                target,
                ManagedFileBaseline::Absent,
                bundled.digest.clone(),
            );
            Ok(DlssAction::Create {
                binding,
                bytes: bundled.bytes,
            })
        }
        (PersistedDlss::None, Some(live)) => {
            evidence::validate_catalog_live(catalog_claim, live.image.file.digest())?;
            evidence::ensure_compatible(&target, &live, &bundled)?;
            if live.version >= bundled.version {
                return Ok(DlssAction::Noop {
                    binding: Some(ManagedAddonFile::reused(
                        target,
                        live.image.file.digest().clone(),
                    )),
                });
            }
            evidence::validate_catalog_absent(catalog_claim)?;
            let (sidecar_path, sidecar) = evidence::observe_sidecar(authority, &target)?;
            evidence::require_absent_sidecar(&sidecar_path, &sidecar)?;
            let binding = ManagedAddonFile::owned(
                target,
                ManagedFileBaseline::Present {
                    sha256: live.image.file.digest().clone(),
                },
                bundled.digest,
            );
            Ok(DlssAction::Acquire {
                binding,
                live: evidence::into_owned(&live.image),
                bytes: bundled.bytes,
            })
        }
        (PersistedDlss::Reused(_), None) => Err(evidence::invalid(
            "reused DLSS binding cannot adopt an absent live endpoint",
        )),
        (PersistedDlss::Reused(binding), Some(live)) => {
            evidence::require_persisted_live(
                &target,
                Some(&live.image),
                binding.installed_sha256(),
            )?;
            evidence::validate_catalog_live(catalog_claim, binding.installed_sha256())?;
            evidence::ensure_compatible(&target, &live, &bundled)?;
            if live.version >= bundled.version {
                return Ok(DlssAction::Noop {
                    binding: Some(binding),
                });
            }
            evidence::validate_catalog_absent(catalog_claim)?;
            let (sidecar_path, sidecar) = evidence::observe_sidecar(authority, &target)?;
            evidence::require_absent_sidecar(&sidecar_path, &sidecar)?;
            let binding = ManagedAddonFile::owned(
                target,
                ManagedFileBaseline::Present {
                    sha256: live.image.file.digest().clone(),
                },
                bundled.digest,
            );
            Ok(DlssAction::Acquire {
                binding,
                live: evidence::into_owned(&live.image),
                bytes: bundled.bytes,
            })
        }
        (PersistedDlss::Owned(_), None) => Err(evidence::invalid(
            "owned DLSS binding cannot adopt an absent live endpoint",
        )),
        (PersistedDlss::Owned(binding), Some(live)) => {
            evidence::require_persisted_live(
                &target,
                Some(&live.image),
                binding.installed_sha256(),
            )?;
            evidence::validate_owned_baseline(authority, &binding, catalog_claim, &target, false)?;
            if bundled.digest == *binding.installed_sha256() {
                return Ok(DlssAction::Noop {
                    binding: Some(binding),
                });
            }
            evidence::ensure_compatible(&target, &live, &bundled)?;
            if bundled.version <= live.version {
                return Ok(DlssAction::Noop {
                    binding: Some(binding),
                });
            }
            Ok(DlssAction::Replace {
                binding: ManagedAddonFile::owned(
                    target,
                    binding.baseline().clone(),
                    bundled.digest,
                ),
                live: evidence::into_owned(&live.image),
                bytes: bundled.bytes,
            })
        }
    }
}
