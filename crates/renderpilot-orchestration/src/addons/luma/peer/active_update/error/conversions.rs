use renderpilot_domain::PeerTransitionError;

use crate::addons::luma::peer::{
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

use super::LumaActiveUpdateError;

impl From<ActivePayloadError> for LumaActiveUpdateError {
    fn from(error: ActivePayloadError) -> Self {
        Self::payload(error)
    }
}

impl From<LumaGenericLoweringError> for LumaActiveUpdateError {
    fn from(error: LumaGenericLoweringError) -> Self {
        Self::generic(error)
    }
}

impl From<ActiveHostClassificationError> for LumaActiveUpdateError {
    fn from(error: ActiveHostClassificationError) -> Self {
        Self::host_classification(error)
    }
}

impl From<ActiveHostLoweringError> for LumaActiveUpdateError {
    fn from(error: ActiveHostLoweringError) -> Self {
        Self::active_host_lowering(error)
    }
}

impl From<LumaHostLoweringError> for LumaActiveUpdateError {
    fn from(error: LumaHostLoweringError) -> Self {
        Self::host_lowering(error)
    }
}

impl From<ActiveDgVoodooError> for LumaActiveUpdateError {
    fn from(error: ActiveDgVoodooError) -> Self {
        Self::dg_voodoo_plan(error)
    }
}

impl From<DgVoodooLoweringError> for LumaActiveUpdateError {
    fn from(error: DgVoodooLoweringError) -> Self {
        Self::dg_voodoo_lowering(error)
    }
}

impl From<ActiveDlssClassificationError> for LumaActiveUpdateError {
    fn from(error: ActiveDlssClassificationError) -> Self {
        Self::dlss_classification(error)
    }
}

impl From<ActiveDlssLoweringError> for LumaActiveUpdateError {
    fn from(error: ActiveDlssLoweringError) -> Self {
        Self::dlss_lowering(error)
    }
}

impl From<ManagedDlssUninstallError> for LumaActiveUpdateError {
    fn from(error: ManagedDlssUninstallError) -> Self {
        Self::managed_dlss(error)
    }
}

impl From<LumaCatalogCascadeError> for LumaActiveUpdateError {
    fn from(error: LumaCatalogCascadeError) -> Self {
        Self::catalog_cascade(error)
    }
}

impl From<LumaPeerEffectError> for LumaActiveUpdateError {
    fn from(error: LumaPeerEffectError) -> Self {
        Self::effects(error)
    }
}

impl From<PeerTransitionError> for LumaActiveUpdateError {
    fn from(error: PeerTransitionError) -> Self {
        Self::domain(error)
    }
}
