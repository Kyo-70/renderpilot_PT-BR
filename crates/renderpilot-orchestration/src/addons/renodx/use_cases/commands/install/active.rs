//! Immutable phase resolution for active RenoDX installs.
//!
//! This module only seals read-side evidence.  It does not route a command,
//! fetch bytes, or perform a mutation; a later active route consumes the
//! owned resolution returned here.

mod commit;
mod fingerprint;
mod local_file;
mod model;
mod orchestrate;
mod phase;
mod plan;
mod validation;

pub(super) async fn install(
    request: super::InstallRequest<'_>,
    selected_topology: renderpilot_domain::GameProxyTopology,
) -> Result<renderpilot_domain::InstalledAddon, crate::ServiceError> {
    orchestrate::install(request, selected_topology).await
}

pub(super) async fn install_from_file(
    request: super::InstallRequest<'_>,
    file_path: &str,
    selected_topology: renderpilot_domain::GameProxyTopology,
) -> Result<renderpilot_domain::InstalledAddon, crate::ServiceError> {
    orchestrate::install_from_file(request, file_path, selected_topology).await
}
