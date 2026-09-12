//! Exact file identity checks for OptiScaler lifecycle plans.

use std::path::Path;

use renderpilot_domain::{PathRef, Sha256Hash};

use crate::{ServiceError, failed};

pub(crate) fn matches_optional(path: &Path, expected: Option<&Sha256Hash>) -> bool {
    expected.is_some_and(|hash| {
        renderpilot_detection::sha256_file(path).is_ok_and(|actual| &actual == hash)
    })
}

pub(crate) fn path_ref(path: &Path) -> Result<PathRef, ServiceError> {
    PathRef::new(path.to_string_lossy().into_owned()).map_err(|error| failed(error.to_string()))
}
