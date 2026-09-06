//! Typed authority for the one OptiScaler configuration setting peers may
//! coordinate.
//!
//! OptiScaler owns OptiScaler.ini. A peer transition may carry an exact
//! configuration endpoint only when the root and the persisted Configuration
//! receipt are sealed together. The endpoint represents the
//! Plugins.LoadReshade invariant; it does not transfer ownership of the INI
//! or permit arbitrary key/value edits.

use crate::{
    OptiScalerFileReceipt, OptiScalerFileRole, PathRef, PeerEndpointIntent, PeerEndpointRole,
    PeerTransitionError, normalized_path_key,
};

use super::{EndpointGuard, reconcile::DerivedEndpoint};

/// The only OptiScaler configuration capability exposed to a peer route.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptiScalerConfigCapability {
    /// The peer may coordinate the Plugins.LoadReshade invariant.
    LoadReshade,
}

/// Semantic operation for the typed Plugins.LoadReshade invariant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptiConfigOperation {
    /// Set Plugins.LoadReshade=true after a downstream host is acquired.
    EnableLoadReshade,
    /// Set Plugins.LoadReshade=false before a downstream host is released.
    DisableLoadReshade,
    /// Leave the setting and its physical file untouched.
    NoChange,
}

impl OptiConfigOperation {
    /// Returns the only setting this operation can alter.
    #[must_use]
    pub const fn capability(self) -> OptiScalerConfigCapability {
        OptiScalerConfigCapability::LoadReshade
    }

    /// Returns the required semantic value, or None for no change.
    #[must_use]
    pub const fn load_reshade_value(self) -> Option<bool> {
        match self {
            Self::EnableLoadReshade => Some(true),
            Self::DisableLoadReshade => Some(false),
            Self::NoChange => None,
        }
    }

    /// Returns whether the operation needs a physical config endpoint.
    #[must_use]
    pub const fn requires_physical_endpoint(self) -> bool {
        !matches!(self, Self::NoChange)
    }
}

/// Sealed canonical game root and exact OptiScaler configuration path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OptiScalerConfigAuthority {
    canonical_game_root: PathRef,
    config_path: PathRef,
}

impl OptiScalerConfigAuthority {
    /// Builds authority for exactly canonical_game_root/OptiScaler.ini.
    pub fn new(canonical_game_root: PathRef) -> Result<Self, PeerTransitionError> {
        let root = canonical_game_root.as_str();
        let config = if root.ends_with('/') {
            format!("{root}OptiScaler.ini")
        } else {
            format!("{root}/OptiScaler.ini")
        };
        let config_path = PathRef::new(config).map_err(|_| {
            PeerTransitionError::InvalidOptiScalerConfigPath(canonical_game_root.clone())
        })?;
        Ok(Self {
            canonical_game_root,
            config_path,
        })
    }

    /// Returns the sealed canonical game root.
    #[must_use]
    pub fn canonical_game_root(&self) -> &PathRef {
        &self.canonical_game_root
    }

    /// Returns the only admitted physical config path.
    #[must_use]
    pub fn config_path(&self) -> &PathRef {
        &self.config_path
    }

    /// Checks a path against the exact config endpoint identity.
    #[must_use]
    pub fn matches_config_path(&self, path: &PathRef) -> bool {
        normalized_path_key(self.config_path.as_str()) == normalized_path_key(path.as_str())
    }

    /// Checks that a persisted receipt is the single Configuration binding
    /// for this authority.
    pub fn validate_receipt(
        &self,
        receipt: &OptiScalerFileReceipt,
    ) -> Result<(), PeerTransitionError> {
        receipt.installed.validate().map_err(|_| {
            PeerTransitionError::InvalidOptiScalerConfigReceipt(receipt.path.clone())
        })?;
        if receipt.role != OptiScalerFileRole::Configuration
            || !self.matches_config_path(&receipt.path)
        {
            return Err(PeerTransitionError::InvalidOptiScalerConfigReceipt(
                receipt.path.clone(),
            ));
        }
        Ok(())
    }
}

/// Exact config projection bound to one Configuration receipt and one typed
/// LoadReshade operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExactOptiConfigProjection {
    authority: OptiScalerConfigAuthority,
    receipt: OptiScalerFileReceipt,
    operation: OptiConfigOperation,
}

impl ExactOptiConfigProjection {
    /// Constructs and validates an exact projection.
    pub fn new(
        authority: OptiScalerConfigAuthority,
        receipt: OptiScalerFileReceipt,
        operation: OptiConfigOperation,
    ) -> Result<Self, PeerTransitionError> {
        authority.validate_receipt(&receipt)?;
        Ok(Self {
            authority,
            receipt,
            operation,
        })
    }

    /// Alias for callers rebuilding a projection from persisted parts.
    pub fn from_parts(
        authority: OptiScalerConfigAuthority,
        receipt: OptiScalerFileReceipt,
        operation: OptiConfigOperation,
    ) -> Result<Self, PeerTransitionError> {
        Self::new(authority, receipt, operation)
    }

    /// Returns the sealed authority.
    #[must_use]
    pub fn authority(&self) -> &OptiScalerConfigAuthority {
        &self.authority
    }

    /// Returns the one Configuration receipt.
    #[must_use]
    pub fn receipt(&self) -> &OptiScalerFileReceipt {
        &self.receipt
    }

    /// Returns the typed operation.
    #[must_use]
    pub const fn operation(&self) -> OptiConfigOperation {
        self.operation
    }

    /// Returns the only permitted configuration capability.
    #[must_use]
    pub const fn capability(&self) -> OptiScalerConfigCapability {
        self.operation.capability()
    }

    /// Returns a physical replacement intent, or no intent for NoChange.
    pub fn physical_intent(&self) -> Result<Option<PeerEndpointIntent>, PeerTransitionError> {
        self.validate()?;
        if !self.operation.requires_physical_endpoint() {
            return Ok(None);
        }
        Ok(Some(PeerEndpointIntent::replace(
            self.authority.config_path().clone(),
            PeerEndpointRole::OptiScalerConfig,
            Some(self.receipt.installed.digest().clone()),
            None,
        )?))
    }

    /// Revalidates authority, receipt, and operation invariants at a consumer
    /// boundary.
    pub fn validate(&self) -> Result<(), PeerTransitionError> {
        self.authority.validate_receipt(&self.receipt)?;
        Ok(())
    }
}

/// Reintroduces the one typed configuration endpoint after generic peer
/// derivation. The configuration is not a RenoDX record claim, so its only
/// authority is the exact OptiScaler projection supplied by the caller.
pub(super) fn bind(
    projection: &ExactOptiConfigProjection,
    physical_program: &[PeerEndpointIntent],
    endpoints: &mut Vec<DerivedEndpoint>,
) -> Result<(), PeerTransitionError> {
    projection.validate()?;
    let expected = projection
        .physical_intent()?
        .ok_or(PeerTransitionError::MissingOptiScalerConfigTransition)?;
    let matching = physical_program
        .iter()
        .filter(|intent| intent.role() == PeerEndpointRole::OptiScalerConfig)
        .collect::<Vec<_>>();
    let [actual] = matching.as_slice() else {
        return Err(PeerTransitionError::InvalidOptiScalerConfigCardinality(
            matching.len(),
        ));
    };
    let actual = *actual;
    if actual.path() != expected.path()
        || actual.role() != expected.role()
        || actual.operation() != expected.operation()
        || actual.planned_sha256() != expected.planned_sha256()
        || actual.planned_length().is_none()
    {
        return Err(PeerTransitionError::InvalidOptiScalerConfigTransition(
            "physical endpoint differs from its exact config projection",
        ));
    }
    endpoints.push(DerivedEndpoint {
        intent: actual.clone(),
        guard: EndpointGuard {
            after_sha256: Some(projection.receipt().installed.digest().clone()),
            ..EndpointGuard::default()
        },
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FileReceipt, PeerEndpointOperation, Sha256Hash};

    fn root() -> PathRef {
        PathRef::new(r"C:\Games\Test").expect("root")
    }

    fn receipt(path: &PathRef) -> OptiScalerFileReceipt {
        OptiScalerFileReceipt {
            path: path.clone(),
            installed: FileReceipt::owned(
                "config-id",
                Sha256Hash::new("a".repeat(64)).expect("digest"),
            )
            .expect("receipt"),
            role: OptiScalerFileRole::Configuration,
            cleanup: crate::OptiScalerFileCleanup::PreserveCurrentThenRestoreBaseline,
            baseline: crate::OptiScalerReleaseFileBaseline::Absent,
        }
    }

    #[test]
    fn binds_exact_root_config_path_and_load_reshade_only() {
        let authority = OptiScalerConfigAuthority::new(root()).expect("authority");
        assert_eq!(
            authority.config_path().as_str(),
            "C:/Games/Test/OptiScaler.ini"
        );
        let projection = ExactOptiConfigProjection::new(
            authority.clone(),
            receipt(authority.config_path()),
            OptiConfigOperation::EnableLoadReshade,
        )
        .expect("projection");
        assert_eq!(
            projection.capability(),
            OptiScalerConfigCapability::LoadReshade
        );
        let intent = projection
            .physical_intent()
            .expect("intent result")
            .expect("intent");
        assert_eq!(intent.role(), PeerEndpointRole::OptiScalerConfig);
        assert_eq!(intent.operation(), PeerEndpointOperation::Replace);
    }

    #[test]
    fn rejects_wrong_path_or_non_configuration_receipt() {
        let authority = OptiScalerConfigAuthority::new(root()).expect("authority");
        let mut wrong = receipt(authority.config_path());
        wrong.path = PathRef::new("C:/Games/Test/other.ini").expect("path");
        assert!(matches!(
            ExactOptiConfigProjection::new(
                authority.clone(),
                wrong,
                OptiConfigOperation::EnableLoadReshade,
            ),
            Err(PeerTransitionError::InvalidOptiScalerConfigReceipt(_))
        ));
        let mut wrong_role = receipt(authority.config_path());
        wrong_role.role = OptiScalerFileRole::Runtime;
        assert!(matches!(
            ExactOptiConfigProjection::new(
                authority,
                wrong_role,
                OptiConfigOperation::DisableLoadReshade,
            ),
            Err(PeerTransitionError::InvalidOptiScalerConfigReceipt(_))
        ));
    }

    #[test]
    fn no_change_has_no_physical_endpoint() {
        let authority = OptiScalerConfigAuthority::new(root()).expect("authority");
        let projection = ExactOptiConfigProjection::new(
            authority.clone(),
            receipt(authority.config_path()),
            OptiConfigOperation::NoChange,
        )
        .expect("projection");
        assert!(!projection.operation().requires_physical_endpoint());
        assert!(projection.physical_intent().expect("result").is_none());
    }
}
