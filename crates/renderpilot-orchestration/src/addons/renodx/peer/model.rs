//! Immutable active-install projections shared by the RenoDX command phases.

use std::path::{Path, PathBuf};

use renderpilot_domain::{GameProxyTopology, PathRef, RenoDxReshadeIniFeature, Sha256Hash};

use crate::addons::reshade::types::ReshadeChannel;
use crate::addons::reshade::{host_policy::TopologyHostAssessment, scan::ReshadeContent};
use crate::peer_mutation_executor::PeerPathSnapshot;

/// Which public RenoDX install command produced an active-install snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InstallCommandVariant {
    /// Installation resolved from the RenoDX catalogue.
    Catalog,
    /// Installation using a caller-selected local add-on file.
    InstallFromFile,
}

/// The exact retained ReShade configuration source.
///
/// A present file keeps the bytes and metadata read through one verified
/// handle.  An absent file keeps only the exact path that a later typed
/// endpoint may create.  No caller needs to rediscover the configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RenoDxConfigSourceSeal {
    /// No configuration existed at the exact active root during phase one.
    Absent { exact_ini_path: PathBuf },
    /// A regular, non-link configuration retained from the phase-one read.
    File {
        exact_ini_path: PathBuf,
        identity: String,
        digest: Sha256Hash,
        length: u64,
        owned_bytes: Vec<u8>,
        raw_addon_path_token: Option<String>,
    },
}

impl RenoDxConfigSourceSeal {
    /// Returns the exact configuration path, whether present or absent.
    pub(crate) fn exact_ini_path(&self) -> &Path {
        match self {
            Self::Absent { exact_ini_path } | Self::File { exact_ini_path, .. } => exact_ini_path,
        }
    }

    /// Returns the retained bytes for a present configuration.
    #[cfg(test)]
    pub(crate) fn owned_bytes(&self) -> Option<&[u8]> {
        match self {
            Self::Absent { .. } => None,
            Self::File { owned_bytes, .. } => Some(owned_bytes),
        }
    }

    /// Returns the exact raw `[ADDON] AddonPath` token, if present.
    #[cfg(test)]
    pub(crate) fn raw_addon_path_token(&self) -> Option<&str> {
        match self {
            Self::Absent { .. } => None,
            Self::File {
                raw_addon_path_token,
                ..
            } => raw_addon_path_token.as_deref(),
        }
    }

    #[cfg(test)]
    pub(crate) fn is_present(&self) -> bool {
        matches!(self, Self::File { .. })
    }
}

/// All roots and exact endpoint paths selected during phase one.
#[derive(Debug, Clone)]
pub(crate) struct RenoDxRootSeal {
    pub(crate) canonical_game_root: PathBuf,
    pub(crate) canonical_game_root_ref: PathRef,
    pub(crate) config_source: RenoDxConfigSourceSeal,
    pub(crate) effective_addon_root: PathBuf,
    pub(crate) payload_root: Option<PathBuf>,
    pub(crate) payload_root_ref: Option<PathRef>,
    pub(crate) exact_ini_path: PathBuf,
    pub(crate) exact_proxy_host: Option<PathBuf>,
    pub(crate) canonical_registered_exe: Option<PathBuf>,
    pub(crate) roots: crate::addons::peer_lifecycle::PeerRoots,
}

impl PartialEq for RenoDxRootSeal {
    fn eq(&self, other: &Self) -> bool {
        self.canonical_game_root == other.canonical_game_root
            && self.canonical_game_root_ref == other.canonical_game_root_ref
            && self.config_source == other.config_source
            && self.effective_addon_root == other.effective_addon_root
            && self.payload_root == other.payload_root
            && self.payload_root_ref == other.payload_root_ref
            && self.exact_ini_path == other.exact_ini_path
            && self.exact_proxy_host == other.exact_proxy_host
            && self.canonical_registered_exe == other.canonical_registered_exe
            && self.roots.roots() == other.roots.roots()
    }
}

impl Eq for RenoDxRootSeal {}

impl RenoDxRootSeal {
    pub(crate) fn canonical_game_root(&self) -> &Path {
        &self.canonical_game_root
    }

    pub(crate) fn canonical_game_root_ref(&self) -> &PathRef {
        &self.canonical_game_root_ref
    }

    pub(crate) fn config_source(&self) -> &RenoDxConfigSourceSeal {
        &self.config_source
    }

    pub(crate) fn effective_addon_root(&self) -> &Path {
        &self.effective_addon_root
    }

    pub(crate) fn payload_root(&self) -> Option<&Path> {
        self.payload_root.as_deref()
    }

    pub(crate) fn payload_root_ref(&self) -> Option<&PathRef> {
        self.payload_root_ref.as_ref()
    }

    pub(crate) fn exact_ini_path(&self) -> &Path {
        &self.exact_ini_path
    }

    pub(crate) fn exact_proxy_host(&self) -> Option<&Path> {
        self.exact_proxy_host.as_deref()
    }

    pub(crate) fn canonical_registered_exe(&self) -> Option<&Path> {
        self.canonical_registered_exe.as_deref()
    }

    pub(crate) fn roots(&self) -> &crate::addons::peer_lifecycle::PeerRoots {
        &self.roots
    }
}

/// Immutable phase-one facts consumed by the active install composer.
#[derive(Debug)]
pub(crate) struct InstallActiveSnapshot {
    pub(crate) variant: InstallCommandVariant,
    pub(crate) feature: RenoDxReshadeIniFeature,
    pub(crate) request_fingerprint: Sha256Hash,
    pub(crate) plan_fingerprint: Sha256Hash,
    pub(crate) requested_channel: ReshadeChannel,
    pub(crate) canonical_target_dir: PathBuf,
    pub(crate) canonical_target_dir_ref: PathRef,
    pub(crate) topology: Option<GameProxyTopology>,
    pub(crate) root_seal: RenoDxRootSeal,
    pub(crate) payload_path: PathRef,
    pub(crate) payload_preimage: PeerPathSnapshot,
    pub(crate) host_path: Option<PathRef>,
    pub(crate) host_preimage: Option<PeerPathSnapshot>,
    pub(crate) host_assessment: Option<TopologyHostAssessment>,
    pub(crate) acquisition_sidecar_preimage: Option<PeerPathSnapshot>,
    pub(crate) registered_executable: Option<PathRef>,
    pub(crate) writes_host: bool,
    pub(crate) content: ReshadeContent,
}

impl InstallActiveSnapshot {
    pub(crate) const fn feature(&self) -> RenoDxReshadeIniFeature {
        self.feature
    }

    pub(crate) const fn requested_channel(&self) -> ReshadeChannel {
        self.requested_channel
    }

    pub(crate) fn topology(&self) -> Option<&GameProxyTopology> {
        self.topology.as_ref()
    }

    pub(crate) fn root_seal(&self) -> &RenoDxRootSeal {
        &self.root_seal
    }

    pub(crate) fn payload_path(&self) -> &PathRef {
        &self.payload_path
    }

    pub(crate) fn payload_preimage(&self) -> &PeerPathSnapshot {
        &self.payload_preimage
    }

    pub(crate) fn host_path(&self) -> Option<&PathRef> {
        self.host_path.as_ref()
    }

    pub(crate) fn host_preimage(&self) -> Option<&PeerPathSnapshot> {
        self.host_preimage.as_ref()
    }

    pub(crate) fn host_assessment(&self) -> Option<&TopologyHostAssessment> {
        self.host_assessment.as_ref()
    }

    pub(crate) fn acquisition_sidecar_preimage(&self) -> Option<&PeerPathSnapshot> {
        self.acquisition_sidecar_preimage.as_ref()
    }

    pub(crate) fn registered_executable(&self) -> Option<&PathRef> {
        self.registered_executable.as_ref()
    }

    pub(crate) const fn writes_host(&self) -> bool {
        self.writes_host
    }

    pub(crate) const fn content(&self) -> ReshadeContent {
        self.content
    }

    /// Compares every phase-three fact that can affect the active route.
    #[must_use]
    pub(crate) fn phase3_matches(&self, other: &Self) -> bool {
        self.variant == other.variant
            && self.feature == other.feature
            && self.request_fingerprint == other.request_fingerprint
            && self.plan_fingerprint == other.plan_fingerprint
            && self.requested_channel == other.requested_channel
            && self.canonical_target_dir == other.canonical_target_dir
            && self.canonical_target_dir_ref == other.canonical_target_dir_ref
            && self.topology == other.topology
            && self.root_seal == other.root_seal
            && self.payload_path == other.payload_path
            && self.payload_preimage == other.payload_preimage
            && self.host_path == other.host_path
            && self.host_preimage == other.host_preimage
            && self.host_assessment == other.host_assessment
            && self.acquisition_sidecar_preimage == other.acquisition_sidecar_preimage
            && self.registered_executable == other.registered_executable
            && self.writes_host == other.writes_host
            && self.content == other.content
    }

    /// Maps phase-three drift to the install retry contract.
    pub(crate) fn ensure_phase3_matches(&self, other: &Self) -> Result<(), crate::ServiceError> {
        if self.phase3_matches(other) {
            Ok(())
        } else {
            Err(crate::addons::renodx::errors::state_changed_retry_install())
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use tempfile::tempdir;

    use super::*;
    use crate::addons::peer_lifecycle::PeerRoots;

    fn hash(value: char) -> Sha256Hash {
        Sha256Hash::new(value.to_string().repeat(Sha256Hash::HEX_LENGTH)).expect("hash")
    }

    fn snapshot(root: &Path, request_fingerprint: Sha256Hash) -> InstallActiveSnapshot {
        let root = crate::paths::canonicalize_existing(root).expect("canonical root");
        let root_ref =
            PathRef::new(root.to_str().expect("UTF-8 root").to_owned()).expect("root ref");
        let ini_path = root.join("ReShade.ini");
        let seal = RenoDxRootSeal {
            canonical_game_root: root.clone(),
            canonical_game_root_ref: root_ref.clone(),
            config_source: RenoDxConfigSourceSeal::Absent {
                exact_ini_path: ini_path.clone(),
            },
            effective_addon_root: root.clone(),
            payload_root: None,
            payload_root_ref: None,
            exact_ini_path: ini_path,
            exact_proxy_host: None,
            canonical_registered_exe: None,
            roots: PeerRoots::new(root.clone(), None).expect("roots"),
        };
        InstallActiveSnapshot {
            variant: InstallCommandVariant::Catalog,
            feature: RenoDxReshadeIniFeature::Install,
            request_fingerprint,
            plan_fingerprint: hash('b'),
            requested_channel: ReshadeChannel::Stable,
            canonical_target_dir: root,
            canonical_target_dir_ref: root_ref.clone(),
            topology: None,
            root_seal: seal,
            payload_path: PathRef::new(format!("{}/addon.addon64", root_ref.as_str()))
                .expect("payload path"),
            payload_preimage: PeerPathSnapshot::Absent,
            host_path: None,
            host_preimage: None,
            host_assessment: None,
            acquisition_sidecar_preimage: None,
            registered_executable: None,
            writes_host: false,
            content: ReshadeContent::Empty,
        }
    }

    #[test]
    fn phase3_drift_compares_sealed_facts_and_maps_to_retry() {
        let root = tempdir().expect("root");
        let initial = snapshot(root.path(), hash('a'));
        let same = snapshot(root.path(), hash('a'));
        let changed = snapshot(root.path(), hash('c'));

        assert!(initial.phase3_matches(&same));
        assert!(initial.ensure_phase3_matches(&same).is_ok());
        assert!(!initial.phase3_matches(&changed));
        let error = initial
            .ensure_phase3_matches(&changed)
            .expect_err("drift must request a retry");
        assert!(error.to_string().contains("retry the install"));
    }
}
