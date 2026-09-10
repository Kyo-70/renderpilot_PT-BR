use std::{error::Error, fmt};

use super::{LumaActiveUpdateError, LumaActiveUpdateErrorKind};

impl fmt::Display for LumaActiveUpdateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            LumaActiveUpdateErrorKind::InvalidInput(reason) => {
                write!(formatter, "active Luma update: invalid input: {reason}")
            }
            LumaActiveUpdateErrorKind::InvalidInputDetail(reason) => {
                write!(formatter, "active Luma update: invalid input: {reason}")
            }
            LumaActiveUpdateErrorKind::Authority(error) => write!(
                formatter,
                "active Luma update: authority rejected input: {error}"
            ),
            LumaActiveUpdateErrorKind::Observation { path, error } => write!(
                formatter,
                "active Luma update: observation failed at {path}: {error}"
            ),
            LumaActiveUpdateErrorKind::Payload(error) => {
                write!(formatter, "active Luma update: payload failed: {error}")
            }
            LumaActiveUpdateErrorKind::Generic(error) => write!(
                formatter,
                "active Luma update: generic payload failed: {error}"
            ),
            LumaActiveUpdateErrorKind::HostClassification(error) => write!(
                formatter,
                "active Luma update: host classification failed: {error}"
            ),
            LumaActiveUpdateErrorKind::ActiveHostLowering(error) => write!(
                formatter,
                "active Luma update: active host lowering failed: {error}"
            ),
            LumaActiveUpdateErrorKind::HostLowering(error) => {
                write!(
                    formatter,
                    "active Luma update: host lowering failed: {error}"
                )
            }
            LumaActiveUpdateErrorKind::DgVoodooPlan(error) => write!(
                formatter,
                "active Luma update: dgVoodoo planning failed: {error}"
            ),
            LumaActiveUpdateErrorKind::DgVoodooLowering(error) => write!(
                formatter,
                "active Luma update: dgVoodoo lowering failed: {error}"
            ),
            LumaActiveUpdateErrorKind::DlssClassification(error) => write!(
                formatter,
                "active Luma update: DLSS classification failed: {error}"
            ),
            LumaActiveUpdateErrorKind::DlssLowering(error) => {
                write!(
                    formatter,
                    "active Luma update: DLSS lowering failed: {error}"
                )
            }
            LumaActiveUpdateErrorKind::ManagedDlss(error) => {
                write!(
                    formatter,
                    "active Luma update: managed DLSS failed: {error}"
                )
            }
            LumaActiveUpdateErrorKind::CatalogCascade(error) => write!(
                formatter,
                "active Luma update: catalog cascade failed: {error}"
            ),
            LumaActiveUpdateErrorKind::Effects(error) => write!(
                formatter,
                "active Luma update: effect finalization failed: {error}"
            ),
            LumaActiveUpdateErrorKind::Domain(error) => write!(
                formatter,
                "active Luma update: domain rejected input: {error}"
            ),
            LumaActiveUpdateErrorKind::Metadata(error) => write!(
                formatter,
                "active Luma update: metadata validation failed: {error}"
            ),
            LumaActiveUpdateErrorKind::EndpointFreeTopology => write!(
                formatter,
                "active Luma update: endpoint-free update changed proxy topology"
            ),
            LumaActiveUpdateErrorKind::EndpointFreeMtime => write!(
                formatter,
                "active Luma update: endpoint-free update changed file provenance"
            ),
            LumaActiveUpdateErrorKind::EndpointFreeCascade(reason) => write!(
                formatter,
                "active Luma update: endpoint-free update selected catalog filesystem effects: {reason}"
            ),
        }
    }
}

impl Error for LumaActiveUpdateError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match &self.kind {
            LumaActiveUpdateErrorKind::Authority(error)
            | LumaActiveUpdateErrorKind::Metadata(error)
            | LumaActiveUpdateErrorKind::Observation { error, .. } => Some(error),
            LumaActiveUpdateErrorKind::Payload(error) => Some(error),
            LumaActiveUpdateErrorKind::Generic(error) => Some(error),
            LumaActiveUpdateErrorKind::HostClassification(error) => Some(error),
            LumaActiveUpdateErrorKind::ActiveHostLowering(error) => Some(error),
            LumaActiveUpdateErrorKind::HostLowering(error) => Some(error),
            LumaActiveUpdateErrorKind::DgVoodooPlan(error) => Some(error),
            LumaActiveUpdateErrorKind::DgVoodooLowering(error) => Some(error),
            LumaActiveUpdateErrorKind::DlssClassification(error) => Some(error),
            LumaActiveUpdateErrorKind::DlssLowering(error) => Some(error),
            LumaActiveUpdateErrorKind::ManagedDlss(error) => Some(error),
            LumaActiveUpdateErrorKind::CatalogCascade(error) => Some(error),
            LumaActiveUpdateErrorKind::Effects(error) => Some(error),
            LumaActiveUpdateErrorKind::Domain(error) => Some(error),
            LumaActiveUpdateErrorKind::InvalidInput(_)
            | LumaActiveUpdateErrorKind::InvalidInputDetail(_)
            | LumaActiveUpdateErrorKind::EndpointFreeTopology
            | LumaActiveUpdateErrorKind::EndpointFreeMtime
            | LumaActiveUpdateErrorKind::EndpointFreeCascade(_) => None,
        }
    }
}
