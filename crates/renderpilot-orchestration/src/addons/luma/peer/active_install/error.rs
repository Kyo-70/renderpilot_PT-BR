use std::{error::Error, fmt};

use renderpilot_domain::PathRef;

use crate::ServiceError;

use super::super::{
    active_dgvoodoo::ActiveDgVoodooError,
    active_dlss::{ActiveDlssClassificationError, ActiveDlssLoweringError},
    active_host::{ActiveHostClassificationError, ActiveHostLoweringError},
    active_payload::ActivePayloadError,
    effects::LumaPeerEffectError,
};

#[derive(Debug)]
pub(crate) enum LumaActiveInstallCompositionError {
    InvalidInput(&'static str),
    Authority(ServiceError),
    Payload(ActivePayloadError),
    DgVoodoo(ActiveDgVoodooError),
    DlssClassification(ActiveDlssClassificationError),
    DlssLowering(ActiveDlssLoweringError),
    HostClassification(ActiveHostClassificationError),
    HostLowering(ActiveHostLoweringError),
    Observation { path: PathRef, error: ServiceError },
    Effects(LumaPeerEffectError),
    Record(String),
    EmptyProgram,
}

impl fmt::Display for LumaActiveInstallCompositionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInput(reason) => {
                write!(formatter, "invalid active Luma install: {reason}")
            }
            Self::Authority(error) => write!(
                formatter,
                "active Luma install authority rejected input: {error}"
            ),
            Self::Payload(error) => write!(formatter, "active Luma payload failed: {error}"),
            Self::DgVoodoo(error) => write!(formatter, "active Luma dgVoodoo failed: {error}"),
            Self::DlssClassification(error) => {
                write!(formatter, "active Luma DLSS classification failed: {error}")
            }
            Self::DlssLowering(error) => {
                write!(formatter, "active Luma DLSS lowering failed: {error}")
            }
            Self::HostClassification(error) => {
                write!(formatter, "active Luma host classification failed: {error}")
            }
            Self::HostLowering(error) => {
                write!(formatter, "active Luma host lowering failed: {error}")
            }
            Self::Observation { path, error } => {
                write!(
                    formatter,
                    "active Luma observation failed at {path}: {error}"
                )
            }
            Self::Effects(error) => {
                write!(formatter, "active Luma effect finalization failed: {error}")
            }
            Self::Record(reason) => write!(formatter, "active Luma record failed: {reason}"),
            Self::EmptyProgram => formatter.write_str("active Luma install produced no effects"),
        }
    }
}

impl Error for LumaActiveInstallCompositionError {}

impl From<ActivePayloadError> for LumaActiveInstallCompositionError {
    fn from(error: ActivePayloadError) -> Self {
        Self::Payload(error)
    }
}

impl From<ActiveDgVoodooError> for LumaActiveInstallCompositionError {
    fn from(error: ActiveDgVoodooError) -> Self {
        Self::DgVoodoo(error)
    }
}

impl From<ActiveDlssClassificationError> for LumaActiveInstallCompositionError {
    fn from(error: ActiveDlssClassificationError) -> Self {
        Self::DlssClassification(error)
    }
}

impl From<ActiveDlssLoweringError> for LumaActiveInstallCompositionError {
    fn from(error: ActiveDlssLoweringError) -> Self {
        Self::DlssLowering(error)
    }
}

impl From<ActiveHostClassificationError> for LumaActiveInstallCompositionError {
    fn from(error: ActiveHostClassificationError) -> Self {
        Self::HostClassification(error)
    }
}

impl From<ActiveHostLoweringError> for LumaActiveInstallCompositionError {
    fn from(error: ActiveHostLoweringError) -> Self {
        Self::HostLowering(error)
    }
}

impl From<LumaPeerEffectError> for LumaActiveInstallCompositionError {
    fn from(error: LumaPeerEffectError) -> Self {
        Self::Effects(error)
    }
}
