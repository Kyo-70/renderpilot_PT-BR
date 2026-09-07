//! Facade for the closed peer-storage runtime.

mod commit;
mod fingerprint;
mod preparation;
mod types;

#[cfg(test)]
mod renodx_reshade_ini_tests;

pub use types::{
    PeerCommitPreparation, PeerStorageRuntime, PreparedPeerCommitPermit,
    SharedPeerCommitPreparation,
};

#[cfg(test)]
pub(crate) use fingerprint::read_file_fingerprint;
pub(crate) use fingerprint::{
    domain_error, ensure_runtime, read_file_manifest, read_shared_manifest,
};
pub(crate) use types::RenoDxOptiScalerConfigPeerCommit;
pub(crate) use types::RuntimeInstance;
