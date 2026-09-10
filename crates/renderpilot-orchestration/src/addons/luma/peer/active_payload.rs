//! Planning and lowering of the generic payload for an active Luma install.
//!
//! The facade keeps the peer-facing API small. Structural validation,
//! filesystem observation, and effect lowering live in separate modules so a
//! future record composer cannot accidentally bypass one of those boundaries.

mod lowering;
mod model;
mod observation;
mod validation;

pub(crate) use lowering::lower_active_payload;
pub(crate) use model::{ActivePayloadError, ActivePayloadProjection, ValidatedActivePayloadTarget};
pub(crate) use validation::{is_exact_dlss_relative, validate_active_payload};

#[cfg(test)]
pub(crate) use model::ActivePayloadStructuralError;
