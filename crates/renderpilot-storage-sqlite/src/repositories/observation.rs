use renderpilot_application::{AppError, AppResult};

/// Result of observing one durable row without collapsing invalid data into
/// absence.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum RowObservation<T> {
    /// No row matched the observation key.
    Missing,
    /// A matching row was decoded and validated successfully.
    Present(T),
    /// A matching row was decoded but rejected by its typed validator.
    Invalid(AppError),
}

impl<T> RowObservation<T> {
    pub(crate) fn into_optional(self) -> AppResult<Option<T>> {
        match self {
            Self::Missing => Ok(None),
            Self::Present(value) => Ok(Some(value)),
            Self::Invalid(error) => Err(error),
        }
    }
}
