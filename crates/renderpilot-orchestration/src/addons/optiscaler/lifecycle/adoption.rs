//! Exact adoption of unmanaged immutable releases.

mod identity;
mod peer;
#[cfg(test)]
mod tests;

pub(crate) use identity::adopt_exact;
#[cfg(test)]
pub(in crate::addons::optiscaler) use peer::PeerSidecarTransition;
pub(in crate::addons::optiscaler) use peer::{
    PeerHostTransitionPlan, PeerReceiptTransition, PeerTopologyDirection, plan_peer_host_transition,
};
