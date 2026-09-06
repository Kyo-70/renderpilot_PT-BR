//! Pure contract for peer file and proxy-topology transitions.
//!
//! The module translates persisted peer/topology claims into a flat, ordered
//! endpoint program. It contains no filesystem, SQLite, or authority/permit
//! type; native execution and durable CAS remain outside the domain crate.

mod catalog;
mod catalog_merge;
mod catalog_physical;
mod catalog_physical_helpers;
mod claims;
mod derive;
mod dlss;
mod guards;
mod metadata;
mod model;
mod optiscaler_config;
mod planning;
mod reconcile;
mod renodx_reshade_ini;
mod reused_membership;
mod validation;

#[cfg(test)]
mod tests;

use crate::Sha256Hash;

pub use catalog::{PeerCatalogDeletedBaseline, PeerCatalogRollbackClaim};
pub use catalog_physical::PeerCatalogPhysicalContract;
pub use dlss::{RenoDxDlssBeforeImage, RenoDxDlssClaim, RenoDxDlssProjection};
pub use guards::{
    PeerReadGuardEvidence, PeerReadGuardExpectation, PeerReadGuardRequirement, PeerReadGuardSource,
    required_read_guards, required_read_guards_with_catalog,
    required_read_guards_with_renodx_reshade_ini,
    required_read_guards_with_renodx_reshade_ini_and_dlss,
    required_read_guards_with_renodx_reshade_ini_and_optiscaler_config, validate_read_guards,
};
pub use model::{
    CoordinatedPeerOperation, PeerEndpointEvidence, PeerEndpointIntent, PeerEndpointOperation,
    PeerEndpointRole, PeerFileImage, PeerTransitionAuthorities, PeerTransitionContext,
    PeerTransitionError, PlannedGameProxyTopology, ProxyPeerRoute,
};
pub use optiscaler_config::{
    ExactOptiConfigProjection, OptiConfigOperation, OptiScalerConfigAuthority,
    OptiScalerConfigCapability,
};
pub use renodx_reshade_ini::{
    RENODX_DLSS_FIX_INSTALL, RENODX_DLSS_FIX_UNINSTALL, RENODX_DLSS_FIX_UPDATE, RENODX_INSTALL,
    RENODX_INSTALL_FROM_FILE, RENODX_UNINSTALL, RenoDxReshadeIniAuthority, RenoDxReshadeIniFeature,
};
pub use reused_membership::PeerReusedClaimMembershipContract;
pub use validation::{
    validate_evidence, validate_intents, validate_intents_with_authorities,
    validate_intents_with_renodx_reshade_ini,
};

pub use claims::managed_sidecar_path;
pub use metadata::validate_peer_metadata_only;

/// Snapshot guard retained by a derived contract for exact typed preimages.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct EndpointGuard {
    pub(crate) before_sha256: Option<Sha256Hash>,
    pub(crate) before_length: Option<u64>,
    pub(crate) after_sha256: Option<Sha256Hash>,
    pub(crate) after_length: Option<u64>,
}

/// Complete pure-domain peer transition contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerTransitionContract {
    route: ProxyPeerRoute,
    intents: Vec<PeerEndpointIntent>,
    guards: Vec<EndpointGuard>,
    // Derived exclusively from the sealed topology while building a
    // coordinated contract. Revalidation must replay that same authority;
    // rebuilding it from consumer input would weaken the sealed boundary.
    topology_downstream: Option<crate::PathRef>,
    renodx_reshade_ini: Option<RenoDxReshadeIniAuthority>,
    optiscaler_config: Option<ExactOptiConfigProjection>,
    dlss_projection: Option<RenoDxDlssProjection>,
}

impl PeerTransitionContract {
    /// Derives a closed endpoint program from exact aggregate images and the
    /// caller's ordered physical program.
    pub fn derive_physical(
        before_peer: Option<&crate::InstalledAddon>,
        after_peer: Option<&crate::InstalledAddon>,
        before_topology: Option<&crate::GameProxyTopology>,
        planned_after_topology: Option<&PlannedGameProxyTopology>,
        route: ProxyPeerRoute,
        physical_program: Vec<PeerEndpointIntent>,
    ) -> Result<Self, PeerTransitionError> {
        derive::derive_physical(
            before_peer,
            after_peer,
            before_topology,
            planned_after_topology,
            route,
            physical_program,
        )
    }

    /// Derives a peer transition while merging one exact catalog rollback
    /// projection into the same physical program.
    pub fn derive_physical_with_catalog(
        before_peer: Option<&crate::InstalledAddon>,
        after_peer: Option<&crate::InstalledAddon>,
        before_topology: Option<&crate::GameProxyTopology>,
        planned_after_topology: Option<&PlannedGameProxyTopology>,
        route: ProxyPeerRoute,
        physical_program: Vec<PeerEndpointIntent>,
        catalog: Option<&PeerCatalogPhysicalContract>,
    ) -> Result<Self, PeerTransitionError> {
        derive::derive_physical_with_catalog(
            before_peer,
            after_peer,
            before_topology,
            planned_after_topology,
            route,
            physical_program,
            catalog,
        )
    }

    /// Derives a physical transition with the typed RenoDX ReShade.ini
    /// authority bound to its exact canonical game root.
    pub fn derive_physical_with_renodx_reshade_ini(
        before_peer: Option<&crate::InstalledAddon>,
        after_peer: Option<&crate::InstalledAddon>,
        before_topology: Option<&crate::GameProxyTopology>,
        planned_after_topology: Option<&PlannedGameProxyTopology>,
        route: ProxyPeerRoute,
        authority: RenoDxReshadeIniAuthority,
        physical_program: Vec<PeerEndpointIntent>,
    ) -> Result<Self, PeerTransitionError> {
        derive::derive_physical_scoped(
            PeerTransitionContext::new(
                before_peer,
                after_peer,
                before_topology,
                planned_after_topology,
                route,
            ),
            physical_program,
            None,
            Some(authority),
            None,
        )
    }

    /// Derives the narrow RenoDX proxy route that also updates OptiScaler's
    /// exact LoadReshade receipt. The projection grants no ownership of the
    /// INI; it only binds this one endpoint to the OptiScaler aggregate.
    pub fn derive_physical_with_renodx_reshade_ini_and_optiscaler_config(
        context: PeerTransitionContext<'_>,
        authority: Option<RenoDxReshadeIniAuthority>,
        optiscaler_config: ExactOptiConfigProjection,
        physical_program: Vec<PeerEndpointIntent>,
    ) -> Result<Self, PeerTransitionError> {
        derive::derive_physical_scoped(
            context,
            physical_program,
            None,
            authority,
            Some(optiscaler_config),
        )
    }

    /// Derives a peer transition carrying the exact RenoDX DLSS-Fix claim
    /// projection. The physical program may be empty only for this specialized
    /// projection, allowing an atomic claim-only repair without weakening the
    /// generic peer contract.
    pub fn derive_physical_with_renodx_reshade_ini_and_dlss(
        context: PeerTransitionContext<'_>,
        authority: Option<RenoDxReshadeIniAuthority>,
        physical_program: Vec<PeerEndpointIntent>,
        dlss_projection: RenoDxDlssProjection,
    ) -> Result<Self, PeerTransitionError> {
        dlss_projection.validate_against_peers(context.before_peer, context.after_peer)?;
        if physical_program.is_empty() {
            if authority.is_some() || !matches!(context.route, ProxyPeerRoute::DurableDisjoint) {
                return Err(PeerTransitionError::EmptyIntentSet);
            }
            validation::validate_dlss_physical_program(&[], &dlss_projection)?;
            let planned_shape = planning::topology_shape(
                context.before_topology,
                context.planned_after_topology,
                context.route,
            )?;
            planning::validate_shape(context.before_topology, &planned_shape)?;
            claims::validate_snapshots(
                context.before_peer,
                context.after_peer,
                context.before_topology,
                planned_shape.exact(),
            )?;
            return Ok(Self {
                route: context.route,
                intents: Vec::new(),
                guards: Vec::new(),
                topology_downstream: None,
                renodx_reshade_ini: None,
                optiscaler_config: None,
                dlss_projection: Some(dlss_projection),
            });
        }
        let mut contract =
            derive::derive_physical_scoped(context, physical_program, None, authority, None)?;
        validation::validate_dlss_physical_program(contract.intents(), &dlss_projection)?;
        contract.dlss_projection = Some(dlss_projection);
        Ok(contract)
    }

    /// Returns the closed route class.
    #[must_use]
    pub const fn route(&self) -> ProxyPeerRoute {
        self.route
    }

    /// Returns ordered endpoint intents.
    #[must_use]
    pub fn intents(&self) -> &[PeerEndpointIntent] {
        &self.intents
    }

    /// Validates the contract's endpoint program again at a consumer boundary.
    pub fn validate_intents(&self) -> Result<(), PeerTransitionError> {
        if self.intents.is_empty()
            && self.dlss_projection.is_some()
            && self.renodx_reshade_ini.is_none()
            && matches!(self.route, ProxyPeerRoute::DurableDisjoint)
        {
            return Ok(());
        }
        validation::validate_intents_with_authorities(
            self.route,
            &self.intents,
            &PeerTransitionAuthorities {
                topology_downstream: self.topology_downstream.clone(),
                renodx_reshade_ini: self.renodx_reshade_ini.clone(),
                optiscaler_config: self.optiscaler_config.clone(),
                dlss_fix: self.dlss_projection.clone(),
            },
        )
    }

    /// Returns the typed RenoDX ReShade.ini authority, when present.
    #[must_use]
    pub fn renodx_reshade_ini_authority(&self) -> Option<&RenoDxReshadeIniAuthority> {
        self.renodx_reshade_ini.as_ref()
    }

    /// Returns the exact OptiScaler configuration projection when present.
    #[must_use]
    pub fn optiscaler_config_projection(&self) -> Option<&ExactOptiConfigProjection> {
        self.optiscaler_config.as_ref()
    }

    /// Returns the exact DLSS-Fix claim projection, when this specialized
    /// contract carries one.
    #[must_use]
    pub fn renodx_dlss_projection(&self) -> Option<&RenoDxDlssProjection> {
        self.dlss_projection.as_ref()
    }

    /// Validates ordered runtime evidence, including snapshot guards and paired
    /// live/sidecar capture or restore invariants.
    pub fn validate_evidence(
        &self,
        evidence: &[PeerEndpointEvidence],
    ) -> Result<(), PeerTransitionError> {
        validation::validate_evidence_with_guards(&self.intents, evidence, &self.guards)
    }

    /// Validates ordered O1 preimages and rejects a stable generic replace
    /// whose planned postimage is byte-identical to the observed preimage.
    pub fn validate_preimages(
        &self,
        before: &[Option<PeerFileImage>],
    ) -> Result<(), PeerTransitionError> {
        if before.len() != self.intents.len() {
            return Err(PeerTransitionError::EvidenceCardinality {
                expected: self.intents.len(),
                actual: before.len(),
            });
        }
        for ((intent, guard), observed) in self.intents.iter().zip(&self.guards).zip(before) {
            match intent.operation() {
                PeerEndpointOperation::Create if observed.is_some() => {
                    return Err(PeerTransitionError::InvalidPreimage(intent.path().clone()));
                }
                PeerEndpointOperation::Replace | PeerEndpointOperation::Remove
                    if observed.is_none() =>
                {
                    return Err(PeerTransitionError::InvalidPreimage(intent.path().clone()));
                }
                _ => {}
            }
            if let (Some(image), Some(expected)) = (observed, guard.before_sha256.as_ref())
                && image.sha256() != expected
            {
                return Err(PeerTransitionError::DigestMismatch(intent.path().clone()));
            }
            if let (Some(image), Some(expected)) = (observed, guard.before_length)
                && image.length() != expected
            {
                return Err(PeerTransitionError::LengthMismatch(intent.path().clone()));
            }
            if intent.operation() == PeerEndpointOperation::Replace
                && guard.before_sha256.is_none()
                && intent.planned_sha256().is_some_and(|planned| {
                    observed
                        .as_ref()
                        .is_some_and(|image| image.sha256() == planned)
                })
                && intent.planned_length().is_some_and(|planned| {
                    observed
                        .as_ref()
                        .is_some_and(|image| image.length() == planned)
                })
            {
                return Err(PeerTransitionError::NoopEndpoint(intent.path().clone()));
            }
        }
        Ok(())
    }
}
