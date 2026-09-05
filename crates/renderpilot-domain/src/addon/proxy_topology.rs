//! Neutral aggregate for a game's root proxy slot.
//!
//! The topology deliberately has no OptiScaler lifecycle policy. It records
//! exact root pre-state, outer identity, and optional peer custody so multiple
//! features can reason about the same slot without introducing a second
//! ownership row.

use std::{collections::BTreeSet, error::Error, fmt};

use serde::{Deserialize, Serialize};

use crate::{FileReceipt, GameId, PathRef, normalized_path_key};

/// Recognized implementation occupying one logical proxy role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProxyImplementation {
    /// OptiScaler outer loader.
    OptiScaler,
    /// ReShade host.
    ReShade,
    /// Special K injection loader.
    SpecialK,
    /// Ultimate ASI Loader.
    UltimateAsiLoader,
    /// DLSS Enabler loader.
    DlssEnabler,
    /// Unrecognized external proxy.
    Unknown,
}

/// One physical link in a root-proxy chain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProxyLink {
    /// Recognized implementation role.
    pub implementation: ProxyImplementation,
    /// Physical DLL path.
    pub path: PathRef,
    /// Immutable committed file evidence.
    pub receipt: FileReceipt,
}

/// Exact origin of the root slot while an outer proxy is installed.
///
/// A relocated downstream DLL is itself the durable root pre-state. It is not
/// duplicated into a second `.bak` authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProxyRootPrestate {
    /// The root slot did not contain a file before the outer proxy.
    Absent,
    /// The prior root host is the topology's exact downstream link.
    RelocatedDownstream,
}

/// Aggregate coordinating a game's single root proxy slot and downstream host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameProxyTopology {
    /// Stable topology identifier.
    pub id: String,
    /// Owning game.
    pub game_id: GameId,
    /// DLL path loaded directly by the game.
    pub root_slot: PathRef,
    /// Outermost active implementation and exact postimage hash.
    pub outer: ProxyLink,
    /// Optional downstream implementation loaded by the outer proxy.
    pub downstream: Option<ProxyLink>,
    /// Exact path to which the downstream implementation returns when the
    /// outer proxy is removed. This is a transition target, not a second
    /// ownership record and not a historical snapshot of the root slot.
    #[serde(default)]
    pub downstream_origin: Option<PathRef>,
    /// Durable authority for the root slot pre-state.
    pub root_prestate: ProxyRootPrestate,
}

impl GameProxyTopology {
    /// Iterates every path participating in this topology.
    ///
    /// This is a topology/containment view only. It is not destructive
    /// cleanup authority; callers must check the corresponding typed
    /// [`FileReceipt`] ownership and identity before mutating a path.
    pub fn participant_paths(&self) -> impl Iterator<Item = &PathRef> {
        std::iter::once(&self.root_slot)
            .chain(self.downstream.iter().map(|link| &link.path))
            .chain(self.downstream_origin.iter())
    }

    /// Validates identity, path uniqueness, and cross-field ownership rules.
    pub fn validate(&self) -> Result<(), ProxyTopologyError> {
        self.outer
            .receipt
            .validate()
            .map_err(|_| ProxyTopologyError::InvalidFileReceipt)?;
        if let Some(downstream) = &self.downstream {
            downstream
                .receipt
                .validate()
                .map_err(|_| ProxyTopologyError::InvalidFileReceipt)?;
        }
        if self.id.trim().is_empty() {
            return Err(ProxyTopologyError::EmptyField("id"));
        }
        let root_key = normalized_path_key(self.root_slot.as_str());
        if root_key != normalized_path_key(self.outer.path.as_str()) {
            return Err(ProxyTopologyError::RootOuterPathMismatch);
        }
        let mut active = BTreeSet::new();
        active.insert(root_key.clone());
        if let Some(downstream) = &self.downstream
            && !active.insert(normalized_path_key(downstream.path.as_str()))
        {
            return Err(ProxyTopologyError::DuplicatePath(downstream.path.clone()));
        }
        if let (Some(downstream), Some(origin)) = (&self.downstream, &self.downstream_origin)
            && normalized_path_key(downstream.path.as_str()) == normalized_path_key(origin.as_str())
        {
            return Err(ProxyTopologyError::PeerOriginEqualsActive(origin.clone()));
        }
        match (
            self.root_prestate,
            self.downstream.as_ref(),
            self.downstream_origin.as_ref(),
        ) {
            (ProxyRootPrestate::Absent, None, None) => {}
            (
                ProxyRootPrestate::Absent | ProxyRootPrestate::RelocatedDownstream,
                Some(_),
                Some(origin),
            ) if normalized_path_key(origin.as_str()) == root_key => {}
            (
                ProxyRootPrestate::Absent | ProxyRootPrestate::RelocatedDownstream,
                Some(_),
                Some(origin),
            ) => {
                return Err(ProxyTopologyError::PeerOriginMustBeRoot(origin.clone()));
            }
            (ProxyRootPrestate::RelocatedDownstream, _, _) => {
                return Err(ProxyTopologyError::InvalidRelocatedRootPrestate);
            }
            (ProxyRootPrestate::Absent, _, _) => {
                return Err(ProxyTopologyError::IncompleteDownstreamOrigin);
            }
        }
        Ok(())
    }
}

impl ProxyImplementation {
    /// Stable lowercase wire identifier.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OptiScaler => "opti_scaler",
            Self::ReShade => "reshade",
            Self::SpecialK => "special_k",
            Self::UltimateAsiLoader => "ultimate_asi_loader",
            Self::DlssEnabler => "dlss_enabler",
            Self::Unknown => "unknown",
        }
    }
}

/// Invalid proxy-topology aggregate data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProxyTopologyError {
    /// A required string was empty.
    EmptyField(&'static str),
    /// Root slot and outer link do not identify the same physical path.
    RootOuterPathMismatch,
    /// A path occurred more than once.
    DuplicatePath(PathRef),
    /// The downstream origin is the active downstream path.
    PeerOriginEqualsActive(PathRef),
    /// A downstream link and its origin must be present or absent together.
    IncompleteDownstreamOrigin,
    /// Relocated root custody must point from the root slot to a downstream.
    InvalidRelocatedRootPrestate,
    /// The peer return target is always the exact root slot for this topology.
    PeerOriginMustBeRoot(PathRef),
    /// A topology receipt violates its non-filesystem invariants.
    InvalidFileReceipt,
}

impl fmt::Display for ProxyTopologyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyField(field) => {
                write!(formatter, "proxy topology {field} must not be empty")
            }
            Self::RootOuterPathMismatch => {
                formatter.write_str("proxy topology root slot differs from outer path")
            }
            Self::DuplicatePath(path) => write!(formatter, "duplicate proxy topology path: {path}"),
            Self::PeerOriginEqualsActive(path) => {
                write!(formatter, "proxy peer origin equals active path: {path}")
            }
            Self::IncompleteDownstreamOrigin => formatter
                .write_str("proxy topology downstream link and origin must be present together"),
            Self::InvalidRelocatedRootPrestate => formatter.write_str(
                "proxy topology relocated root pre-state must originate at the root slot",
            ),
            Self::PeerOriginMustBeRoot(path) => write!(
                formatter,
                "proxy topology peer return target must be the root slot, not {path}"
            ),
            Self::InvalidFileReceipt => {
                formatter.write_str("proxy topology file receipt is invalid")
            }
        }
    }
}

impl Error for ProxyTopologyError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn topology() -> GameProxyTopology {
        let proxy = PathRef::new("C:/Games/Test/dxgi.dll").expect("proxy");
        GameProxyTopology {
            id: "optiscaler:test".to_owned(),
            game_id: GameId::new("manual:test").expect("game"),
            root_slot: proxy.clone(),
            outer: ProxyLink {
                implementation: ProxyImplementation::OptiScaler,
                path: proxy,
                receipt: FileReceipt::owned(
                    "proxy-id",
                    crate::Sha256Hash::new("a".repeat(64)).expect("hash"),
                )
                .expect("receipt"),
            },
            downstream: None,
            downstream_origin: None,
            root_prestate: ProxyRootPrestate::Absent,
        }
    }

    #[test]
    fn validates_root_pre_state_and_outer_postimage() {
        topology().validate().expect("valid topology");
    }

    #[test]
    fn rejects_invalid_downstream_receipt_during_deserialization() {
        let mut value = serde_json::to_value(topology()).expect("serialize topology");
        value["downstream"] = serde_json::json!({
            "implementation": "reshade",
            "path": "C:/Games/Test/ReShade64.dll",
            "receipt": {
                "identity": "  ",
                "digest": "b".repeat(64),
                "ownership": "reused"
            }
        });
        value["downstream_origin"] = serde_json::json!("C:/Games/Test/dxgi.dll");

        assert!(serde_json::from_value::<GameProxyTopology>(value).is_err());
    }

    #[test]
    fn rejects_a_root_outer_path_mismatch() {
        let mut value = topology();
        value.outer.path = PathRef::new("C:/Games/Test/other.dll").unwrap();
        assert!(matches!(
            value.validate(),
            Err(ProxyTopologyError::RootOuterPathMismatch)
        ));
    }

    #[test]
    fn relocated_root_prestate_requires_an_exact_downstream_from_the_root_slot() {
        let mut value = topology();
        let root = value.root_slot.clone();
        value.downstream = Some(ProxyLink {
            implementation: ProxyImplementation::ReShade,
            path: PathRef::new("C:/Games/Test/ReShade64.dll").expect("downstream"),
            receipt: FileReceipt::reused(
                "peer-id",
                crate::Sha256Hash::new("b".repeat(64)).expect("hash"),
            )
            .expect("receipt"),
        });
        value.downstream_origin = Some(root);
        value.root_prestate = ProxyRootPrestate::RelocatedDownstream;
        value.validate().expect("relocated root pre-state");

        value.downstream_origin = None;
        assert!(matches!(
            value.validate(),
            Err(ProxyTopologyError::InvalidRelocatedRootPrestate)
        ));
    }

    #[test]
    fn downstream_and_origin_are_a_closed_pair() {
        let mut value = topology();
        value.downstream = Some(ProxyLink {
            implementation: ProxyImplementation::ReShade,
            path: PathRef::new("C:/Games/Test/ReShade64.dll").expect("downstream"),
            receipt: FileReceipt::reused(
                "peer-id",
                crate::Sha256Hash::new("b".repeat(64)).expect("hash"),
            )
            .expect("receipt"),
        });
        assert!(matches!(
            value.validate(),
            Err(ProxyTopologyError::IncompleteDownstreamOrigin)
        ));
    }

    #[test]
    fn absent_root_prestate_accepts_a_later_created_peer_returning_to_root() {
        let mut value = topology();
        value.downstream = Some(ProxyLink {
            implementation: ProxyImplementation::ReShade,
            path: PathRef::new("C:/Games/Test/ReShade64.dll").expect("downstream"),
            receipt: FileReceipt::reused(
                "peer-id",
                crate::Sha256Hash::new("b".repeat(64)).expect("hash"),
            )
            .expect("receipt"),
        });
        value.downstream_origin = Some(value.root_slot.clone());
        value.validate().expect("later-created peer topology");
    }

    #[test]
    fn peer_return_target_cannot_be_an_unrelated_historical_path() {
        let mut value = topology();
        value.downstream = Some(ProxyLink {
            implementation: ProxyImplementation::ReShade,
            path: PathRef::new("C:/Games/Test/ReShade64.dll").expect("downstream"),
            receipt: FileReceipt::reused(
                "peer-id",
                crate::Sha256Hash::new("b".repeat(64)).expect("hash"),
            )
            .expect("receipt"),
        });
        value.downstream_origin = Some(PathRef::new("C:/Games/Test/original.dll").expect("origin"));
        assert!(matches!(
            value.validate(),
            Err(ProxyTopologyError::PeerOriginMustBeRoot(_))
        ));
    }
}
