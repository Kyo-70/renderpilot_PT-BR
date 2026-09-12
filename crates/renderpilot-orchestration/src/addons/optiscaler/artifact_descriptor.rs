//! Shared immutable transport identity for release and module cache materialization.

use std::path::PathBuf;

use renderpilot_domain::Sha256Hash;
use reqwest::Url;

use super::archive::{is_x64_pe, sha256_hex};
use super::types::{OptiScalerModuleArtifact, OptiScalerRelease};
use crate::{ServiceError, failed};

#[derive(Clone, Debug)]
pub(super) struct VerifiedArtifactDescriptor {
    pub id: String,
    pub source_url: Url,
    pub size: u64,
    pub sha256: Sha256Hash,
    pub cache_path: PathBuf,
    pub pe_x64: bool,
}

impl VerifiedArtifactDescriptor {
    pub fn release(release: &OptiScalerRelease) -> Result<Self, ServiceError> {
        let sha256 = Sha256Hash::new(release.archive_sha256.clone())
            .map_err(|error| failed(format!("invalid release archive hash: {error}")))?;
        Ok(Self {
            id: release.id.clone(),
            source_url: super::source::release_download_url(release)?,
            size: release.archive_size,
            cache_path: crate::app_dir::app_dir()?
                .join("cache")
                .join("optiscaler")
                .join(format!("{}.7z", sha256.as_str())),
            sha256,
            pe_x64: false,
        })
    }

    pub fn module(
        module_id: &str,
        artifact: &OptiScalerModuleArtifact,
    ) -> Result<Self, ServiceError> {
        let sha256 = Sha256Hash::new(artifact.sha256.clone())
            .map_err(|error| failed(format!("invalid module artifact hash: {error}")))?;
        Ok(Self {
            id: format!("{module_id}:{}", artifact.id),
            source_url: super::source::download_url_from_source(&artifact.source)?,
            size: artifact.size,
            cache_path: crate::app_dir::app_dir()?
                .join("cache")
                .join("optiscaler")
                .join("modules")
                .join(format!("{}.bin", sha256.as_str())),
            sha256,
            pe_x64: artifact.pe_x64,
        })
    }

    pub fn validate_bytes(&self, bytes: &[u8]) -> Result<(), ServiceError> {
        if bytes.len() as u64 != self.size
            || sha256_hex(bytes) != self.sha256.as_str()
            || (self.pe_x64 && !is_x64_pe(bytes))
        {
            return Err(failed(format!(
                "OptiScaler artifact {} does not match its pinned size, hash, or PE identity",
                self.id
            )));
        }
        Ok(())
    }
}
