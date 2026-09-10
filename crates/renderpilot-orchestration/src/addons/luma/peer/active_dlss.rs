//! Pure planning and lowering for DLSS during an active-topology Luma install.
//!
//! The facade keeps classification and lowering behind a small, stable API;
//! each phase lives in a focused module and receives only retained evidence.

mod classification;
mod lowering;
mod model;

pub(crate) use classification::classify_active_dlss;
pub(crate) use lowering::lower_active_dlss_owned;
#[cfg(test)]
pub(crate) use model::ActiveDlssClassification;
pub(crate) use model::{ActiveDlssClassificationError, ActiveDlssLoweringError};
