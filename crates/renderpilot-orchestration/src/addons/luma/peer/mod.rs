//! Guard-bound persistence seams for Luma peer metadata.

mod active_dgvoodoo;
mod active_dlss;
mod active_host;
mod active_install;
mod active_payload;
mod active_update;
mod cascade;
pub(crate) mod catalog_cascade;
mod dgvoodoo;
mod effects;
mod engine_uninstall;
mod generic;
mod host;
mod managed_dlss;
mod managed_host;
mod record;
mod root_authority;
mod snapshot_input;
mod uninstall;

#[cfg(test)]
mod active_dgvoodoo_tests;
#[cfg(test)]
mod active_dlss_tests;
#[cfg(test)]
mod active_host_tests;
#[cfg(test)]
mod active_install_tests;
#[cfg(test)]
mod active_payload_tests;
#[cfg(test)]
mod cascade_tests;
#[cfg(test)]
mod catalog_cascade_tests;
#[cfg(test)]
mod dgvoodoo_tests;
#[cfg(test)]
mod effects_tests;
#[cfg(test)]
mod engine_uninstall_tests;
#[cfg(test)]
mod generic_tests;
#[cfg(test)]
mod host_tests;
#[cfg(test)]
mod managed_dlss_tests;
#[cfg(test)]
mod managed_host_tests;
#[cfg(test)]
mod record_tests;
#[cfg(test)]
mod root_authority_tests;
#[cfg(test)]
mod uninstall_tests;

pub(crate) use active_install::{LumaActiveInstallInput, compose_active_install};
pub(crate) use active_update::{
    LumaActiveUpdateAggregateMembership, LumaActiveUpdateComposition,
    LumaActiveUpdateDgVoodooInput, LumaActiveUpdateEvidence, LumaActiveUpdateHostInput,
    LumaActiveUpdateHostObservation, LumaActiveUpdateInput, LumaActiveUpdateMetadata,
    LumaActiveUpdatePayloadInput, LumaActiveUpdatePhysical, LumaActiveUpdatePrepared,
    compose_active_update,
};
pub(crate) use record::commit_metadata;
pub(crate) use root_authority::LumaPeerRootAuthority;
pub(crate) use uninstall::{PlannedManagedDlssRelease, compose_active_uninstall};
