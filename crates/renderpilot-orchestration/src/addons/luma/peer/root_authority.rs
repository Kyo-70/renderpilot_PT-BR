//! Closed filesystem authority for one Luma peer operation.
//!
//! ReShade configuration is an input to this boundary only.  Once the
//! authority is constructed, no operation may re-read the configuration or
//! derive another root from a persisted record, catalog card, or endpoint.

use std::path::{Path, PathBuf};

use renderpilot_domain::{
    AddonKind, ComponentFile, InstalledAddon, NormalizedPathRelation, PathRef,
    PeerCatalogRollbackClaim, normalized_path_relation,
};

use crate::ServiceError;
use crate::addons::peer_lifecycle::PeerRoots;
use crate::addons::reshade::scan;

/// Immutable, Luma-private roots and content sealed at the operation boundary.
#[derive(Debug, Clone)]
pub(crate) struct LumaPeerRootAuthority {
    roots: PeerRoots,
    canonical_game_root: PathBuf,
    canonical_game_root_ref: PathRef,
    effective_addon_root: PathBuf,
    effective_addon_root_ref: PathRef,
    external_capability_root: Option<PathBuf>,
    external_capability_root_ref: Option<PathRef>,
    content: scan::ReshadeContent,
}

impl LumaPeerRootAuthority {
    /// Resolves ReShade's effective add-on root exactly once after the game
    /// root and current host path have been independently constrained.
    pub(crate) fn resolve(game_dir: &Path, current_host_path: &Path) -> Result<Self, ServiceError> {
        let canonical_game_root = crate::paths::canonical_candidate(game_dir)
            .map_err(|error| crate::failed(format!("Luma game root is not reachable: {error}")))?;
        let game_roots = PeerRoots::new(canonical_game_root.clone(), None)?;
        let current_host = path_ref(current_host_path)?;
        game_roots
            .require_game_path(&current_host)
            .map_err(|error| {
                crate::failed(format!(
                    "Luma ReShade host must be a strict descendant of the game root: {error}"
                ))
            })?;

        // Capture the configuration and its derived paths together. All later
        // access is served from this immutable snapshot, even if ReShade.ini
        // changes or disappears.
        let config =
            scan::resolve_strict_snapshot(game_dir, Some(current_host_path)).map_err(|error| {
                crate::failed(format!("Luma ReShade configuration is invalid: {error}"))
            })?;
        let content = config.assess_content(game_dir, &[]);
        let resolved = config.paths();
        let effective_addon_root = resolved.effective_addon_path.clone();
        let effective_addon_candidate = crate::paths::canonical_candidate(&effective_addon_root)
            .map_err(|error| {
                crate::failed(format!("Luma ReShade AddonPath is not reachable: {error}"))
            })?;
        let external_capability_root = match normalized_path_relation(
            &canonical_game_root.to_string_lossy(),
            &effective_addon_candidate.to_string_lossy(),
        ) {
            NormalizedPathRelation::Equal | NormalizedPathRelation::LeftAncestor => None,
            NormalizedPathRelation::RightAncestor => {
                return Err(crate::failed(
                    "Luma ReShade AddonPath must not be an ancestor of the runtime root",
                ));
            }
            NormalizedPathRelation::Disjoint => Some(effective_addon_candidate.clone()),
        };
        let roots = PeerRoots::new(canonical_game_root, external_capability_root.clone())?;
        let canonical_game_root = roots
            .roots()
            .first()
            .cloned()
            .ok_or_else(|| crate::failed("Luma authority has no canonical game root"))?;
        let canonical_game_root_ref = path_ref(&canonical_game_root)?;
        let effective_addon_root_ref = path_ref(&effective_addon_candidate)?;
        let external_capability_root_ref = external_capability_root
            .as_deref()
            .map(path_ref)
            .transpose()?;

        Ok(Self {
            roots,
            canonical_game_root,
            canonical_game_root_ref,
            effective_addon_root: effective_addon_candidate,
            effective_addon_root_ref,
            external_capability_root,
            external_capability_root_ref,
            content,
        })
    }

    /// Canonical game root for `PeerMutationRequest::game_root`.
    pub(crate) fn canonical_game_root(&self) -> &Path {
        &self.canonical_game_root
    }

    /// Canonical game root as a domain path reference.
    pub(crate) fn canonical_game_root_ref(&self) -> &PathRef {
        &self.canonical_game_root_ref
    }

    /// Exact effective ReShade add-on root sealed from `ReShade.ini`.
    pub(crate) fn effective_addon_root(&self) -> &Path {
        &self.effective_addon_root
    }

    /// Exact effective ReShade add-on root as a domain path reference.
    pub(crate) fn effective_addon_root_ref(&self) -> &PathRef {
        &self.effective_addon_root_ref
    }

    /// Optional external, disjoint filesystem authority required by the
    /// sealed add-on root. Nested add-on roots use the runtime authority.
    pub(crate) fn external_capability_root(&self) -> Option<&Path> {
        self.external_capability_root.as_deref()
    }

    /// Optional external capability root as a path reference.
    pub(crate) fn external_capability_root_ref(&self) -> Option<&PathRef> {
        self.external_capability_root_ref.as_ref()
    }

    /// Returns the one exact DLSS endpoint under the sealed effective payload
    /// root.  The conversion is deliberately fallible so an unrepresentable
    /// platform path cannot be replaced with a lossy surrogate.
    pub(crate) fn effective_dlss_target(&self) -> Result<PathRef, ServiceError> {
        let target = self
            .effective_addon_root()
            .join(renderpilot_detection::NVNGX_DLSS_FILE_NAME);
        let target = PathRef::from_canonical_native_absolute(&target).map_err(|error| {
            crate::failed(format!(
                "Luma effective payload root cannot form canonical nvngx_dlss.dll: {error}"
            ))
        })?;
        self.authorized_root(&target)?;
        Ok(target)
    }

    /// Content classification captured from the same configuration snapshot as
    /// the sealed roots. No filesystem or configuration read occurs here.
    pub(crate) fn content(&self) -> scan::ReshadeContent {
        self.content
    }

    /// Selects the exact sealed root for a path, preferring the game root.
    pub(crate) fn authorized_root(&self, path: &PathRef) -> Result<&PathRef, ServiceError> {
        if self.roots.require_game_path(path).is_ok() {
            return Ok(&self.canonical_game_root_ref);
        }
        if self.roots.require_sealed_path(path).is_ok() {
            return self.external_capability_root_ref.as_ref().ok_or_else(|| {
                crate::failed(format!(
                    "Luma path is under an unavailable sealed add-on root: {}",
                    path.as_str()
                ))
            });
        }
        Err(crate::failed(format!(
            "Luma path is outside the sealed game and add-on roots: {}",
            path.as_str()
        )))
    }

    /// Validates only Luma root/path authority before any filesystem
    /// observation. Domain transition and ownership validation remain below
    /// this boundary.
    pub(crate) fn validate<'a>(
        &self,
        before_peer: Option<&InstalledAddon>,
        after_peer: Option<&InstalledAddon>,
        catalog_claim: Option<&PeerCatalogRollbackClaim>,
        planned_endpoint_paths: impl IntoIterator<Item = &'a PathRef>,
    ) -> Result<(), ServiceError> {
        for record in [before_peer, after_peer].into_iter().flatten() {
            self.validate_record(record)?;
        }
        if let Some(claim) = catalog_claim {
            self.validate_catalog(claim)?;
        }
        for path in planned_endpoint_paths {
            self.authorized_root(path)?;
        }
        Ok(())
    }

    fn validate_record(&self, record: &InstalledAddon) -> Result<(), ServiceError> {
        if record.kind() != AddonKind::Luma {
            return Err(crate::failed(
                "Luma root authority received a non-Luma record",
            ));
        }
        let addon_file = record.addon_file();
        self.authorized_root(addon_file)?;
        let parent = Path::new(addon_file.as_str()).parent().ok_or_else(|| {
            crate::failed(format!(
                "Luma add-on file has no canonical parent: {addon_file}"
            ))
        })?;
        let canonical_parent = crate::paths::canonical_candidate(parent).map_err(|error| {
            crate::failed(format!(
                "Luma add-on file parent is not reachable: {addon_file}: {error}"
            ))
        })?;
        if !crate::paths::same_path(&canonical_parent, &self.effective_addon_root) {
            return Err(crate::failed(format!(
                "Luma add-on file parent drifted from the sealed ReShade AddonPath: {addon_file}"
            )));
        }

        for path in record
            .created_files()
            .iter()
            .chain(record.backed_up_files())
        {
            self.authorized_root(path)?;
        }
        for managed in record.managed_files() {
            self.authorized_root(managed.path())?;
        }
        if let Some(path) = record.registered_exe_path() {
            self.authorized_root(path)?;
        }
        Ok(())
    }

    fn validate_catalog(&self, claim: &PeerCatalogRollbackClaim) -> Result<(), ServiceError> {
        for component in claim
            .before_components()
            .iter()
            .chain(claim.after_components())
        {
            for file in component.files() {
                self.validate_catalog_file(file)?;
            }
        }
        for deleted in claim.deleted_baselines() {
            for file in deleted.baseline().files() {
                self.validate_catalog_file(file)?;
            }
        }
        Ok(())
    }

    fn validate_catalog_file(&self, file: &ComponentFile) -> Result<(), ServiceError> {
        self.authorized_root(file.path()).map(|_| ())
    }
}

fn path_ref(path: &Path) -> Result<PathRef, ServiceError> {
    PathRef::from_canonical_native_absolute(path).map_err(|error| {
        crate::failed(format!(
            "path is not a canonical Luma authority path: {} ({error})",
            path.display(),
        ))
    })
}
