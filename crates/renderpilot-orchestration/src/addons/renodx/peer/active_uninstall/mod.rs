//! Pure and snapshot-boundary implementation of active RenoDX uninstall.

mod compose;
mod effects;
mod error;
mod host;
mod ini;
mod model;
mod snapshot;
mod validation;

#[cfg(test)]
mod tests;

pub(crate) use compose::compose_active_uninstall;
pub(crate) use error::RenoDxActiveUninstallError;
pub(crate) use model::ActiveUninstallComposition;
pub(crate) use snapshot::snapshot_active_uninstall;
