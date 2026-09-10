use std::{error::Error, fmt};

use renderpilot_domain::{PathRef, ProxyTopologyError};

use crate::ServiceError;

use super::super::catalog_cascade::LumaCatalogCascadeError;
use super::super::effects::LumaPeerEffectError;
use super::super::engine_uninstall::EngineUninstallError;
use super::super::managed_dlss::ManagedDlssUninstallError;
use super::super::managed_host::ManagedHostReleaseError;

/// Errors raised while joining the closed Luma uninstall lowerers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LumaActiveUninstallCompositionError {
    InvalidRecord(&'static str),
    Topology(ProxyTopologyError),
    Authority(ServiceError),
    Catalog(LumaCatalogCascadeError),
    Engine(EngineUninstallError),
    ManagedDlss(ManagedDlssUninstallError),
    ManagedHost(ManagedHostReleaseError),
    Effects(LumaPeerEffectError),
    MissingRelease(PathRef),
    DuplicateRelease(PathRef),
    ExtraneousRelease(PathRef),
    ReleaseBindingMismatch(PathRef),
    NonDlssBinding(PathRef),
    ReusedCatalogPath(PathRef),
    ConsumedReleasePlan(PathRef),
    EmptyProgram,
}

impl fmt::Display for LumaActiveUninstallCompositionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRecord(reason) => {
                write!(formatter, "invalid active Luma uninstall record: {reason}")
            }
            Self::Topology(error) => write!(formatter, "invalid active Luma topology: {error}"),
            Self::Authority(error) => {
                write!(
                    formatter,
                    "Luma uninstall authority rejected input: {error}"
                )
            }
            Self::Catalog(error) => write!(formatter, "Luma catalog cascade failed: {error}"),
            Self::Engine(error) => write!(formatter, "Luma engine uninstall failed: {error}"),
            Self::ManagedDlss(error) => {
                write!(formatter, "Luma managed DLSS release failed: {error}")
            }
            Self::ManagedHost(error) => {
                write!(formatter, "Luma managed host release failed: {error}")
            }
            Self::Effects(error) => write!(formatter, "Luma effect finalization failed: {error}"),
            Self::MissingRelease(path) => write!(formatter, "missing managed Luma release: {path}"),
            Self::DuplicateRelease(path) => {
                write!(formatter, "duplicate managed Luma release: {path}")
            }
            Self::ExtraneousRelease(path) => {
                write!(formatter, "extraneous managed Luma release: {path}")
            }
            Self::ReleaseBindingMismatch(path) => {
                write!(formatter, "managed Luma release binding mismatch: {path}")
            }
            Self::NonDlssBinding(path) => {
                write!(formatter, "non-DLSS managed Luma binding: {path}")
            }
            Self::ReusedCatalogPath(path) => {
                write!(
                    formatter,
                    "reused managed Luma path is catalog-consumed: {path}"
                )
            }
            Self::ConsumedReleasePlan(path) => {
                write!(
                    formatter,
                    "catalog-consumed Luma release must keep its binding: {path}"
                )
            }
            Self::EmptyProgram => formatter.write_str("active Luma uninstall produced no effects"),
        }
    }
}

impl Error for LumaActiveUninstallCompositionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Topology(error) => Some(error),
            Self::Authority(error) => Some(error),
            Self::Catalog(error) => Some(error),
            Self::Engine(error) => Some(error),
            Self::ManagedDlss(error) => Some(error),
            Self::ManagedHost(error) => Some(error),
            Self::Effects(error) => Some(error),
            _ => None,
        }
    }
}

impl From<LumaCatalogCascadeError> for LumaActiveUninstallCompositionError {
    fn from(error: LumaCatalogCascadeError) -> Self {
        Self::Catalog(error)
    }
}

impl From<EngineUninstallError> for LumaActiveUninstallCompositionError {
    fn from(error: EngineUninstallError) -> Self {
        Self::Engine(error)
    }
}

impl From<ManagedDlssUninstallError> for LumaActiveUninstallCompositionError {
    fn from(error: ManagedDlssUninstallError) -> Self {
        Self::ManagedDlss(error)
    }
}

impl From<ManagedHostReleaseError> for LumaActiveUninstallCompositionError {
    fn from(error: ManagedHostReleaseError) -> Self {
        Self::ManagedHost(error)
    }
}
