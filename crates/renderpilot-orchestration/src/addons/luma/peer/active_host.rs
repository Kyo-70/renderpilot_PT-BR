//! Pure planning and lowering for the ReShade downstream of an active
//! OptiScaler topology.
//!
//! The active host is a topology participant, not an ordinary ReShade host
//! discovered by scanning the game directory. The public facade keeps the
//! classification model, validation, and effect lowering separate so each
//! phase has one responsibility and remains independently auditable.

mod classification;
mod lowering;
mod model;
mod validation;

pub(crate) use classification::{
    classify_active_host, classify_active_host_from_validated_evidence, observed_topology,
    validate_active_host_evidence,
};
pub(crate) use lowering::lower_active_host_owned;
pub(crate) use model::{
    ActiveHostClassification, ActiveHostClassificationError, ActiveHostLoweringError,
};
pub(crate) use validation::{digest as active_host_digest, validate_prepared_host};
