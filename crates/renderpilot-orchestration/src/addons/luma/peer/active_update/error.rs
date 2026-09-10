use renderpilot_domain::{PathRef, PeerTransitionError};

use crate::ServiceError;

use super::super::{
    active_dgvoodoo::ActiveDgVoodooError,
    active_dlss::{ActiveDlssClassificationError, ActiveDlssLoweringError},
    active_host::{ActiveHostClassificationError, ActiveHostLoweringError},
    active_payload::ActivePayloadError,
    catalog_cascade::LumaCatalogCascadeError,
    dgvoodoo::DgVoodooLoweringError,
    effects::LumaPeerEffectError,
    generic::LumaGenericLoweringError,
    host::LumaHostLoweringError,
    managed_dlss::ManagedDlssUninstallError,
};

mod conversions;
mod display;

/// Opaque failure from the active Luma update boundary.
///
/// The public error value is required by the command-facing composer, while
/// its concrete lowering causes remain private to the peer implementation.
/// Keeping the typed cause in the private kind preserves source inspection and
/// avoids leaking implementation-only error types through the API.
#[derive(Debug)]
pub(crate) struct LumaActiveUpdateError {
    kind: LumaActiveUpdateErrorKind,
}

#[derive(Debug)]
enum LumaActiveUpdateErrorKind {
    InvalidInput(&'static str),
    InvalidInputDetail(String),
    Authority(ServiceError),
    Observation { path: PathRef, error: ServiceError },
    Payload(ActivePayloadError),
    Generic(LumaGenericLoweringError),
    HostClassification(ActiveHostClassificationError),
    ActiveHostLowering(ActiveHostLoweringError),
    HostLowering(LumaHostLoweringError),
    DgVoodooPlan(ActiveDgVoodooError),
    DgVoodooLowering(DgVoodooLoweringError),
    DlssClassification(ActiveDlssClassificationError),
    DlssLowering(ActiveDlssLoweringError),
    ManagedDlss(ManagedDlssUninstallError),
    CatalogCascade(LumaCatalogCascadeError),
    Effects(LumaPeerEffectError),
    Domain(PeerTransitionError),
    Metadata(ServiceError),
    EndpointFreeTopology,
    EndpointFreeMtime,
    EndpointFreeCascade(&'static str),
}

impl LumaActiveUpdateError {
    pub(super) fn invalid_input(reason: &'static str) -> Self {
        Self {
            kind: LumaActiveUpdateErrorKind::InvalidInput(reason),
        }
    }

    pub(super) fn invalid_input_detail(reason: String) -> Self {
        Self {
            kind: LumaActiveUpdateErrorKind::InvalidInputDetail(reason),
        }
    }

    pub(super) fn authority(error: ServiceError) -> Self {
        Self {
            kind: LumaActiveUpdateErrorKind::Authority(error),
        }
    }

    pub(super) fn observation(path: PathRef, error: ServiceError) -> Self {
        Self {
            kind: LumaActiveUpdateErrorKind::Observation { path, error },
        }
    }

    pub(super) fn payload(error: ActivePayloadError) -> Self {
        Self {
            kind: LumaActiveUpdateErrorKind::Payload(error),
        }
    }

    pub(super) fn generic(error: LumaGenericLoweringError) -> Self {
        Self {
            kind: LumaActiveUpdateErrorKind::Generic(error),
        }
    }

    pub(super) fn host_classification(error: ActiveHostClassificationError) -> Self {
        Self {
            kind: LumaActiveUpdateErrorKind::HostClassification(error),
        }
    }

    pub(super) fn active_host_lowering(error: ActiveHostLoweringError) -> Self {
        Self {
            kind: LumaActiveUpdateErrorKind::ActiveHostLowering(error),
        }
    }

    pub(super) fn host_lowering(error: LumaHostLoweringError) -> Self {
        Self {
            kind: LumaActiveUpdateErrorKind::HostLowering(error),
        }
    }

    pub(super) fn dg_voodoo_plan(error: ActiveDgVoodooError) -> Self {
        Self {
            kind: LumaActiveUpdateErrorKind::DgVoodooPlan(error),
        }
    }

    pub(super) fn dg_voodoo_lowering(error: DgVoodooLoweringError) -> Self {
        Self {
            kind: LumaActiveUpdateErrorKind::DgVoodooLowering(error),
        }
    }

    pub(super) fn dlss_classification(error: ActiveDlssClassificationError) -> Self {
        Self {
            kind: LumaActiveUpdateErrorKind::DlssClassification(error),
        }
    }

    pub(super) fn dlss_lowering(error: ActiveDlssLoweringError) -> Self {
        Self {
            kind: LumaActiveUpdateErrorKind::DlssLowering(error),
        }
    }

    pub(super) fn managed_dlss(error: ManagedDlssUninstallError) -> Self {
        Self {
            kind: LumaActiveUpdateErrorKind::ManagedDlss(error),
        }
    }

    pub(super) fn catalog_cascade(error: LumaCatalogCascadeError) -> Self {
        Self {
            kind: LumaActiveUpdateErrorKind::CatalogCascade(error),
        }
    }

    pub(super) fn effects(error: LumaPeerEffectError) -> Self {
        Self {
            kind: LumaActiveUpdateErrorKind::Effects(error),
        }
    }

    pub(super) fn domain(error: PeerTransitionError) -> Self {
        Self {
            kind: LumaActiveUpdateErrorKind::Domain(error),
        }
    }

    pub(super) fn metadata(error: ServiceError) -> Self {
        Self {
            kind: LumaActiveUpdateErrorKind::Metadata(error),
        }
    }

    pub(super) fn endpoint_free_topology() -> Self {
        Self {
            kind: LumaActiveUpdateErrorKind::EndpointFreeTopology,
        }
    }

    pub(super) fn endpoint_free_mtime() -> Self {
        Self {
            kind: LumaActiveUpdateErrorKind::EndpointFreeMtime,
        }
    }

    pub(super) fn endpoint_free_cascade(reason: &'static str) -> Self {
        Self {
            kind: LumaActiveUpdateErrorKind::EndpointFreeCascade(reason),
        }
    }

    #[cfg(test)]
    pub(super) fn is_invalid_input(&self) -> bool {
        matches!(
            self.kind,
            LumaActiveUpdateErrorKind::InvalidInput(_)
                | LumaActiveUpdateErrorKind::InvalidInputDetail(_)
        )
    }

    #[cfg(test)]
    pub(super) fn is_domain(&self) -> bool {
        matches!(self.kind, LumaActiveUpdateErrorKind::Domain(_))
    }

    #[cfg(test)]
    pub(super) fn is_active_host_lowering(&self) -> bool {
        matches!(self.kind, LumaActiveUpdateErrorKind::ActiveHostLowering(_))
    }

    #[cfg(test)]
    pub(super) fn is_host_classification(&self) -> bool {
        matches!(self.kind, LumaActiveUpdateErrorKind::HostClassification(_))
    }

    #[cfg(test)]
    pub(super) fn is_endpoint_free_topology(&self) -> bool {
        matches!(self.kind, LumaActiveUpdateErrorKind::EndpointFreeTopology)
    }

    #[cfg(test)]
    pub(super) fn is_endpoint_free_mtime(&self) -> bool {
        matches!(self.kind, LumaActiveUpdateErrorKind::EndpointFreeMtime)
    }

    #[cfg(test)]
    pub(super) fn is_endpoint_free_cascade(&self) -> bool {
        matches!(self.kind, LumaActiveUpdateErrorKind::EndpointFreeCascade(_))
    }
}
