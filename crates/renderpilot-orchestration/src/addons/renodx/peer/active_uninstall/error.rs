use std::path::PathBuf;

use crate::ServiceError;

#[derive(Debug)]
pub(crate) enum RenoDxActiveUninstallError {
    Invalid(&'static str),
    Path(PathBuf),
    Domain(ServiceError),
}

impl std::fmt::Display for RenoDxActiveUninstallError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(reason) => write!(formatter, "invalid RenoDX active uninstall: {reason}"),
            Self::Path(path) => write!(
                formatter,
                "invalid RenoDX active-uninstall path: {}",
                path.display()
            ),
            Self::Domain(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for RenoDxActiveUninstallError {}

impl From<ServiceError> for RenoDxActiveUninstallError {
    fn from(error: ServiceError) -> Self {
        Self::Domain(error)
    }
}
